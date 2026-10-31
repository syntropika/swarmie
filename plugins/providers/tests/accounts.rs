use futures::{FutureExt, executor::LocalPool, future::LocalBoxFuture};
use serde_json::json;
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU64,
    rc::Rc,
    sync::Arc,
};
use swarmie_ai::catalog::{
    Capability, Evidence, Layer, LimitClaim, Limits, ModelIdentity, Precision, Preparation,
    Requirements, Snapshot, SourceRecord,
};
use swarmie_providers::{
    AccountRecord, CapturedModel, CredentialRequirement, Model, ModelConstruction, ModelFactory,
    ModelMetadataInput, ModelRequest, ModelResponse, ModelSelection, OperationId,
    accounts::{
        AccountCandidate, AccountDriver, AccountStore, Accounts, AuthorizationFlow,
        CredentialStore, ModelChoice, SelectModel, StoreWriteError,
    },
    settings::{Account, ConfigureProvider, ModelDefaults, ModelInfo, ProviderSettings},
};
use tembra::{Context, Host};
use zeroize::Zeroizing;

fn error(reason: &str) -> tembra::Error {
    tembra::Error::new(tembra::Code::Unavailable, reason, "fixture failure")
}

use swarmie_providers::accounts::recovery::*;
use uuid::Uuid;

#[derive(Clone, Copy)]
enum Write {
    Applied,
    NotApplied,
    UnknownApplied,
    UnknownNotApplied,
}

struct Store {
    owner: ProviderOwner,
    image: RefCell<ProviderState>,
    fault: RefCell<Option<(Option<RecoveryPhase>, Write)>>,
    pause: Cell<Option<RecoveryPhase>>,
    cancel_at: RefCell<Option<(RecoveryPhase, Context)>>,
    unavailable: Cell<bool>,
    classification: Cell<u8>,
    saves: Cell<usize>,
    loads: Cell<usize>,
    initialized: Cell<usize>,
}

impl AccountStore for Store {
    fn load(&self) -> LocalBoxFuture<'_, tembra::Result<StateLoad>> {
        async move {
            self.loads.set(self.loads.get() + 1);
            if self.unavailable.get() {
                return Err(error("fixture.barrier_unavailable"));
            }
            match self.classification.get() {
                0 => Ok(StateLoad::Initialized(self.image.borrow().clone())),
                1 => Ok(StateLoad::Absent),
                2 => Ok(StateLoad::Legacy(LegacyObservation::from_bytes(
                    self.owner.clone(),
                    b"[]".to_vec(),
                    RecoveryLimits::default(),
                )?)),
                3 => Ok(StateLoad::Provisioning(ProvisioningPlan::new(
                    self.owner.clone(),
                    Uuid::from_u128(99),
                )?)),
                _ => Err(error("fixture.corrupt")),
            }
        }
        .boxed_local()
    }

    fn initialize(
        &self,
        plan: ProvisioningPlan,
        initial: ProviderState,
    ) -> LocalBoxFuture<'_, Result<(), StoreWriteError>> {
        async move {
            assert_eq!(self.classification.get(), 3);
            assert!(plan.owner() == &self.owner && initial.owner() == &self.owner);
            assert_eq!(initial.revision(), plan.initial_target());
            assert!(
                initial.accounts().is_empty()
                    && initial.credentials().is_empty()
                    && initial.workflow().is_none()
            );
            self.initialized.set(self.initialized.get() + 1);
            *self.image.borrow_mut() = initial;
            self.classification.set(0);
            Ok(())
        }
        .boxed_local()
    }

    fn adopt(
        &self,
        _expected: LegacyPredecessor,
        _initial: ProviderState,
    ) -> LocalBoxFuture<'_, Result<(), StoreWriteError>> {
        async { panic!("legacy metadata does not authorize protected adoption") }.boxed_local()
    }

    fn save(
        &self,
        expected: Uuid,
        next: ProviderState,
    ) -> LocalBoxFuture<'_, Result<(), StoreWriteError>> {
        async move {
            self.saves.set(self.saves.get() + 1);
            if self.image.borrow().revision() != expected {
                return Err(StoreWriteError::NotApplied(error("fixture.cas")));
            }
            let phase = next.workflow().map(RecoveryRecord::phase);
            let outcome = if self
                .fault
                .borrow()
                .as_ref()
                .is_some_and(|(target, _)| *target == phase)
            {
                self.fault.borrow_mut().take().unwrap().1
            } else {
                Write::Applied
            };
            if matches!(outcome, Write::Applied | Write::UnknownApplied) {
                *self.image.borrow_mut() = next;
            }
            let cancel = self
                .cancel_at
                .borrow()
                .as_ref()
                .is_some_and(|(target, _)| Some(*target) == phase);
            if cancel && let Some((_, context)) = self.cancel_at.borrow_mut().take() {
                let _ = context.cancel();
            }
            if self.pause.get().is_some() && self.pause.get() == phase {
                self.pause.set(None);
                futures::future::pending::<()>().await;
            }
            match outcome {
                Write::Applied => Ok(()),
                Write::NotApplied => Err(StoreWriteError::NotApplied(error("fixture.not_applied"))),
                _ => Err(StoreWriteError::Unknown(error("fixture.unknown"))),
            }
        }
        .boxed_local()
    }
}

#[derive(Default)]
struct Secrets {
    values: RefCell<BTreeMap<String, String>>,
    owned: RefCell<BTreeSet<String>>,
    stage_fault: Cell<Option<Write>>,
    retire_fault: Cell<Option<Write>>,
    collide: Cell<bool>,
    partial: Cell<bool>,
    unavailable: Cell<bool>,
    reject_plan: Cell<bool>,
    reads: Cell<usize>,
    stages: Cell<usize>,
    retires: Cell<usize>,
}

fn key(reference: &CredentialReference) -> String {
    serde_json::to_string(reference).unwrap()
}

impl CredentialStore for Secrets {
    fn check_reference(&self, reference: &CredentialReference) -> tembra::Result<()> {
        if self.reject_plan.get() || reference.owner().principal() != "alice" {
            return Err(error("fixture.reference"));
        }
        Ok(())
    }

