use super::*;
use crate::{
    CaptureIntent, CaptureRecord, CaptureState, CapturedSecretReceipt, crypto::RecordSealer,
};
use std::time::{Duration, Instant};

const MAX_PENDING_CAPTURES: usize = 64;
const MAX_SETTLEMENT: Duration = Duration::from_secs(300);

/// Actual source and observation authority, handed privately to one qualified producer owner.
/// A persisted capture ID alone cannot construct this handle.
pub struct CaptureHandle {
    pub(super) node: VaultNode,
    pub(super) session: VaultSession,
    pub(super) execution: ExecutionBinding,
    pub(super) plugin: PluginGrant,
    pub(super) id: Uuid,
    pub(super) management: bool,
    pub(super) _lifetime: Rc<CaptureLifetime>,
}

impl CaptureHandle {
    pub(super) fn eligible(
        &self,
        reference: SecretReference,
        metadata: &SecretMetadata,
    ) -> Result<()> {
        if self.management {
            if !self.execution.is_live() {
                return Err(Error::Revoked);
            }
            self.plugin
                .check(self.plugin.provider(), Permission::Manage)?;
            if reference.account != self.node.identity().account
                || reference.vault != self.node.identity().vault
            {
                return Err(Error::Forbidden);
            }
            Ok(())
        } else {
            self.execution.eligible(reference, metadata)
        }
    }
    pub fn execution(&self) -> &ExecutionBinding {
        &self.execution
    }

    #[cfg(feature = "executors")]
    pub(crate) fn bound_to(&self, node: &VaultNode, session: &VaultSession) -> bool {
        Rc::ptr_eq(&self.node.0, &node.0) && Rc::ptr_eq(&self.session.0, &session.0)
    }
    pub fn record(&self) -> Result<CaptureRecord> {
        self.node.check(&self.session)?;
        if !self.execution.is_live() || !self.session.owner().same_owner(self.execution.shell()) {
            return Err(Error::Forbidden);
        }
        self.node.capture_record(self.id)
    }
}

pub(super) struct CaptureLifetime {
    node: VaultNode,
    id: Uuid,
}
impl Drop for CaptureLifetime {
    fn drop(&mut self) {
        let _ = self.node.cancel_undispatched(self.id);
    }
}

/// Push-only, one-value, one-target sealer. It has no vault data key or read capability.
/// The producer owner must enforce the physical settlement deadline and drop this sink then.
pub struct SecretCaptureSink {
    node: VaultNode,
    id: Uuid,
    sealer: Option<RecordSealer>,
    deadline: Instant,
}

impl SecretCaptureSink {
    /// A timeout or extraction failure cannot establish whether an external credential exists.
    pub fn issuance_unknown(mut self) -> Result<CapturedSecretReceipt> {
        self.sealer.take();
        let mut record = self.node.capture_record(self.id)?;
        record.transition(CaptureState::IssuedUnknown)?;
        self.node.save_capture(&mut record)?;
        Ok(record.receipt())
    }
    pub fn deadline(&self) -> Instant {
        self.deadline
    }

    /// Publication is permitted only after this returns a durable CandidateHeld receipt.
    /// Logical cancellation/session closure does not discard an already dispatched result.
    pub fn push(
        mut self,
        value: SecretValue,
    ) -> std::result::Result<CapturedSecretReceipt, CaptureDeliveryFailure> {
        let candidate = match self
            .sealer
            .take()
            .ok_or(Error::Revoked)
            .and_then(|s| s.seal(Some(&value)))
        {
            Ok(candidate) => candidate,
            Err(error) => return Err(CaptureDeliveryFailure { error, held: None }),
        };
        HeldCandidate {
            node: self.node.clone(),
            id: self.id,
            candidate,
            late: Instant::now() > self.deadline,
        }
        .persist()
    }

    /// Use only when the producer owner proves no credential was issued, not on a timeout.
    pub fn not_issued(mut self) -> Result<CapturedSecretReceipt> {
        self.sealer.take();
        let mut capture = self.node.capture_record(self.id)?;
        capture.transition(CaptureState::ProducerFailedNotIssued)?;
        self.node.save_capture(&mut capture)?;
        Ok(capture.receipt())
    }
}

impl Drop for SecretCaptureSink {
    fn drop(&mut self) {
        if self.sealer.take().is_some() {
            // A cancelled qualified runner cannot prove that an external credential was not issued.
            if let Ok(mut capture) = self.node.capture_record(self.id)
                && capture.state == CaptureState::Dispatched
            {
                let _ = capture.transition(CaptureState::IssuedUnknown);
                let _ = self.node.save_capture(&mut capture);
            }
        }
    }
}

