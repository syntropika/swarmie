//! Provider-owned private model approvals. The runtime carries calls without interpreting them.
use crate::{
    ai::{ModelDecisions, ModelProtection},
    protection::{DecisionIntent, Detector, Inspection, ModelGuard},
};
use futures::{FutureExt, channel::oneshot, future::LocalBoxFuture};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    io::Write,
    path::PathBuf,
    rc::Rc,
};
use swarmie_permissions::{
    Authority, CorePlugin, ExecutionBinding, ExecutionContexts, HumanDecision, HumanSession,
    PluginGrant, ShellIdentity,
};
use tembra::{Context, Orb, Service, State};
use uuid::Uuid;

/// Private provider service. A public or agent transport must not admit this service.
pub const MODEL_PROTECTION_SERVICE: &str = "swarmie.providers.protection";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Preference {
    enabled: bool,
}

struct Pending {
    execution: ExecutionBinding,
    intent: DecisionIntent,
    send: oneshot::Sender<tembra::Result<HumanDecision>>,
}

impl Pending {
    fn live(&self) -> bool {
        self.execution.is_live() && !self.send.is_canceled()
    }
}

/// Native provider interaction engine. Omitting its UI Orb does not prevent mounting models.
/// A policy that requires an exception still requires an authenticated decision to dispatch.
pub struct NativeModelProtection {
    path: PathBuf,
    authority: Authority,
    _policy_authority: Authority,
    shell: HumanSession,
    principal: String,
    installation: Uuid,
    guard: ModelGuard,
    preference: RefCell<Preference>,
    available: bool,
    decisions_available: bool,
    pending: RefCell<BTreeMap<Uuid, Pending>>,
    revision: Cell<Uuid>,
}

fn unavailable() -> tembra::Error {
    tembra::Error::new(
        tembra::Code::Forbidden,
        MODEL_PROTECTION_SERVICE,
        "model approval is unavailable or its owner ended",
    )
}