    fn read(
        &self,
        reference: CredentialReference,
    ) -> LocalBoxFuture<'_, tembra::Result<CredentialRead>> {
        async move {
            self.reads.set(self.reads.get() + 1);
            if self.unavailable.get() {
                return Err(error("fixture.custody_unavailable"));
            }
            Ok(match self.values.borrow().get(&key(&reference)) {
                None => CredentialRead::Missing,
                Some(value) if value == "partial" => CredentialRead::Partial,
                Some(value) if value == "invalid" => CredentialRead::Invalid,
                Some(value) => CredentialRead::Complete(Zeroizing::new(value.clone())),
            })
        }
        .boxed_local()
    }

    fn stage(
        &self,
        reference: CredentialReference,
        value: Zeroizing<String>,
    ) -> LocalBoxFuture<'_, Result<(), StoreWriteError>> {
        async move {
            self.stages.set(self.stages.get() + 1);
            let key = key(&reference);
            if self.collide.replace(false) {
                self.values
                    .borrow_mut()
                    .insert(key.clone(), "collision".into());
            }
            if let Some(existing) = self.values.borrow().get(&key) {
                return if existing == value.as_str() {
                    Ok(())
                } else {
                    Err(StoreWriteError::NotApplied(error("fixture.collision")))
                };
            }
            if self.partial.replace(false) {
                self.values.borrow_mut().insert(key, "partial".into());
                return Err(StoreWriteError::Unknown(error("fixture.partial")));
            }
            let outcome = self.stage_fault.take().unwrap_or(Write::Applied);
            if matches!(outcome, Write::Applied | Write::UnknownApplied) {
                self.owned.borrow_mut().insert(key.clone());
                self.values.borrow_mut().insert(key, value.to_string());
            }
            match outcome {
                Write::Applied => Ok(()),
                Write::NotApplied => Err(StoreWriteError::NotApplied(error("fixture.not_applied"))),
                _ => Err(StoreWriteError::Unknown(error("fixture.unknown"))),
            }
        }
        .boxed_local()
    }

    fn retire(
        &self,
        reference: CredentialReference,
    ) -> LocalBoxFuture<'_, Result<(), StoreWriteError>> {
        async move {
            self.retires.set(self.retires.get() + 1);
            let key = key(&reference);
            if !self.owned.borrow().contains(&key) {
                return Err(StoreWriteError::NotApplied(error("fixture.no_ownership")));
            }
            let outcome = self.retire_fault.take().unwrap_or(Write::Applied);
            if matches!(outcome, Write::Applied | Write::UnknownApplied) {
                self.values.borrow_mut().remove(&key);
            }
            match outcome {
                Write::Applied => Ok(()),
                Write::NotApplied => Err(StoreWriteError::NotApplied(error("fixture.not_applied"))),
                _ => Err(StoreWriteError::Unknown(error("fixture.unknown"))),
            }
        }
        .boxed_local()
    }
}

struct Driver {
    store: Rc<Store>,
    mode: Cell<CredentialRequirement>,
    replacement: RefCell<Option<String>>,
    binding: Cell<u8>,
    renew_error: Cell<bool>,
    discovery_error: Cell<bool>,
    configures: Cell<usize>,
    exchanges: Cell<usize>,
    accepts: Cell<usize>,
    begins: Cell<usize>,
    times: RefCell<Vec<u64>>,
    polls: Rc<Cell<usize>>,
    closes: Rc<Cell<usize>>,
}

impl Driver {
    fn candidate(&self) -> AccountCandidate {
        let (account, mut private) = record("candidate").into_parts();
        private.insert("discovery_revision".into(), json!(2));
        AccountCandidate {
            record: AccountRecord::new(account, private).unwrap(),
            credential: (self.mode.get() == CredentialRequirement::Account)
                .then(|| Zeroizing::new("issued".into())),
            association_key: Some("verified-scope-and-subject".into()),
        }
    }
}

impl AccountDriver for Driver {
    fn protocol_binding(&self) -> tembra::Result<ProtocolBinding> {
        Ok(ProtocolBinding::new([self.binding.get(); 32]))
    }
    fn needs_renewal(
        &self,
        _account: &AccountRecord,
        _credential: &Zeroizing<String>,
        now: u64,
    ) -> tembra::Result<bool> {
        self.times.borrow_mut().push(now);
        Ok(self.replacement.borrow().is_some())
    }
    fn configure(
        &self,
        _input: ConfigureProvider,
    ) -> LocalBoxFuture<'_, tembra::Result<AccountCandidate>> {
        async move {
            assert!(self.store.image.borrow().workflow().unwrap().phase() == RecoveryPhase::Intent);
            self.configures.set(self.configures.get() + 1);
            Ok(self.candidate())
        }
        .boxed_local()
    }
    fn begin_authorization<'a>(
        &'a self,
        _provider: &'a str,
        _returning: Option<&'a Zeroizing<String>>,
    ) -> LocalBoxFuture<'a, tembra::Result<Box<dyn AuthorizationFlow>>> {
        async move {
            assert!(
                self.store.image.borrow().workflow().unwrap().phase()
                    == RecoveryPhase::ExchangeMayStart
            );
            self.begins.set(self.begins.get() + 1);
            Ok(Box::new(Flow {
                polls: self.polls.clone(),
                closes: self.closes.clone(),
            }) as Box<dyn AuthorizationFlow>)
        }
        .boxed_local()
    }
    fn accept_authorization<'a>(
        &'a self,
        _provider: &'a str,
        _credential: &'a Zeroizing<String>,
    ) -> LocalBoxFuture<'a, tembra::Result<AccountCandidate>> {
        async move {
            assert!(
                self.store.image.borrow().workflow().unwrap().phase() == RecoveryPhase::IssuedReady
            );
            self.accepts.set(self.accepts.get() + 1);
            if self.discovery_error.replace(false) {
                return Err(error("fixture.discovery"));
            }
            Ok(self.candidate())
        }
        .boxed_local()
    }
    fn renew<'a>(
        &'a self,
        _account: &'a AccountRecord,
        _credential: &'a Zeroizing<String>,
        now: u64,
    ) -> LocalBoxFuture<'a, tembra::Result<Option<Zeroizing<String>>>> {
        async move {
            assert!(
                self.store.image.borrow().workflow().unwrap().phase()
                    == RecoveryPhase::ExchangeMayStart
            );
            self.times.borrow_mut().push(now);
            self.exchanges.set(self.exchanges.get() + 1);
            let replacement = self.replacement.borrow_mut().take();
            if self.renew_error.get() {
                return Err(error("fixture.exchange_lost"));
            }
            Ok(replacement.map(Zeroizing::new))
        }
        .boxed_local()
    }
    fn validate_credential(&self, _account: &AccountRecord, value: &str) -> tembra::Result<()> {
        if matches!(value, "invalid" | "foreign-registration") {
            return Err(error("providers.credential_invalid"));
        }
        Ok(())
    }
    fn association_key(&self, account: &AccountRecord) -> tembra::Result<Option<String>> {
        Ok(account
            .private_configuration()
            .get("association")
            .and_then(|v| v.as_str())
            .map(str::to_owned))
    }
    fn credential_requirement(
        &self,
        _account: &AccountRecord,
    ) -> tembra::Result<CredentialRequirement> {
        Ok(self.mode.get())
    }
}

