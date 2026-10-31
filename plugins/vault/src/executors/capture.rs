//! Source-owned jobs outlive a cancelled view. No residual producer output is ever published.
use super::*;
use crate::*;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};
use zeroize::Zeroizing;

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CaptureSource {
    Random {
        bytes: usize,
    },
    BashJson {
        request: BashRequest,
        pointer: String,
    },
    HttpJson {
        request: HttpRequest,
        pointer: String,
    },
}
impl CaptureSource {
    /// Fingerprints only the reference-only request/profile, never a generated value.
    pub fn profile(&self) -> Result<String> {
        self.validate()?;
        let hash = digest(&canonical(self)?);
        Ok(format!(
            "capture/v1:{}",
            hash.iter().map(|b| format!("{b:02x}")).collect::<String>()
        ))
    }
    /// Frozen public producer description, including normalized endpoints and input references.
    pub fn destination(&self) -> Result<String> {
        Ok(match self {
            Self::Random { .. } => "native:random".into(),
            Self::BashJson { request, .. } => {
                let inputs = request
                    .environment
                    .iter()
                    .map(|(name, reference)| format!("{name}={reference}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "bash:{}:{}\nInput references: {inputs}",
                    request.cwd.display(),
                    request.source
                )
            }
            Self::HttpJson { request, .. } => {
                let (url, method) = http::validate(request)?;
                let mut public = request.clone();
                public.url = url.to_string();
                format!(
                    "{method} {url}\n{}",
                    String::from_utf8(canonical(&public)?).map_err(|_| Error::Invalid)?
                )
            }
        })
    }
    pub fn timeout_ms(&self) -> u64 {
        match self {
            Self::Random { .. } => 1000,
            Self::BashJson { request, .. } => request.timeout_ms,
            Self::HttpJson { request, .. } => request.timeout_ms,
        }
    }
    fn validate(&self) -> Result<()> {
        match self {
            Self::Random { bytes } if (16..=4096).contains(bytes) => Ok(()),
            Self::Random { .. } => Err(Error::Invalid),
            Self::BashJson { request, pointer } => {
                selector(pointer)?;
                bash::validate(request)
            }
            Self::HttpJson { request, pointer } => {
                selector(pointer)?;
                http::validate(request).map(|_| ())
            }
        }
    }
}
fn selector(pointer: &str) -> Result<()> {
    if pointer.len() > 1024 || (!pointer.is_empty() && !pointer.starts_with('/')) {
        return Err(Error::Invalid);
    }
    Ok(())
}
fn extract(bytes: &[u8], pointer: &str) -> Result<SecretValue> {
    let mut json = http::PrivateJson(Some(
        serde_json::from_slice(bytes).map_err(|_| Error::Unknown)?,
    ));
    let value = json
        .0
        .as_mut()
        .and_then(|v| v.pointer_mut(pointer))
        .ok_or(Error::Unknown)?;
    let serde_json::Value::String(value) = value else {
        return Err(Error::Unknown);
    };
    SecretValue::new(std::mem::take(value).into_bytes())
}

/// The native owner retains this handle until persistence succeeds or the user discards it.
/// Dropping an observer does not cancel the started source. A process exit uses journal recovery.
#[derive(Clone)]
pub struct CaptureJob(Rc<Job>);
struct Job {
    identity: VaultIdentity,
    capture: uuid::Uuid,
    failure: RefCell<Option<CaptureDeliveryFailure>>,
    settled: Cell<bool>,
    physical_finished: Cell<bool>,
    done: tokio::sync::Notify,
}
impl CaptureJob {
    pub fn target(&self) -> (VaultIdentity, uuid::Uuid) {
        (self.0.identity, self.0.capture)
    }
    /// Wait for physical settlement and durable candidate delivery; this never repeats a source.
    pub async fn finished(&self) -> Result<()> {
        while !self.0.settled.get() {
            self.0.done.notified().await;
        }
        if !self.0.physical_finished.get() {
            return Err(Error::Unknown);
        }
        if self.0.failure.borrow().is_some() {
            return Err(Error::Unavailable);
        }
        Ok(())
    }