impl NativeModelProtection {
    /// The embedding supplies an already authenticated private principal and native issuer.
    /// Existing accepted protection preference is used only when this file does not exist.
    pub fn open(
        path: PathBuf,
        authority: Authority,
        installation: Uuid,
        principal: String,
        initial_enabled: bool,
        decisions_available: bool,
    ) -> tembra::Result<Rc<Self>> {
        if principal.is_empty() || installation.is_nil() {
            return Err(unavailable());
        }
        let preference: Preference = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| unavailable())?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Preference {
                enabled: initial_enabled,
            },
            Err(_) => return Err(unavailable()),
        };
        // Policy administration has its own session; opening it must not replace
        // the independent execution owner's anonymous session in the shared issuer.
        let policy_authority = Authority::new();
        let shell = policy_authority.authenticate_shell(ShellIdentity {
            cloud: None,
            account: None,
            installation,
            generation: Uuid::new_v4(),
        });
        let detector = Detector::bundled();
        let available = detector.is_some();
        let guard = ModelGuard::new(
            "core.model-protection".into(),
            PluginGrant::bundled(CorePlugin::ModelProtection, "core.model-protection".into()),
            detector,
        );
        if !preference.enabled {
            let decision = shell
                .decide(&("model-protection-policy/v1", guard.generation(), false))
                .map_err(|_| unavailable())?;
            guard
                .configure(&shell, decision, false)
                .map_err(|_| unavailable())?;
        }
        Ok(Rc::new(Self {
            path,
            authority,
            _policy_authority: policy_authority,
            shell,
            principal,
            installation,
            guard,
            preference: RefCell::new(preference),
            available,
            decisions_available,
            pending: RefCell::new(BTreeMap::new()),
            revision: Cell::new(Uuid::new_v4()),
        }))
    }

    /// Existing provider extension interface, reused by native and custom compositions.
    pub fn protection(self: &Rc<Self>, executions: ExecutionContexts) -> ModelProtection {
        ModelProtection::Guarded {
            guard: self.guard.clone(),
            executions,
            decisions: self.clone(),
        }
    }

    fn check_execution(&self, execution: &ExecutionBinding) -> tembra::Result<()> {
        if !self.shell.is_live()
            || !execution.is_live()
            || execution.shell().identity().installation != self.installation
        {
            return Err(unavailable());
        }
        self.authority
            .check_binding(execution.shell())
            .map_err(|_| unavailable())
    }

    fn prune(&self) {
        let mut pending = self.pending.borrow_mut();
        let previous = pending.len();
        pending.retain(|_, item| item.live());
        if pending.len() != previous {
            self.revision.set(Uuid::new_v4());
        }
    }

    fn project(&self) -> Value {
        self.prune();
        let pending = self.pending.borrow();
        let cards = pending
            .iter()
            .map(|(id, item)| {
                let (inspection, rules) = match &item.intent.inspection {
                    Inspection::Detected { findings } => (
                        "detected",
                        findings.iter().map(|finding| finding.rule.clone()).collect::<Vec<_>>(),
                    ),
                    Inspection::Incomplete => ("incomplete", vec![]),
                    _ => ("unavailable", vec![]),
                };
                json!({"id":id,"kind":"model","conversation":item.intent.execution.conversation,
                    "title":"Sending was stopped","purpose":"Review this exact model attempt before sending.",
                    "destination":item.intent.destination,"inspection":inspection,"rules":rules})
            })
            .collect::<Vec<_>>();
        let mut counts = BTreeMap::<String, usize>::new();
        for item in pending.values() {
            let conversation = item
                .intent
                .execution
                .conversation
                .map(|id| id.to_string())
                .unwrap_or_default();
            *counts.entry(conversation).or_default() += 1;
        }
        json!({"revision":self.revision.get(),"cards":cards,"cardCounts":counts,"detectorEnabled":self.preference.borrow().enabled,"detectorAvailable":self.available})
    }

    fn decide(&self, id: Uuid, choice: &str) -> tembra::Result<()> {
        if !self.decisions_available || !self.shell.is_live() {
            return Err(unavailable());
        }
        if choice != "cancel" && choice != "allow-once" {
            return Err(unavailable());
        }
        let pending = self
            .pending
            .borrow_mut()
            .remove(&id)
            .ok_or_else(unavailable)?;
        self.revision.set(Uuid::new_v4());
        self.check_execution(&pending.execution)?;
        if !pending.live() {
            return Err(unavailable());
        }
        if choice == "cancel" {
            return Ok(());
        }
        let decision = self
            .authority
            .decide_for(
                pending.execution.shell(),
                &("model-send/v1", &pending.intent),
            )
            .map_err(|_| unavailable())?;
        let _ = pending.send.send(Ok(decision));
        Ok(())
    }

    fn configure(&self, enabled: bool) -> tembra::Result<()> {
        if !self.decisions_available || !self.shell.is_live() {
            return Err(unavailable());
        }
        let parent = self.path.parent().ok_or_else(unavailable)?;
        let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|_| unavailable())?;
        let preference = Preference { enabled };
        serde_json::to_writer(&mut file, &preference).map_err(|_| unavailable())?;
        file.flush().map_err(|_| unavailable())?;
        file.as_file().sync_all().map_err(|_| unavailable())?;
        file.persist(&self.path).map_err(|_| unavailable())?;
        let decision = self
            .shell
            .decide(&(
                "model-protection-policy/v1",
                self.guard.generation(),
                enabled,
            ))
            .map_err(|_| unavailable())?;
        self.guard
            .configure(&self.shell, decision, enabled)
            .map_err(|_| unavailable())?;
        *self.preference.borrow_mut() = preference;
        self.revision.set(Uuid::new_v4());
        Ok(())
    }

    pub fn close(&self) {
        self.pending.borrow_mut().clear();
        self.revision.set(Uuid::new_v4());
        self.shell.close();
    }

    fn authorize(&self, context: &Context) -> tembra::Result<()> {
        context.check()?;
        if context.principal()? != self.principal || !self.shell.is_live() {
            return Err(unavailable());
        }
        Ok(())
    }

    /// Optional private presentation endpoint; policy and final dispatch remain provider-owned.
    pub fn orb(self: Rc<Self>) -> Orb {
        Orb::new(MODEL_PROTECTION_SERVICE, 1).service(
            Service::new(MODEL_PROTECTION_SERVICE, move |declaration| {
                if declaration.address()?.principal != self.principal
                    || declaration.address()?.key != ["main"]
                {
                    return Err(unavailable());
                }
                Ok(Presentation {
                    owner: self.clone(),
                    state: declaration
                        .state(json!({"revision":self.revision.get()}), 1, |_| Ok(()))?,
                    cache: RefCell::new(None),
                })
            })
            .method("read", |env, context, (): ()| async move {
                env.page(&context, None)
            })
            .method("page", |env, context, (cursor,): (Cursor,)| async move {
                env.page(&context, Some(cursor))
            })
            .method(
                "decide",
                |env, context, (id, choice): (Uuid, String)| async move {
                    env.owner.authorize(&context)?;
                    env.owner.decide(id, &choice)?;
                    env.publish(&context)
                },
            )
            .method(
                "configure",
                |env, context, (enabled,): (bool,)| async move {
                    env.owner.authorize(&context)?;
                    env.owner.configure(enabled)?;
                    env.publish(&context)
                },
            ),
        )
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    revision: Uuid,
    offset: usize,
}