fn record(id: &str) -> AccountRecord {
    AccountRecord::new(
        Account {
            id: id.into(),
            provider_id: "fixture".into(),
            provider_label: "Fixture".into(),
            label: "Fixture account".into(),
            models: vec![
                ModelInfo {
                    id: "main".into(),
                    label: "Main".into(),
                    efforts: vec!["high".into()],
                },
                ModelInfo {
                    id: "summary".into(),
                    label: "Summary".into(),
                    efforts: vec![],
                },
            ],
            available: true,
            defaults: ModelDefaults {
                model: Some("main".into()),
                effort: Some("high".into()),
                auxiliary_model: Some("summary".into()),
            },
            authorization_method: Some("fixture".into()),
            models_truncated: false,
        },
        serde_json::from_value(
            json!({"association":"verified-scope-and-subject","unrecognized":{"keep":true}}),
        )
        .unwrap(),
    )
    .unwrap()
}

#[derive(Default)]
struct Trace {
    snapshots: RefCell<Vec<Arc<Snapshot>>>,
    selections: RefCell<Vec<ModelSelection>>,
    credentials: RefCell<Vec<Option<String>>>,
}

struct Factory {
    trace: Rc<Trace>,
    mode: CredentialRequirement,
    limits: bool,
}
struct Construction {
    trace: Rc<Trace>,
}
struct Unused;
impl Model for Unused {
    fn complete<'a>(
        &'a self,
        _context: &'a Context,
        _operation: OperationId,
        _request: &'a ModelRequest,
    ) -> LocalBoxFuture<'a, tembra::Result<ModelResponse>> {
        async { Err(error("fixture.no_dispatch")) }.boxed_local()
    }
}
impl ModelConstruction for Construction {
    fn construct(
        self: Box<Self>,
        snapshot: Arc<Snapshot>,
        credential: Option<Zeroizing<String>>,
    ) -> tembra::Result<Rc<dyn Model>> {
        self.trace.snapshots.borrow_mut().push(snapshot);
        self.trace
            .credentials
            .borrow_mut()
            .push(credential.map(|v| v.to_string()));
        Ok(Rc::new(Unused))
    }
}
impl ModelFactory for Factory {
    fn capture(
        &self,
        _account: &AccountRecord,
        selection: &ModelSelection,
    ) -> tembra::Result<CapturedModel> {
        self.trace.selections.borrow_mut().push(selection.clone());
        let identity = ModelIdentity {
            provider: "fixture".into(),
            adapter: "fixture-profile".into(),
            endpoint: "https://provider.example/".into(),
            deployment: Some(selection.account_id.clone()),
            model: selection.model_id.clone(),
            revision: None,
        };
        let claim = |tokens| LimitClaim {
            tokens: NonZeroU64::new(tokens).unwrap(),
            precision: Precision::Reported,
            evidence: Evidence {
                source: "fixture-source".into(),
                revision: "one".into(),
                layer: Layer::Provider,
                observed_at_ms: 0,
                expires_at_ms: None,
            },
        };
        let records = if self.limits {
            vec![SourceRecord {
                identity: identity.clone(),
                limits: Limits {
                    context: vec![claim(1000)],
                    input: vec![],
                    output: vec![claim(100)],
                },
                tools: Capability::default(),
                streaming: Capability::default(),
                native_output_maximum: Capability::default(),
                native_compaction: Capability::default(),
            }]
        } else {
            vec![]
        };
        CapturedModel::new(
            ModelMetadataInput {
                preparation: Preparation {
                    identity,
                    configuration_revision: format!("{:?}", selection.reasoning),
                    requirements: Requirements::default(),
                    allow_stale_for_ms: 0,
                },
                embedded: None,
                records,
            },
            self.mode,
            Box::new(Construction {
                trace: self.trace.clone(),
            }),
        )
    }
}

struct Fixture {
    accounts: Rc<Accounts>,
    store: Rc<Store>,
    secrets: Rc<Secrets>,
    driver: Rc<Driver>,
    trace: Rc<Trace>,
}

impl Fixture {
    fn recreate(&self) -> Rc<Accounts> {
        Rc::new(
            Accounts::new(
                self.store.owner.clone(),
                self.store.clone(),
                self.secrets.clone(),
                self.driver.clone(),
                Rc::new(|| 42),
                64,
                RecoveryLimits::default(),
            )
            .unwrap(),
        )
    }
    fn factory(&self, limits: bool) -> Factory {
        Factory {
            trace: self.trace.clone(),
            mode: self.driver.mode.get(),
            limits,
        }
    }
    fn phase(&self) -> Option<RecoveryPhase> {
        self.store
            .image
            .borrow()
            .workflow()
            .map(RecoveryRecord::phase)
    }
    fn fail(&self, phase: Option<RecoveryPhase>, outcome: Write) {
        *self.store.fault.borrow_mut() = Some((phase, outcome));
    }
}

fn fixture(mode: CredentialRequirement) -> Fixture {
    let owner = ProviderOwner::new("alice".into(), Uuid::from_u128(1)).unwrap();
    let secrets = Rc::new(Secrets::default());
    let mut refs = BTreeMap::new();
    if mode == CredentialRequirement::Account {
        let reference = CredentialReference::new(
            owner.clone(),
            CredentialSlot::Account {
                account_id: "account".into(),
            },
            Uuid::from_u128(2),
        )
        .unwrap();
        secrets
            .values
            .borrow_mut()
            .insert(key(&reference), "original".into());
        secrets.owned.borrow_mut().insert(key(&reference));
        refs.insert("account".into(), reference);
    }
    let store = Rc::new(Store {
        image: RefCell::new(
            ProviderState::new(
                owner.clone(),
                Uuid::from_u128(3),
                vec![record("account")],
                refs,
                None,
                RecoveryLimits::default(),
            )
            .unwrap(),
        ),
        owner,
        fault: RefCell::new(None),
        pause: Cell::new(None),
        cancel_at: RefCell::new(None),
        unavailable: Cell::new(false),
        classification: Cell::new(0),
        saves: Cell::new(0),
        loads: Cell::new(0),
        initialized: Cell::new(0),
    });
    let driver = Rc::new(Driver {
        store: store.clone(),
        mode: Cell::new(mode),
        replacement: RefCell::new(None),
        binding: Cell::new(1),
        renew_error: Cell::new(false),
        discovery_error: Cell::new(false),
        configures: Cell::new(0),
        exchanges: Cell::new(0),
        accepts: Cell::new(0),
        begins: Cell::new(0),
        times: RefCell::new(vec![]),
        polls: Rc::new(Cell::new(0)),
        closes: Rc::new(Cell::new(0)),
    });
    let accounts = Rc::new(
        Accounts::new(
            store.owner.clone(),
            store.clone(),
            secrets.clone(),
            driver.clone(),
            Rc::new(|| 42),
            64,
            RecoveryLimits::default(),
        )
        .unwrap(),
    );
    Fixture {
        accounts,
        store,
        secrets,
        driver,
        trace: Rc::new(Trace::default()),
    }
}