    pub fn is_settled(&self) -> bool {
        self.0.settled.get() && self.0.failure.borrow().is_none()
    }
    pub fn retry_persistence(&self) -> Result<Option<CapturedSecretReceipt>> {
        let failure = self.0.failure.borrow_mut().take();
        match failure {
            None => Ok(None),
            Some(failure) => match failure.retry() {
                Ok(receipt) => Ok(Some(receipt)),
                Err(failure) => {
                    let error = failure.error;
                    *self.0.failure.borrow_mut() = Some(failure);
                    Err(error)
                }
            },
        }
    }
}

/// Frozen native execution identity and adapters. Agent input supplies only reference-based sources.
pub struct CaptureProducers {
    pub node: VaultNode,
    pub session: VaultSession,
    pub execution: ExecutionBinding,
    pub plugin: PluginGrant,
    pub decisions: Rc<dyn UseDecisions>,
    pub bash: Rc<BashExecutor>,
    pub http: Rc<HttpExecutor>,
}
impl CaptureProducers {
    pub async fn start(
        self: Rc<Self>,
        handle: Rc<CaptureHandle>,
        source: CaptureSource,
        decision: HumanDecision,
    ) -> Result<CaptureJob> {
        source.validate()?;
        if !handle.bound_to(&self.node, &self.session) {
            return Err(Error::Forbidden);
        }
        let capture = handle.record()?;
        if capture.intent.profile != source.profile()?
            || capture.intent.destination != source.destination()?
            || capture.intent.settlement_ms != source.timeout_ms()
            || capture.intent.execution != *self.execution.identity()
            || capture.intent.provider != self.plugin.provider()
        {
            return Err(Error::Forbidden);
        }
        let attempt = ExecutionAttempt {
            node: &self.node,
            session: &self.session,
            execution: &self.execution,
            plugin: &self.plugin,
            decisions: self.decisions.as_ref(),
            operation: capture.intent.originating_operation,
            attempt: capture.intent.attempt,
        };
        let (references, destination) = match &source {
            CaptureSource::Random { .. } => (vec![], source.destination()?),
            CaptureSource::BashJson { request, .. } => (
                request.environment.values().copied().collect(),
                source.destination()?,
            ),
            CaptureSource::HttpJson { request, .. } => (
                request
                    .credentials
                    .values()
                    .map(|c| c.reference)
                    .chain(request.fields.iter().map(|f| f.reference))
                    .collect(),
                source.destination()?,
            ),
        };
        let approved = attempt.approvals(&references, &destination).await?;
        for input in &approved {
            if !scope_subset(&capture.intent.metadata.projects, &input.metadata.projects)
                || !scope_subset(&capture.intent.metadata.agents, &input.metadata.agents)
                || approval_strength(capture.intent.metadata.approval)
                    < approval_strength(input.metadata.approval)
            {
                return Err(Error::Forbidden);
            }
        }
        let sink = self
            .node
            .admit_capture(
                &handle,
                decision,
                Duration::from_millis(source.timeout_ms()),
            )
            .await?;
        let job = CaptureJob(Rc::new(Job {
            identity: capture.intent.identity,
            capture: capture.id,
            failure: RefCell::new(None),
            settled: Cell::new(false),
            physical_finished: Cell::new(false),
            done: tokio::sync::Notify::new(),
        }));
        let retained = job.clone();
        // Runtime-owned local task; no JoinHandle abort is tied to the requesting chat/view.
        tokio::task::spawn_local(async move {
            let _completion = Completion(retained.clone());
            let attempt = ExecutionAttempt {
                node: &self.node,
                session: &self.session,
                execution: &self.execution,
                plugin: &self.plugin,
                decisions: self.decisions.as_ref(),
                operation: capture.intent.originating_operation,
                attempt: capture.intent.attempt,
            };
            let mut not_started = false;
            let produced = match source {
                CaptureSource::Random { bytes } => {
                    let mut raw = Zeroizing::new(vec![0; bytes]);
                    rand::rngs::OsRng.fill_bytes(&mut raw);
                    const ALPHABET: &[u8; 16] = b"0123456789abcdef";
                    let mut text = Zeroizing::new(Vec::with_capacity(bytes * 2));
                    for byte in raw.iter() {
                        text.push(ALPHABET[(byte >> 4) as usize]);
                        text.push(ALPHABET[(byte & 15) as usize]);
                    }
                    SecretValue::new(std::mem::take(&mut *text))
                }
                CaptureSource::BashJson { request, pointer } => {
                    match self.bash.run(&attempt, request, true, Some(approved)).await {
                        Ok(bash::ProcessResult::Captured(bytes)) => extract(&bytes, &pointer),
                        Err(error) if error != Error::Unknown => {
                            not_started = true;
                            Err(error)
                        }
                        _ => Err(Error::Unknown),
                    }
                }
                CaptureSource::HttpJson { request, pointer } => {
                    match self.http.run(&attempt, request, true, Some(approved)).await {
                        Ok(http::HttpResult::Captured(bytes)) => extract(&bytes, &pointer),
                        Err(error) if error != Error::Unknown => {
                            not_started = true;
                            Err(error)
                        }
                        _ => Err(Error::Unknown),
                    }
                }
            };
            match produced {
                Ok(value) => {
                    if let Err(failure) = sink.push(value) {
                        *retained.0.failure.borrow_mut() = Some(failure);
                        retained.0.physical_finished.set(true);
                        return;
                    }
                    let _ = self.node.store_capture(&handle).await;
                }
                Err(_) if not_started => {
                    let _ = sink.not_issued();
                }
                Err(_) => {
                    let _ = sink.issuance_unknown();
                }
            }
            retained.0.physical_finished.set(true);
        });
        Ok(job)
    }
}

struct Completion(CaptureJob);
impl Drop for Completion {
    fn drop(&mut self) {
        self.0.0.settled.set(true);
        self.0.0.done.notify_waiters();
    }
}

fn scope_subset(target: &Scope, source: &Scope) -> bool {
    match (target, source) {
        (_, Scope::Unrestricted) => true,
        (Scope::Only(target), Scope::Only(source)) => target.is_subset(source),
        _ => false,
    }
}
fn approval_strength(approval: Approval) -> u8 {
    match approval {
        Approval::None => 0,
        Approval::Conversation => 1,
        Approval::Operation => 2,
    }
}