#[derive(Deserialize, Serialize)]
struct Page {
    revision: Uuid,
    data: String,
    next: Option<Cursor>,
}

struct CachedProjection {
    revision: Uuid,
    document: String,
}

struct Presentation {
    owner: Rc<NativeModelProtection>,
    state: State<Value>,
    cache: RefCell<Option<CachedProjection>>,
}

impl Presentation {
    fn publish(&self, context: &Context) -> tembra::Result<Value> {
        self.cache.borrow_mut().take();
        let next = json!({"revision":self.owner.revision.get()});
        // The policy or decision already settled. Observation cannot reverse its outcome.
        let _ = self.state.update(context, |_| Ok(next.clone()));
        Ok(next)
    }

    fn page(&self, context: &Context, cursor: Option<Cursor>) -> tembra::Result<Page> {
        self.owner.authorize(context)?;
        // Pruning retires dead decisions and advances the view revision before cursor checks.
        self.owner.prune();
        let revision = self.owner.revision.get();
        let offset = match cursor {
            None => {
                let document =
                    serde_json::to_string(&self.owner.project()).map_err(|_| unavailable())?;
                *self.cache.borrow_mut() = Some(CachedProjection { revision, document });
                0
            }
            Some(cursor) if cursor.revision == revision => cursor.offset,
            Some(_) => return Err(stale_projection()),
        };
        let cache = self.cache.borrow();
        let cached = cache
            .as_ref()
            .filter(|cached| cached.revision == revision)
            .ok_or_else(stale_projection)?;
        if offset >= cached.document.len() || !cached.document.is_char_boundary(offset) {
            return Err(stale_projection());
        }
        let mut end = offset.saturating_add(8192).min(cached.document.len());
        while !cached.document.is_char_boundary(end) {
            end -= 1;
        }
        loop {
            let page = Page {
                revision,
                data: cached.document[offset..end].into(),
                next: (end < cached.document.len()).then_some(Cursor {
                    revision,
                    offset: end,
                }),
            };
            if serde_json::to_vec(&page).map_err(|_| unavailable())?.len() <= 48 * 1024 {
                return Ok(page);
            }
            end = offset + (end - offset) / 2;
            while !cached.document.is_char_boundary(end) {
                end -= 1;
            }
            if end == offset {
                return Err(unavailable());
            }
        }
    }
}

fn stale_projection() -> tembra::Error {
    tembra::Error::new(
        tembra::Code::Conflict,
        MODEL_PROTECTION_SERVICE,
        "the model approval projection changed",
    )
}