fn select(choice: ModelChoice) -> SelectModel {
    SelectModel {
        account_id: "account".into(),
        model: choice,
        reasoning: None,
    }
}
fn configure() -> ConfigureProvider {
    ConfigureProvider {
        provider_id: "fixture".into(),
        method: "fixture".into(),
        credential: None,
        url: None,
    }
}
fn run<F>(body: F)
where
    F: FnOnce(Context) -> LocalBoxFuture<'static, tembra::Result<()>> + 'static,
{
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    pool.run_until(host.root("alice", body)).unwrap();
}

struct Flow {
    polls: Rc<Cell<usize>>,
    closes: Rc<Cell<usize>>,
}
impl AuthorizationFlow for Flow {
    fn instructions(&self) -> swarmie_providers::accounts::AuthorizationInstructions {
        swarmie_providers::accounts::AuthorizationInstructions {
            authorization_url: "https://provider.example/authorize".into(),
            user_code: None,
            callback_uri: None,
        }
    }
    fn poll(&self) -> LocalBoxFuture<'_, tembra::Result<Option<Zeroizing<String>>>> {
        async move {
            self.polls.set(self.polls.get() + 1);
            Ok(Some(Zeroizing::new("issued".into())))
        }
        .boxed_local()
    }
    fn submit_redirect<'a>(
        &'a self,
        _callback: &'a str,
    ) -> LocalBoxFuture<'a, tembra::Result<Zeroizing<String>>> {
        async { Ok(Zeroizing::new("issued".into())) }.boxed_local()
    }
    fn cancel(&self) {}
    fn close(&self) -> LocalBoxFuture<'_, ()> {
        async move {
            self.closes.set(self.closes.get() + 1);
        }
        .boxed_local()
    }
}

#[test]
fn metadata_precedes_inference_custody_and_principal_precedes_storage() {
    let f = fixture(CredentialRequirement::Account);
    run(move |context| {
        async move {
            assert_eq!(
                f.accounts
                    .select(&context, select(ModelChoice::Default), &f.factory(false))
                    .await
                    .err()
                    .unwrap()
                    .reason,
                "providers.model_metadata"
            );
            assert_eq!(f.secrets.reads.get(), 0);
            let loads = f.store.loads.get();
            assert_eq!(
                f.accounts
                    .configure("foreign".into(), configure())
                    .await
                    .err()
                    .unwrap()
                    .reason,
                "providers.principal"
            );
            assert_eq!(f.store.loads.get(), loads);
            Ok(())
        }
        .boxed_local()
    });
}

#[test]
fn defaults_auxiliary_reasoning_and_one_cache_do_not_probe_credential_free_accounts() {
    let f = fixture(CredentialRequirement::None);
    run(move |context| {
        async move {
            let factory = f.factory(true);
            for _ in 0..1000 {
                f.accounts
                    .select(&context, select(ModelChoice::Default), &factory)
                    .await?;
            }
            assert!(
                f.trace
                    .snapshots
                    .borrow()
                    .iter()
                    .all(|v| Arc::ptr_eq(v, &f.trace.snapshots.borrow()[0]))
            );
            assert_eq!(
                f.trace.selections.borrow()[0].reasoning.as_deref(),
                Some("high")
            );
            f.accounts
                .select(&context, select(ModelChoice::Auxiliary), &factory)
                .await?;
            assert_eq!(
                f.trace.selections.borrow().last().unwrap().model_id,
                "summary"
            );
            let mut clear = select(ModelChoice::Default);
            clear.reasoning = Some(String::new());
            f.accounts.select(&context, clear, &factory).await?;
            assert_ne!(
                f.trace.snapshots.borrow()[0].generation(),
                f.trace.snapshots.borrow().last().unwrap().generation()
            );
            f.accounts
                .set_defaults(
                    "alice".into(),
                    swarmie_providers::settings::SetModelDefaults {
                        account_id: "account".into(),
                        defaults: ModelDefaults {
                            model: Some("summary".into()),
                            effort: None,
                            auxiliary_model: None,
                        },
                    },
                )
                .await?;
            assert!(f.phase().is_none());
            assert_eq!(f.secrets.reads.get(), 0);
            assert_eq!(f.secrets.stages.get(), 0);
            Ok(())
        }
        .boxed_local()
    });
}

#[test]
fn configure_publishes_one_account_reference_image_and_preserves_private_extensions() {
    let f = fixture(CredentialRequirement::Account);
    futures::executor::block_on(async {
        let original = f
            .store
            .image
            .borrow()
            .credentials()
            .get("account")
            .cloned()
            .unwrap();
        let result = f
            .accounts
            .configure("alice".into(), configure())
            .await
            .unwrap();
        assert_eq!(result.id, "account");
        assert_eq!(result.defaults.model.as_deref(), Some("main"));
        let state = f.store.image.borrow();
        assert!(state.workflow().is_none());
        assert!(state.credentials().get("account") != Some(&original));
        assert_eq!(
            state.accounts()[0]
                .private_configuration()
                .get("unrecognized"),
            Some(&json!({"keep":true}))
        );
        assert!(!f.secrets.values.borrow().contains_key(&key(&original)));
        assert_eq!(f.secrets.stages.get(), 1);
        assert_eq!(f.secrets.retires.get(), 1);
    });
}

#[test]
fn uncertain_intent_never_dispatches_configuration_in_that_invocation() {
    for outcome in [
        Write::NotApplied,
        Write::UnknownApplied,
        Write::UnknownNotApplied,
    ] {
        let f = fixture(CredentialRequirement::Account);
        f.fail(Some(RecoveryPhase::Intent), outcome);
        futures::executor::block_on(async {
            assert!(
                f.accounts
                    .configure("alice".into(), configure())
                    .await
                    .is_err()
            );
            assert_eq!(f.driver.configures.get(), 0);
            assert_eq!(f.secrets.stages.get(), 0);
        });
    }
}