/// A storage failure retains encrypted output for a producer-owned persistence retry.
/// Debug intentionally includes no candidate, input, ciphertext or secret-bearing diagnostics.
#[must_use]
pub struct CaptureDeliveryFailure {
    pub error: Error,
    held: Option<Box<HeldCandidate>>,
}

impl std::fmt::Debug for CaptureDeliveryFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CaptureDeliveryFailure")
            .field("error", &self.error)
            .field("candidate_retained", &self.held.is_some())
            .finish()
    }
}

impl CaptureDeliveryFailure {
    pub fn retry(self) -> std::result::Result<CapturedSecretReceipt, Self> {
        match self.held {
            Some(held) => (*held).persist(),
            None => Err(self),
        }
    }
}

struct HeldCandidate {
    node: VaultNode,
    id: Uuid,
    candidate: EncryptedRecord,
    late: bool,
}

impl HeldCandidate {
    fn persist(self) -> std::result::Result<CapturedSecretReceipt, CaptureDeliveryFailure> {
        let result = (|| {
            let mut capture = self.node.capture_record(self.id)?;
            if !matches!(
                capture.state,
                CaptureState::Dispatched | CaptureState::IssuedUnknown
            ) {
                return Err(Error::Forbidden);
            }
            capture.candidate = Some(self.candidate.clone());
            capture.arrival_late = self.late;
            capture.transition(CaptureState::CandidateHeld)?;
            if self.late {
                capture.transition(CaptureState::NeedsDecision)?;
            }
            self.node.save_capture(&mut capture)?;
            Ok(capture.receipt())
        })();
        result.map_err(|error| CaptureDeliveryFailure {
            error,
            held: Some(Box::new(self)),
        })
    }
}

impl VaultNode {
    pub(super) fn capture_record(&self, id: Uuid) -> Result<CaptureRecord> {
        self.0
            .state
            .borrow()
            .snapshot
            .captures
            .iter()
            .find(|c| c.id == id)
            .cloned()
            .ok_or(Error::NotFound)
    }

    pub(super) fn capture_lifetime(&self, id: Uuid) -> Rc<CaptureLifetime> {
        if let Some(lifetime) = self
            .0
            .state
            .borrow()
            .capture_lifetimes
            .get(&id)
            .and_then(Weak::upgrade)
        {
            return lifetime;
        }
        let lifetime = Rc::new(CaptureLifetime {
            node: self.clone(),
            id,
        });
        self.0
            .state
            .borrow_mut()
            .capture_lifetimes
            .insert(id, Rc::downgrade(&lifetime));
        lifetime
    }
    fn cancel_undispatched(&self, id: Uuid) -> Result<()> {
        let mut record = self.capture_record(id)?;
        if !record.dispatched
            && record.candidate.is_none()
            && matches!(
                record.state,
                CaptureState::AwaitingApproval
                    | CaptureState::Admitted
                    | CaptureState::NeedsDecision
            )
        {
            record.transition(CaptureState::Cancelled)?;
            self.save_capture(&mut record)?;
        }
        Ok(())
    }
    /// Authority-reducing cancellation never cancels an already dispatched physical producer.
    pub fn cancel_capture(&self, handle: &CaptureHandle) -> Result<()> {
        if !Rc::ptr_eq(&self.0, &handle.node.0) {
            return Err(Error::Forbidden);
        }
        self.cancel_undispatched(handle.id)
    }

    pub(super) fn save_capture(&self, capture: &mut CaptureRecord) -> Result<()> {
        let mut snapshot = self.0.state.borrow().snapshot.clone();
        let next_revision = capture
            .journal_revision
            .checked_add(1)
            .ok_or(Error::Exhausted)?;
        let mut next = capture.clone();
        next.journal_revision = next_revision;
        if let Some(prior) = snapshot
            .captures
            .iter_mut()
            .find(|record| record.id == capture.id)
        {
            if prior.journal_revision != capture.journal_revision {
                return Err(Error::Stale);
            }
            *prior = next;
        } else {
            if capture.journal_revision != 0 {
                return Err(Error::Stale);
            }
            snapshot.captures.push(next);
        }
        self.persist(snapshot)?;
        capture.journal_revision = next_revision;
        Ok(())
    }