impl ModelDecisions for NativeModelProtection {
    fn request<'a>(
        &'a self,
        execution: &'a ExecutionBinding,
        intent: &'a DecisionIntent,
    ) -> LocalBoxFuture<'a, tembra::Result<HumanDecision>> {
        async move {
            self.check_execution(execution)?;
            if !self.decisions_available
                || intent.execution != *execution.identity()
                || intent.shell != *execution.shell().identity()
            {
                return Err(unavailable());
            }
            let (send, receive) = oneshot::channel();
            {
                let mut pending = self.pending.borrow_mut();
                pending.retain(|_, item| item.live());
                if pending.len() >= 64 {
                    return Err(unavailable());
                }
                self.revision.set(Uuid::new_v4());
                pending.insert(
                    Uuid::new_v4(),
                    Pending {
                        execution: execution.clone(),
                        intent: intent.clone(),
                        send,
                    },
                );
            }
            receive.await.map_err(|_| unavailable())?
        }
        .boxed_local()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protection::{Coverage, ModelAttempt};
    use futures::{executor::LocalPool, poll};
    use swarmie_permissions::ExecutionIdentity;

    fn owner(authority: &Authority, installation: Uuid) -> HumanSession {
        authority.authenticate_shell(ShellIdentity {
            cloud: None,
            account: None,
            installation,
            generation: Uuid::new_v4(),
        })
    }

    fn execution(authority: &Authority, owner: &HumanSession) -> ExecutionBinding {
        authority
            .execution(
                owner,
                ExecutionIdentity {
                    account: None,
                    execution: Uuid::new_v4(),
                    generation: Uuid::new_v4(),
                    project: None,
                    agent: None,
                    conversation: Some(Uuid::new_v4()),
                    main_agent: true,
                },
            )
            .unwrap()
    }

    #[test]
    fn opening_provider_policy_preserves_execution_owner_and_absent_ui_keeps_base_working() {
        let directory = tempfile::tempdir().unwrap();
        let authority = Authority::new();
        let installation = Uuid::new_v4();
        let human = owner(&authority, installation);
        let execution = execution(&authority, &human);
        let engine = NativeModelProtection::open(
            directory.path().join("protection.json"),
            authority,
            installation,
            "private".into(),
            false,
            false,
        )
        .unwrap();
        assert!(human.is_live());
        // No UI Orb is installed. Explicitly disabled inspection still admits base requests.
        let prepared = engine
            .guard
            .prepare(
                execution.clone(),
                ModelAttempt::new(Uuid::new_v4()),
                "fixture".into(),
                "https://example.invalid/model".into(),
                b"synthetic model input".to_vec(),
                Coverage::TextJson,
            )
            .unwrap();
        assert!(matches!(
            prepared.intent().inspection,
            Inspection::Unprotected
        ));
        assert!(
            futures::executor::block_on(engine.request(&execution, prepared.intent())).is_err()
        );
        assert!(execution.is_live());
        assert!(engine.project()["cards"].as_array().unwrap().is_empty());
    }

    #[test]
    fn private_decision_is_exact_one_use_and_retirement_removes_pending_requests() {
        let directory = tempfile::tempdir().unwrap();
        let authority = Authority::new();
        let installation = Uuid::new_v4();
        let human = owner(&authority, installation);
        let execution = execution(&authority, &human);
        let engine = NativeModelProtection::open(
            directory.path().join("protection.json"),
            authority.clone(),
            installation,
            "private".into(),
            true,
            true,
        )
        .unwrap();
        let prepared = engine
            .guard
            .prepare(
                execution.clone(),
                ModelAttempt::new(Uuid::new_v4()),
                "fixture".into(),
                "https://example.invalid/model".into(),
                b"synthetic input".to_vec(),
                Coverage::TextJson,
            )
            .unwrap();
        let intent = prepared.intent().clone();
        let mut pool = LocalPool::new();
        pool.run_until(async {
            let pending = engine.request(&execution, &intent);
            futures::pin_mut!(pending);
            assert!(poll!(&mut pending).is_pending());
            let id = engine.project()["cards"][0]["id"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap();
            assert!(engine.decide(id, "conversation").is_err());
            assert!(engine.decide(id, "allow-once").is_ok());
            assert!(engine.decide(id, "allow-once").is_err());
            let decision = pending.await.unwrap();
            assert!(
                decision
                    .consume_owner(execution.shell(), &("model-send/v1", &intent))
                    .is_ok()
            );
            assert!(
                decision
                    .consume_owner(execution.shell(), &("model-send/v1", &intent))
                    .is_err()
            );
            let cancelled = engine.request(&execution, &intent);
            futures::pin_mut!(cancelled);
            assert!(poll!(&mut cancelled).is_pending());
            execution.close();
            assert!(engine.project()["cards"].as_array().unwrap().is_empty());
            assert!(cancelled.await.is_err());
        });
    }

    #[test]
    fn private_orb_rejects_other_principals_and_preferences_survive_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("protection.json");
        let authority = Authority::new();
        let installation = Uuid::new_v4();
        let engine = NativeModelProtection::open(
            path.clone(),
            authority.clone(),
            installation,
            "private".into(),
            true,
            true,
        )
        .unwrap();
        engine.configure(false).unwrap();
        let mut pool = LocalPool::new();
        let host = tembra::Host::new(pool.spawner());
        host.install(vec![engine.orb()]).unwrap();
        let denied = pool.run_until(host.root("public", |context| {
            async move {
                let reference = context.get(
                    MODEL_PROTECTION_SERVICE,
                    vec!["main".into()],
                    Default::default(),
                )?;
                reference.ready().await?;
                Ok(())
            }
            .boxed_local()
        }));
        assert!(denied.is_err());
        pool.run_until(host.close()).unwrap();
        let reopened = NativeModelProtection::open(
            path,
            authority,
            installation,
            "private".into(),
            true,
            true,
        )
        .unwrap();
        assert!(!reopened.project()["detectorEnabled"].as_bool().unwrap());
    }

    #[test]
    fn large_escaped_destinations_are_lossless_paged_and_stale_cursors_are_refused() {
        let directory = tempfile::tempdir().unwrap();
        let authority = Authority::new();
        let installation = Uuid::new_v4();
        let human = owner(&authority, installation);
        let execution = execution(&authority, &human);
        let engine = NativeModelProtection::open(
            directory.path().join("protection.json"),
            authority,
            installation,
            "private".into(),
            true,
            true,
        )
        .unwrap();
        let destination = format!("https://example.invalid/{}", "雪\\\"\n".repeat(100_000));
        let prepared = engine
            .guard
            .prepare(
                execution.clone(),
                ModelAttempt::new(Uuid::new_v4()),
                "fixture".into(),
                destination.clone(),
                b"input".to_vec(),
                Coverage::TextJson,
            )
            .unwrap();
        let (send, _receiver) = oneshot::channel();
        engine.pending.borrow_mut().insert(
            Uuid::new_v4(),
            Pending {
                execution,
                intent: prepared.intent().clone(),
                send,
            },
        );
        let mut pool = LocalPool::new();
        let host = tembra::Host::new(pool.spawner());
        host.install(vec![engine.clone().orb()]).unwrap();
        let owner = engine.clone();
        pool.run_until(host.root("private", move |context| {
            async move {
                let reference = context.get(
                    MODEL_PROTECTION_SERVICE,
                    vec!["main".into()],
                    Default::default(),
                )?;
                reference.ready().await?;
                let mut page: Page = reference.call(&context, "read", ()).await?;
                let mut data = String::new();
                let mut pages = 0;
                loop {
                    assert!(serde_json::to_vec(&page).unwrap().len() <= 48 * 1024);
                    data.push_str(&page.data);
                    pages += 1;
                    let Some(cursor) = page.next else {
                        break;
                    };
                    page = reference.call(&context, "page", (cursor,)).await?;
                }
                assert!(pages > 1);
                let decoded: Value = serde_json::from_str(&data).unwrap();
                assert_eq!(decoded["cards"][0]["destination"], destination);
                let first: Page = reference.call(&context, "read", ()).await?;
                owner.configure(false)?;
                let stale: tembra::Result<Page> = reference
                    .call(&context, "page", (first.next.unwrap(),))
                    .await;
                assert!(stale.is_err());
                Ok(())
            }
            .boxed_local()
        }))
        .unwrap();
        pool.run_until(host.close()).unwrap();
    }
}