#[test]
fn ready_checkpoint_is_recovered_from_durable_state_after_accounts_recreation() {
    let f = fixture(CredentialRequirement::Account);
    f.fail(Some(RecoveryPhase::CandidateReady), Write::UnknownApplied);
    run(move |context| {
        async move {
            assert!(
                f.accounts
                    .configure("alice".into(), configure())
                    .await
                    .is_err()
            );
            assert!(f.phase() == Some(RecoveryPhase::CandidateReady));
            let recreated = f.recreate();
            let account = recreated.reconcile(&context).await?.unwrap();
            assert_eq!(account.id, "account");
            assert!(f.phase().is_none());
            assert_eq!(f.driver.configures.get(), 1);
            assert_eq!(f.secrets.stages.get(), 1);
            Ok(())
        }
        .boxed_local()
    });
}

#[test]
fn complete_candidate_without_confirmed_ready_checkpoint_is_not_adopted_after_restart() {
    let f = fixture(CredentialRequirement::Account);
    f.fail(
        Some(RecoveryPhase::CandidateReady),
        Write::UnknownNotApplied,
    );
    run(move |context| {
        async move {
            assert!(
                f.accounts
                    .configure("alice".into(), configure())
                    .await
                    .is_err()
            );
            assert!(f.phase() == Some(RecoveryPhase::CandidateStaging));
            let candidate = f
                .store
                .image
                .borrow()
                .workflow()
                .unwrap()
                .references()
                .candidate()
                .cloned()
                .unwrap();
            assert!(matches!(
                f.secrets.read(candidate).await?,
                CredentialRead::Complete(_)
            ));
            let saves = f.store.saves.get();
            assert!(f.recreate().reconcile(&context).await.is_err());
            assert_eq!(f.store.saves.get(), saves);
            assert_eq!(f.secrets.retires.get(), 0);
            Ok(())
        }
        .boxed_local()
    });
}

#[test]
fn unequal_sealed_candidate_collision_is_preserved_and_not_published_or_retired_after_restart() {
    let f = fixture(CredentialRequirement::Account);
    f.secrets.collide.set(true);
    run(move |context| {
        async move {
            assert!(
                f.accounts
                    .configure("alice".into(), configure())
                    .await
                    .is_err()
            );
            assert!(f.phase() == Some(RecoveryPhase::CandidateStaging));
            let before = f
                .store
                .image
                .borrow()
                .credentials()
                .get("account")
                .cloned()
                .unwrap();
            let saves = f.store.saves.get();
            assert!(f.recreate().reconcile(&context).await.is_err());
            assert_eq!(f.store.saves.get(), saves);
            assert_eq!(f.secrets.retires.get(), 0);
            assert_eq!(
                f.secrets.values.borrow().get(&key(&before)).unwrap(),
                "original"
            );
            assert!(
                f.secrets
                    .values
                    .borrow()
                    .values()
                    .any(|value| value == "collision")
            );
            Ok(())
        }
        .boxed_local()
    });
}

#[test]
fn partial_candidate_or_unknown_stage_remains_active_without_restage_or_erasure() {
    for partial in [false, true] {
        let f = fixture(CredentialRequirement::Account);
        if partial {
            f.secrets.partial.set(true);
        } else {
            f.secrets.stage_fault.set(Some(Write::UnknownApplied));
        }
        run(move |context| {
            async move {
                assert!(
                    f.accounts
                        .configure("alice".into(), configure())
                        .await
                        .is_err()
                );
                assert!(f.recreate().reconcile(&context).await.is_err());
                assert_eq!(f.secrets.stages.get(), 1);
                assert_eq!(f.secrets.retires.get(), 0);
                assert!(
                    f.accounts
                        .select(&context, select(ModelChoice::Default), &f.factory(true))
                        .await
                        .is_err()
                );
                Ok(())
            }
            .boxed_local()
        });
    }
}

#[test]
fn applied_unknown_publication_recovers_exact_target_and_cleanup_without_replay() {
    let f = fixture(CredentialRequirement::Account);
    f.fail(Some(RecoveryPhase::Committed), Write::UnknownApplied);
    run(move |context| {
        async move {
            assert!(
                f.accounts
                    .configure("alice".into(), configure())
                    .await
                    .is_err()
            );
            assert!(f.phase() == Some(RecoveryPhase::Committed));
            f.recreate().reconcile(&context).await?;
            assert!(f.phase().is_none());
            assert_eq!(f.driver.configures.get(), 1);
            assert_eq!(f.secrets.stages.get(), 1);
            Ok(())
        }
        .boxed_local()
    });
}

#[test]
fn old_revision_after_unknown_publication_is_not_permission_to_repeat_the_save() {
    let f = fixture(CredentialRequirement::Account);
    f.fail(Some(RecoveryPhase::Committed), Write::UnknownNotApplied);
    run(move |context| {
        async move {
            assert!(
                f.accounts
                    .configure("alice".into(), configure())
                    .await
                    .is_err()
            );
            assert!(f.phase() == Some(RecoveryPhase::AccountPublication));
            let saves = f.store.saves.get();
            assert!(f.recreate().reconcile(&context).await.is_err());
            assert_eq!(f.store.saves.get(), saves);
            assert_eq!(f.secrets.retires.get(), 0);
            Ok(())
        }
        .boxed_local()
    });
}

#[test]
fn unknown_retirement_is_recognized_through_qualified_absence_without_delete_replay() {
    let f = fixture(CredentialRequirement::Account);
    f.secrets.retire_fault.set(Some(Write::UnknownApplied));
    run(move |context| {
        async move {
            assert!(
                f.accounts
                    .configure("alice".into(), configure())
                    .await
                    .is_err()
            );
            assert!(f.phase() == Some(RecoveryPhase::Retirement));
            f.recreate().reconcile(&context).await?;
            assert!(f.phase().is_none());
            assert_eq!(f.secrets.retires.get(), 1);
            Ok(())
        }
        .boxed_local()
    });
}

#[test]
fn protocol_drift_and_unqualified_observation_do_not_clear_active_workflow() {
    let f = fixture(CredentialRequirement::Account);
    f.fail(Some(RecoveryPhase::CandidateReady), Write::UnknownApplied);
    run(move |context| {
        async move {
            assert!(
                f.accounts
                    .configure("alice".into(), configure())
                    .await
                    .is_err()
            );
            f.driver.binding.set(2);
            assert!(f.recreate().reconcile(&context).await.is_err());
            f.driver.binding.set(1);
            f.store.unavailable.set(true);
            assert!(f.recreate().reconcile(&context).await.is_err());
            assert!(f.phase() == Some(RecoveryPhase::CandidateReady));
            assert_eq!(f.secrets.retires.get(), 0);
            Ok(())
        }
        .boxed_local()
    });
}