    /// Native producer adapters call this after qualifying their profile and destination.
    /// Repeated delivery of the same authenticated durable operation returns the same capture.
    pub async fn prepare_capture(
        &self,
        session: &VaultSession,
        execution: &ExecutionBinding,
        plugin: &PluginGrant,
        intent: CaptureIntent,
    ) -> Result<CaptureHandle> {
        self.refresh_current(session).await?;
        self.check(session)?;
        plugin.check(&intent.provider, Permission::RequestCapture)?;
        if plugin.generation() != intent.producer_generation {
            return Err(Error::Forbidden);
        }
        if !session.owner().same_owner(execution.shell())
            || intent.execution != *execution.identity()
            || intent.identity != self.0.identity
        {
            return Err(Error::Forbidden);
        }
        let reference = SecretReference {
            account: intent.identity.account,
            vault: intent.identity.vault,
            secret: intent.secret,
        };
        self.check_agent_mutation(
            session,
            execution,
            reference,
            intent.expected,
            &intent.metadata,
        )?;
        let existing = self
            .0
            .state
            .borrow()
            .snapshot
            .captures
            .iter()
            .find(|c| c.intent.originating_operation == intent.originating_operation)
            .cloned();
        if let Some(prior) = existing {
            let mut stable = intent.clone();
            stable.attempt = prior.intent.attempt;
            stable.producer_generation = prior.intent.producer_generation;
            if prior.intent != stable {
                return Err(Error::Invalid);
            }
            let same_owner = self
                .0
                .state
                .borrow()
                .capture_owners
                .get(&prior.id)
                .and_then(Weak::upgrade)
                .is_some_and(|s| Rc::ptr_eq(&s, &session.0));
            if !same_owner {
                return Err(Error::Forbidden);
            }
            return Ok(CaptureHandle {
                node: self.clone(),
                session: session.clone(),
                execution: execution.clone(),
                plugin: plugin.clone(),
                id: prior.id,
                management: false,
                _lifetime: self.capture_lifetime(prior.id),
            });
        }
        if self.cloud().mode(self.0.identity).await? != VaultMode::Current {
            return Err(Error::VaultNotCurrent);
        }
        self.check(session)?;
        if self
            .0
            .state
            .borrow()
            .snapshot
            .captures
            .iter()
            .filter(|c| {
                !matches!(
                    c.state,
                    CaptureState::Saved
                        | CaptureState::Discarded
                        | CaptureState::Cancelled
                        | CaptureState::ProducerFailedNotIssued
                )
            })
            .count()
            >= MAX_PENDING_CAPTURES
        {
            return Err(Error::Exhausted);
        }
        let prior = match self.latest(reference) {
            Ok(r) => Some(r),
            Err(Error::NotFound) => None,
            Err(e) => return Err(e),
        };
        if prior.as_ref().map(|r| r.header.revision) != intent.expected {
            return Err(Error::Stale);
        }
        if prior.as_ref().is_some_and(|record| {
            record.header.metadata != intent.metadata
                && record.header.metadata.policy_generation == intent.metadata.policy_generation
        }) {
            return Err(Error::Invalid);
        }

        let mut capture = CaptureRecord::prepare(intent)?;
        let id = capture.id;
        self.save_capture(&mut capture)?;
        self.0
            .state
            .borrow_mut()
            .capture_owners
            .insert(id, Rc::downgrade(&session.0));
        Ok(CaptureHandle {
            node: self.clone(),
            session: session.clone(),
            execution: execution.clone(),
            plugin: plugin.clone(),
            id,
            management: false,
            _lifetime: self.capture_lifetime(id),
        })
    }