#[test]
fn renewal_uses_the_same_time_and_capture_and_never_replays_a_lost_exchange() {
    let f = fixture(CredentialRequirement::Account);
    *f.driver.replacement.borrow_mut() = Some("renewed".into());
    f.driver.renew_error.set(true);
    run(move |context| {
        async move {
            assert!(
                f.accounts
                    .select(&context, select(ModelChoice::Default), &f.factory(true))
                    .await
                    .is_err()
            );
            assert_eq!(*f.driver.times.borrow(), vec![42, 42]);
            assert!(f.phase() == Some(RecoveryPhase::ExchangeMayStart));
            assert!(f.recreate().reconcile(&context).await.is_err());
            assert!(
                f.recreate()
                    .select(&context, select(ModelChoice::Default), &f.factory(true))
                    .await
                    .is_err()
            );
            assert_eq!(f.driver.exchanges.get(), 1);
            assert_eq!(f.secrets.stages.get(), 0);
            assert!(
                f.secrets
                    .values
                    .borrow()
                    .values()
                    .any(|value| value == "original")
            );
            Ok(())
        }
        .boxed_local()
    });
}

#[test]
fn renewal_ready_checkpoint_recovery_does_not_use_a_new_clock_or_old_token() {
    for applied in [true, false] {
        let f = fixture(CredentialRequirement::Account);
        *f.driver.replacement.borrow_mut() = Some("renewed".into());
        f.fail(
            Some(RecoveryPhase::CandidateReady),
            if applied {
                Write::UnknownApplied
            } else {
                Write::UnknownNotApplied
            },
        );
        run(move |context| {
            async move {
                assert!(
                    f.accounts
                        .select(&context, select(ModelChoice::Default), &f.factory(true))
                        .await
                        .is_err()
                );
                let result = f.recreate().reconcile(&context).await;
                assert_eq!(result.is_ok(), applied);
                assert_eq!(f.driver.exchanges.get(), 1);
                assert_eq!(*f.driver.times.borrow(), vec![42, 42]);
                if applied {
                    assert!(
                        f.secrets
                            .values
                            .borrow()
                            .values()
                            .any(|value| value == "renewed")
                    );
                }
                Ok(())
            }
            .boxed_local()
        });
    }
}

#[test]
fn unknown_ready_checkpoint_after_observer_loss_survives_accounts_recreation() {
    let f = fixture(CredentialRequirement::Account);
    f.store.pause.set(Some(RecoveryPhase::CandidateReady));
    run(move |context| {
        async move {
            assert!(
                f.accounts
                    .configure("alice".into(), configure())
                    .now_or_never()
                    .is_none()
            );
            assert!(f.phase() == Some(RecoveryPhase::CandidateReady));
            f.recreate().reconcile(&context).await?;
            assert!(f.phase().is_none());
            assert_eq!(f.driver.configures.get(), 1);
            Ok(())
        }
        .boxed_local()
    });
}

#[test]
fn absence_legacy_corruption_and_foreign_owner_never_initialize_or_dispatch_protocol() {
    for classification in [1, 2, 4] {
        let f = fixture(CredentialRequirement::Account);
        f.store.classification.set(classification);
        futures::executor::block_on(async {
            assert!(
                f.accounts
                    .configure("alice".into(), configure())
                    .await
                    .is_err()
            );
            assert_eq!(f.store.initialized.get(), 0);
            assert_eq!(f.driver.configures.get(), 0);
        });
    }
    let f = fixture(CredentialRequirement::Account);
    let foreign = ProviderOwner::new("other".into(), Uuid::new_v4()).unwrap();
    *f.store.image.borrow_mut() = ProviderState::new(
        foreign,
        Uuid::new_v4(),
        vec![],
        BTreeMap::new(),
        None,
        RecoveryLimits::default(),
    )
    .unwrap();
    assert!(
        futures::executor::block_on(f.accounts.configure("alice".into(), configure())).is_err()
    );
    assert_eq!(f.driver.configures.get(), 0);
}

#[test]
fn only_positive_fake_provisioning_plan_initializes_the_exact_empty_target() {
    let f = fixture(CredentialRequirement::None);
    f.store.classification.set(3);
    let page = futures::executor::block_on(f.accounts.list("alice".into(), None)).unwrap();
    assert!(page.accounts.is_empty());
    assert_eq!(f.store.initialized.get(), 1);
    assert_eq!(f.store.image.borrow().revision(), Uuid::from_u128(99));
}

#[test]
fn known_unsavable_renewal_reference_is_refused_before_exchange() {
    let f = fixture(CredentialRequirement::Account);
    *f.driver.replacement.borrow_mut() = Some("renewed".into());
    f.secrets.reject_plan.set(true);
    run(move |context| {
        async move {
            assert!(
                f.accounts
                    .select(&context, select(ModelChoice::Default), &f.factory(true))
                    .await
                    .is_err()
            );
            assert_eq!(f.driver.exchanges.get(), 0);
            assert_eq!(f.store.saves.get(), 0);
            Ok(())
        }
        .boxed_local()
    });
}

#[test]
fn unavailable_custody_is_an_error_and_does_not_become_missing_or_ready() {
    let f = fixture(CredentialRequirement::Account);
    f.secrets.unavailable.set(true);
    assert_eq!(
        futures::executor::block_on(f.accounts.list("alice".into(), None))
            .err()
            .unwrap()
            .reason,
        "fixture.custody_unavailable"
    );
    assert!(f.phase().is_none());
    assert_eq!(f.secrets.stages.get(), 0);
}

#[test]
fn waiting_authorization_is_checkpointed_before_begin_and_gates_new_work() {
    let f = fixture(CredentialRequirement::Account);
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    host.install(vec![swarmie_providers::accounts::authorization::orb(
        f.accounts.clone(),
    )])
    .unwrap();
    let reference = Rc::new(
        host.get(
            swarmie_providers::accounts::authorization::SERVICE,
            vec!["fixture".into(), Uuid::new_v4().to_string()],
            "alice",
        )
        .unwrap(),
    );
    pool.run_until(async move {
        reference.ready().await.unwrap();
        assert!(f.phase() == Some(RecoveryPhase::ExchangeMayStart));
        assert_eq!(f.driver.begins.get(), 1);
        assert_eq!(f.driver.polls.get(), 0);
        let accounts = f.accounts.clone();
        let factory = f.factory(true);
        host.root("alice", move |context| {
            async move {
                assert!(
                    accounts
                        .select(&context, select(ModelChoice::Default), &factory)
                        .await
                        .is_err()
                );
                assert!(
                    accounts
                        .configure("alice".into(), configure())
                        .await
                        .is_err()
                );
                Ok(())
            }
            .boxed_local()
        })
        .await
        .unwrap();
        reference.release().unwrap();
        host.close().await.unwrap();
        assert_eq!(f.driver.closes.get(), 1);
    });
}

#[test]
fn issued_ready_survives_discovery_failure_and_restart_without_exchange_replay() {
    let f = fixture(CredentialRequirement::Account);
    f.driver.discovery_error.set(true);
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    host.install(vec![swarmie_providers::accounts::authorization::orb(
        f.accounts.clone(),
    )])
    .unwrap();
    let reference = Rc::new(
        host.get(
            swarmie_providers::accounts::authorization::SERVICE,
            vec!["fixture".into(), Uuid::new_v4().to_string()],
            "alice",
        )
        .unwrap(),
    );
    let next_host = Host::new(pool.spawner());
    pool.run_until(async move {
        reference.ready().await.unwrap();
        let call = reference.clone();
        host.root("alice", move |context| {
            async move {
                call.call::<_, ()>(&context, "poll", ()).await?;
                call.call::<_, ()>(&context, "retry", ()).await?;
                Ok(())
            }
            .boxed_local()
        })
        .await
        .unwrap();
        assert!(f.phase() == Some(RecoveryPhase::IssuedReady));
        assert_eq!(f.driver.polls.get(), 1);
        assert_eq!(f.driver.accepts.get(), 1);
        reference.release().unwrap();
        host.close().await.unwrap();
        let recreated = f.recreate();
        next_host
            .root("alice", move |context| {
                async move {
                    recreated.reconcile(&context).await?;
                    Ok(())
                }
                .boxed_local()
            })
            .await
            .unwrap();
        assert!(f.phase().is_none());
        assert_eq!(f.driver.begins.get(), 1);
        assert_eq!(f.driver.polls.get(), 1);
        assert_eq!(f.driver.accepts.get(), 2);
    });
}

#[test]
fn escrow_collision_or_lost_issued_ready_checkpoint_cannot_be_discovered_after_restart() {
    for collision in [true, false] {
        let f = fixture(CredentialRequirement::Account);
        if collision {
            f.secrets.collide.set(true);
        } else {
            f.fail(Some(RecoveryPhase::IssuedReady), Write::UnknownNotApplied);
        }
        let mut pool = LocalPool::new();
        let host = Host::new(pool.spawner());
        host.install(vec![swarmie_providers::accounts::authorization::orb(
            f.accounts.clone(),
        )])
        .unwrap();
        let reference = Rc::new(
            host.get(
                swarmie_providers::accounts::authorization::SERVICE,
                vec!["fixture".into(), Uuid::new_v4().to_string()],
                "alice",
            )
            .unwrap(),
        );
        pool.run_until(async move {
            reference.ready().await.unwrap();
            let call = reference.clone();
            let recreated = f.recreate();
            host.root("alice", move |context| {
                async move {
                    call.call::<_, ()>(&context, "poll", ()).await?;
                    assert!(recreated.reconcile(&context).await.is_err());
                    Ok(())
                }
                .boxed_local()
            })
            .await
            .unwrap();
            assert!(f.phase() == Some(RecoveryPhase::ExchangeMayStart));
            assert_eq!(f.driver.accepts.get(), 0);
            assert_eq!(f.driver.polls.get(), 1);
            assert_eq!(f.secrets.retires.get(), 0);
            reference.release().unwrap();
            host.close().await.unwrap();
        });
    }
}

#[test]
fn original_session_recognizes_its_unknown_clear_without_replaying_exchange() {
    let f = fixture(CredentialRequirement::Account);
    f.fail(None, Write::UnknownApplied);
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    host.install(vec![swarmie_providers::accounts::authorization::orb(
        f.accounts.clone(),
    )])
    .unwrap();
    let reference = Rc::new(
        host.get(
            swarmie_providers::accounts::authorization::SERVICE,
            vec!["fixture".into(), Uuid::new_v4().to_string()],
            "alice",
        )
        .unwrap(),
    );
    pool.run_until(async move {
        reference.ready().await.unwrap();let call=reference.clone();let accounts=f.accounts.clone();
        host.root("alice",move|context|async move {
            call.call::<_,()>(&context,"poll",()).await?;
            let failed:Option<swarmie_providers::accounts::authorization::AuthorizationState>=call.state::<Option<swarmie_providers::accounts::authorization::AuthorizationState>>()?.value()?.as_ref().clone();
            assert!(matches!(failed,Some(swarmie_providers::accounts::authorization::AuthorizationState::Failed {retryable:false,..})));
            assert_eq!(accounts.reconcile(&context).await?.unwrap().id,"account");
            call.call::<_,()>(&context,"poll",()).await?;
            let connected:Option<swarmie_providers::accounts::authorization::AuthorizationState>=call.state::<Option<swarmie_providers::accounts::authorization::AuthorizationState>>()?.value()?.as_ref().clone();
            assert!(matches!(connected,Some(swarmie_providers::accounts::authorization::AuthorizationState::Connected { .. })));
            Ok(())
        }.boxed_local()).await.unwrap();
        assert_eq!(f.driver.polls.get(),1);reference.release().unwrap();host.close().await.unwrap();
    });
}

#[test]
fn uncertain_clear_is_not_replayed_from_its_predecessor_after_restart() {
    let f = fixture(CredentialRequirement::Account);
    f.fail(None, Write::UnknownNotApplied);
    run(move |context| {
        async move {
            assert!(
                f.accounts
                    .configure("alice".into(), configure())
                    .await
                    .is_err()
            );
            assert!(f.phase() == Some(RecoveryPhase::Retirement));
            assert!(
                f.store
                    .image
                    .borrow()
                    .workflow()
                    .unwrap()
                    .replacement()
                    .is_some()
            );
            let saves = f.store.saves.get();
            assert!(f.recreate().reconcile(&context).await.is_err());
            assert_eq!(f.store.saves.get(), saves);
            Ok(())
        }
        .boxed_local()
    });
}