    /// Persist Admitted before final authority consumption, then Dispatched before producer I/O.
    /// No API on a returned sink can perform a second producer invocation.
    pub async fn admit_capture(
        &self,
        handle: &CaptureHandle,
        decision: HumanDecision,
        settlement: Duration,
    ) -> Result<SecretCaptureSink> {
        if !Rc::ptr_eq(&self.0, &handle.node.0)
            || settlement.is_zero()
            || settlement > MAX_SETTLEMENT
        {
            return Err(Error::Invalid);
        }
        if handle.management {
            return Err(Error::Forbidden);
        }
        self.refresh_current(&handle.session).await?;
        match self.cloud().mode(self.0.identity).await? {
            VaultMode::Current => {}
            VaultMode::ArchivedReadable => return Err(Error::VaultNotCurrent),
            VaultMode::Exposed => return Err(Error::RootExposed),
        }
        let key = self.check(&handle.session)?;
        let mut capture = handle.record()?;
        handle
            .plugin
            .check(&capture.intent.provider, Permission::RequestCapture)?;
        if handle.plugin.generation() != capture.intent.producer_generation {
            return Err(Error::Forbidden);
        }
        self.check_agent_mutation(
            &handle.session,
            &handle.execution,
            SecretReference {
                account: self.identity().account,
                vault: self.identity().vault,
                secret: capture.intent.secret,
            },
            capture.intent.expected,
            &capture.intent.metadata,
        )?;
        if settlement.as_millis() != u128::from(capture.intent.settlement_ms)
            || !(capture.state == CaptureState::AwaitingApproval
                || (capture.state == CaptureState::NeedsDecision
                    && !capture.dispatched
                    && capture.candidate.is_none()))
        {
            return Err(Error::Forbidden);
        }
        let reference = SecretReference {
            account: self.0.identity.account,
            vault: self.0.identity.vault,
            secret: capture.intent.secret,
        };
        let prior = match self.latest(reference) {
            Ok(r) => Some(r),
            Err(Error::NotFound) => None,
            Err(e) => return Err(e),
        };
        if prior.as_ref().map(|r| r.header.revision) != capture.intent.expected {
            return Err(Error::Stale);
        }
        let sealer = key.sealer(RecordHeader {
            version: 1,
            identity: self.0.identity,
            secret: capture.intent.secret,
            revision: capture.mutation,
            previous: capture.intent.expected,
            successor: next_successor(prior.as_ref())?,
            metadata: capture.intent.metadata.clone(),
            kind: RecordKind::Value,
        })?;
        capture.transition(CaptureState::Admitted)?;
        self.save_capture(&mut capture)?;
        if let Err(error) = decision.consume_owner(
            handle.session.owner(),
            &("vault-blind-save/v1", capture.id, &capture.intent),
        ) {
            capture.transition(CaptureState::NeedsDecision)?;
            self.save_capture(&mut capture)?;
            return Err(error.into());
        }
        self.check(&handle.session)?;
        capture.transition(CaptureState::Dispatched)?;
        self.save_capture(&mut capture)?;
        Ok(SecretCaptureSink {
            node: self.clone(),
            id: capture.id,
            sealer: Some(sealer),
            deadline: Instant::now() + settlement,
        })
    }

    /// Submit the same encrypted candidate; external issuance is never repeated by this method.
    pub async fn store_capture(&self, handle: &CaptureHandle) -> Result<CapturedSecretReceipt> {
        self.check(&handle.session)?;
        let mut capture = handle.record()?;
        handle
            .plugin
            .check(handle.plugin.provider(), Permission::AcceptAndStore)?;
        if capture.state == CaptureState::Saved {
            return Ok(capture.receipt());
        }
        let candidate = capture.candidate.clone().ok_or(Error::NotFound)?;
        handle.eligible(
            SecretReference {
                account: self.identity().account,
                vault: self.identity().vault,
                secret: candidate.header.secret,
            },
            &candidate.header.metadata,
        )?;
        if capture.state == CaptureState::Submitted || capture.state == CaptureState::PendingStore {
            let outcome = self
                .cloud()
                .recognize(capture.submission.clone().ok_or(Error::Integrity)?)
                .await?;
            match outcome {
                crate::CommitOutcome::Accepted => {
                    self.synchronize(&handle.session).await?;
                    capture.transition(CaptureState::Saved)?;
                    self.save_capture(&mut capture)?;
                    return Ok(capture.receipt());
                }
                crate::CommitOutcome::Unknown => return Ok(capture.receipt()),
                crate::CommitOutcome::NotDispatched => {
                    capture.transition(CaptureState::NeedsDecision)?;
                    self.save_capture(&mut capture)?;
                    return Ok(capture.receipt());
                }
                _ => {
                    capture.transition(CaptureState::NeedsDecision)?;
                    self.save_capture(&mut capture)?;
                    return Ok(capture.receipt());
                }
            }
        } else if capture.state != CaptureState::CandidateHeld {
            return Err(Error::ApprovalRequired);
        }
        self.check(&handle.session)?;
        if self.cloud().mode(self.0.identity).await? != VaultMode::Current {
            capture.transition(CaptureState::Stranded)?;
            self.save_capture(&mut capture)?;
            return Ok(capture.receipt());
        }
        let command = if let Some(command) = &capture.submission {
            command.clone()
        } else {
            crate::RecordCommand::prepare(&self.0.keys, self.0.device, candidate.clone())?
        };
        capture.submission = Some(command.clone());
        if capture.state != CaptureState::Submitted {
            capture.transition(CaptureState::Submitted)?;
        }
        self.save_capture(&mut capture)?;
        let outcome = self
            .cloud()
            .commit(command)
            .await
            .unwrap_or(crate::CommitOutcome::Unknown);
        let next = match outcome {
            crate::CommitOutcome::Accepted => {
                self.synchronize(&handle.session).await?;
                CaptureState::Saved
            }
            crate::CommitOutcome::Unknown => CaptureState::PendingStore,
            crate::CommitOutcome::VaultNotCurrent | crate::CommitOutcome::RootExposed => {
                CaptureState::Stranded
            }
            _ => CaptureState::NeedsDecision,
        };
        capture.transition(next)?;
        self.save_capture(&mut capture)?;
        Ok(capture.receipt())
    }
}