#[test]
fn unresolved_cleanup_gates_new_work_but_inspection_and_owned_retirement_can_continue() {
    let f = fixture(CredentialRequirement::Account);
    f.secrets.retire_fault.set(Some(Write::NotApplied));
    run(move |context| {
        async move {
            assert!(
                f.accounts
                    .configure("alice".into(), configure())
                    .await
                    .is_err()
            );
            assert!(f.phase() == Some(RecoveryPhase::Retirement));
            assert!(
                f.accounts
                    .configure("alice".into(), configure())
                    .await
                    .is_err()
            );
            assert!(
                f.accounts
                    .select(&context, select(ModelChoice::Default), &f.factory(true))
                    .await
                    .is_err()
            );
            let catalog = f.accounts.list("alice".into(), None).await?;
            assert!(catalog.accounts.iter().all(|account| !account.available));
            f.recreate().reconcile(&context).await?;
            assert!(f.phase().is_none());
            assert_eq!(f.driver.configures.get(), 1);
            assert_eq!(f.secrets.stages.get(), 1);
            Ok(())
        }
        .boxed_local()
    });
}

#[test]
fn lost_renewal_decision_time_in_intent_cannot_be_replaced_with_a_fresh_clock() {
    let f = fixture(CredentialRequirement::Account);
    *f.driver.replacement.borrow_mut() = Some("renewed".into());
    f.store.pause.set(Some(RecoveryPhase::Intent));
    run(move |context| {
        async move {
            assert!(
                f.accounts
                    .select(&context, select(ModelChoice::Default), &f.factory(true))
                    .now_or_never()
                    .is_none()
            );
            assert!(f.phase() == Some(RecoveryPhase::Intent));
            assert!(f.recreate().reconcile(&context).await.is_err());
            assert_eq!(*f.driver.times.borrow(), vec![42]);
            assert_eq!(f.driver.exchanges.get(), 0);
            Ok(())
        }
        .boxed_local()
    });
}

#[test]
fn canceled_original_session_does_not_publish_connected_after_unknown_clear_recognition() {
    let f = fixture(CredentialRequirement::Account);
    f.fail(None, Write::UnknownApplied);
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    host.install(vec![swarmie_providers::accounts::authorization::orb(
        f.accounts.clone(),
    )])
    .unwrap();
    let reference = Rc::new(
        host.get(
            swarmie_providers::accounts::authorization::SERVICE,
            vec!["fixture".into(), Uuid::new_v4().to_string()],
            "alice",
        )
        .unwrap(),
    );
    pool.run_until(async move {
        reference.ready().await.unwrap();let call=reference.clone();let accounts=f.accounts.clone();
        host.root("alice",move|context|async move {
            call.call::<_,()>(&context,"poll",()).await?;
            call.call::<_,()>(&context,"cancel",()).await?;
            accounts.reconcile(&context).await?;
            call.call::<_,()>(&context,"poll",()).await?;
            let state=call.state::<Option<swarmie_providers::accounts::authorization::AuthorizationState>>()?;
            assert!(matches!(state.value()?.as_ref().as_ref(),Some(swarmie_providers::accounts::authorization::AuthorizationState::Failed {retryable:false,..})));Ok(())
        }.boxed_local()).await.unwrap();
        reference.release().unwrap();host.close().await.unwrap();
    });
}

#[test]
fn a_second_session_cannot_claim_the_original_pending_publication() {
    let f = fixture(CredentialRequirement::Account);
    f.fail(Some(RecoveryPhase::Committed), Write::UnknownApplied);
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    host.install(vec![swarmie_providers::accounts::authorization::orb(
        f.accounts.clone(),
    )])
    .unwrap();
    let reference = Rc::new(
        host.get(
            swarmie_providers::accounts::authorization::SERVICE,
            vec!["fixture".into(), Uuid::new_v4().to_string()],
            "alice",
        )
        .unwrap(),
    );
    pool.run_until(async move {
        reference.ready().await.unwrap();let call=reference.clone();
        host.root("alice",move|context|async move { call.call::<_,()>(&context,"poll",()).await }.boxed_local()).await.unwrap();
        let second=host.get(swarmie_providers::accounts::authorization::SERVICE,
            vec!["fixture".into(),Uuid::new_v4().to_string()],"alice").unwrap();
        assert!(second.ready().await.is_err());assert_eq!(f.driver.begins.get(),1);
        let call=reference.clone();let accounts=f.accounts.clone();
        host.root("alice",move|context|async move {
            accounts.reconcile(&context).await?;call.call::<_,()>(&context,"poll",()).await?;
            let state=call.state::<Option<swarmie_providers::accounts::authorization::AuthorizationState>>()?;
            assert!(matches!(state.value()?.as_ref().as_ref(),Some(swarmie_providers::accounts::authorization::AuthorizationState::Connected { .. })));Ok(())
        }.boxed_local()).await.unwrap();
        reference.release().unwrap();second.release().unwrap();host.close().await.unwrap();
    });
}

#[test]
fn returning_account_registration_failure_precedes_begin_and_checkpoint() {
    let f = fixture(CredentialRequirement::Account);
    let reference = f
        .store
        .image
        .borrow()
        .credentials()
        .get("account")
        .cloned()
        .unwrap();
    f.secrets
        .values
        .borrow_mut()
        .insert(key(&reference), "foreign-registration".into());
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    host.install(vec![swarmie_providers::accounts::authorization::orb(
        f.accounts.clone(),
    )])
    .unwrap();
    let reference = host
        .get(
            swarmie_providers::accounts::authorization::SERVICE,
            vec![
                "fixture".into(),
                Uuid::new_v4().to_string(),
                "account".into(),
            ],
            "alice",
        )
        .unwrap();
    pool.run_until(async move {
        assert!(reference.ready().await.is_err());
        assert_eq!(f.driver.begins.get(), 0);
        assert_eq!(f.store.saves.get(), 0);
        reference.release().unwrap();
        host.close().await.unwrap();
    });
}

#[test]
fn cancellation_during_the_exchange_checkpoint_prevents_remote_renewal_dispatch() {
    let f = fixture(CredentialRequirement::Account);
    *f.driver.replacement.borrow_mut() = Some("renewed".into());
    let store = f.store.clone();
    let driver = f.driver.clone();
    let accounts = f.accounts.clone();
    let factory = f.factory(true);
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let result = pool.run_until(host.root("alice", move |context| {
        async move {
            *store.cancel_at.borrow_mut() =
                Some((RecoveryPhase::ExchangeMayStart, context.clone()));
            assert!(
                accounts
                    .select(&context, select(ModelChoice::Default), &factory)
                    .await
                    .is_err()
            );
            Ok(())
        }
        .boxed_local()
    }));
    // The enclosing root can also report its cancellation. Neither observation
    // changes the checkpoint or authorizes dispatch with a canceled scope.
    drop(result);
    assert_eq!(driver.exchanges.get(), 0);
    assert!(f.phase() == Some(RecoveryPhase::ExchangeMayStart));
}
