use super::*;
use crate::{CaptureHandle, CaptureRecord, CaptureState, CapturedSecretReceipt};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureRecoveryIntent {
    pub capture: Uuid,
    pub original_operation: Uuid,
    pub execution: crate::ExecutionIdentity,
}

impl VaultNode {
    /// Safe management projection. Candidate ciphertext and source output are not exposed.
    pub fn capture_receipts(
        &self,
        session: &VaultSession,
        plugin: &PluginGrant,
    ) -> Result<Vec<CapturedSecretReceipt>> {
        self.check(session)?;
        plugin.check(plugin.provider(), Permission::Manage)?;
        Ok(self
            .0
            .state
            .borrow()
            .snapshot
            .captures
            .iter()
            .map(CaptureRecord::receipt)
            .collect())
    }
    /// A native human chooses the retained operation. An agent's capture ID is insufficient.
    /// No producer permission is resurrected: dispatched journals are only recognized or saved.
    pub fn recover_capture(
        &self,
        session: &VaultSession,
        execution: &ExecutionBinding,
        plugin: &PluginGrant,
        intent: CaptureRecoveryIntent,
        decision: HumanDecision,
    ) -> Result<CaptureHandle> {
        self.check(session)?;
        plugin.check(plugin.provider(), Permission::AcceptAndStore)?;
        if !session.owner().same_owner(execution.shell())
            || intent.execution != *execution.identity()
        {
            return Err(Error::Forbidden);
        }
        let record = self.capture_record(intent.capture)?;
        if record.intent.identity != self.identity()
            || record.intent.originating_operation != intent.original_operation
        {
            return Err(Error::Forbidden);
        }
        execution.eligible(
            SecretReference {
                account: self.identity().account,
                vault: self.identity().vault,
                secret: record.intent.secret,
            },
            &record.intent.metadata,
        )?;
        decision.consume_owner(session.owner(), &("vault-recover-capture/v1", &intent))?;
        self.0
            .state
            .borrow_mut()
            .capture_owners
            .insert(record.id, Rc::downgrade(&session.0));
        Ok(CaptureHandle {
            node: self.clone(),
            session: session.clone(),
            execution: execution.clone(),
            plugin: plugin.clone(),
            id: record.id,
            management: false,
            _lifetime: self.capture_lifetime(record.id),
        })
    }
    /// Recognize the original immutable submission before allowing a renamed/rebased candidate.
    pub async fn recognize_capture(&self, handle: &CaptureHandle) -> Result<crate::CommitOutcome> {
        self.refresh_authority(&handle.session).await?;
        let mut record = handle.record()?;
        let Some(command) = &record.submission else {
            return Ok(crate::CommitOutcome::NotDispatched);
        };
        let outcome = self.cloud().recognize(command.clone()).await?;
        if outcome == crate::CommitOutcome::Accepted {
            self.synchronize(&handle.session).await?;
            record.transition(CaptureState::Saved)?;
            self.save_capture(&mut record)?;
        } else if outcome != crate::CommitOutcome::Unknown
            && record.candidate.is_some()
            && matches!(
                record.state,
                CaptureState::Submitted | CaptureState::PendingStore
            )
        {
            record.transition(CaptureState::NeedsDecision)?;
            self.save_capture(&mut record)?;
        }
        Ok(outcome)
    }
    /// Re-encrypt the same held value under a new record revision after explicit metadata approval.
    /// External issuance never runs here. An unknown original mutation blocks this operation.
    pub async fn rebase_capture(
        &self,
        handle: &CaptureHandle,
        metadata: SecretMetadata,
        expected: Option<Uuid>,
        decision: HumanDecision,
    ) -> Result<CapturedSecretReceipt> {
        let outcome = self.recognize_capture(handle).await?;
        if outcome == crate::CommitOutcome::Accepted {
            return Ok(handle.record()?.receipt());
        }
        if outcome == crate::CommitOutcome::Unknown {
            return Err(Error::Unknown);
        }
        if self.cloud().mode(self.identity()).await? != VaultMode::Current {
            return Err(Error::VaultNotCurrent);
        }
        let mut record = handle.record()?;
        handle
            .plugin
            .check(handle.plugin.provider(), Permission::AcceptAndStore)?;
        metadata.validate()?;
        let reference = SecretReference {
            account: self.identity().account,
            vault: self.identity().vault,
            secret: record.intent.secret,
        };
        handle.eligible(reference, &metadata)?;
        decision.consume_owner(
            handle.session.owner(),
            &(
                "vault-rebase-capture/v1",
                record.id,
                record.mutation,
                &metadata,
                expected,
            ),
        )?;
        let previous = match self.latest(reference) {
            Ok(record) => Some(record),
            Err(Error::NotFound) => None,
            Err(e) => return Err(e),
        };
        if previous.as_ref().map(|r| r.header.revision) != expected {
            return Err(Error::Stale);
        }
        if previous.as_ref().is_some_and(|r| {
            r.header.metadata != metadata
                && r.header.metadata.policy_generation == metadata.policy_generation
        }) {
            return Err(Error::Invalid);
        }
        let key = self.check(&handle.session)?;
        let value = key.open(record.candidate.as_ref().ok_or(Error::NotFound)?)?;
        let mutation = Uuid::new_v4();
        record.candidate = Some(key.seal(
            RecordHeader {
                version: 1,
                identity: self.identity(),
                secret: record.intent.secret,
                revision: mutation,
                previous: expected,
                successor: next_successor(previous.as_ref())?,
                metadata: metadata.clone(),
                kind: RecordKind::Value,
            },
            Some(&value),
        )?);
        record.mutation = mutation;
        record.submission = None;
        record.transition(CaptureState::CandidateHeld)?;
        // Keep the immutable original source/issuance intent. Only the candidate header changes.
        self.save_capture(&mut record)?;
        Ok(record.receipt())
    }
    /// The confirmation warns that discarding storage does not revoke a credential at its issuer.
    pub async fn discard_capture(
        &self,
        handle: &CaptureHandle,
        decision: HumanDecision,
    ) -> Result<CapturedSecretReceipt> {
        let outcome = self.recognize_capture(handle).await?;
        if outcome == crate::CommitOutcome::Accepted {
            return Ok(handle.record()?.receipt());
        }
        if outcome == crate::CommitOutcome::Unknown {
            return Err(Error::Unknown);
        }
        let mut record = handle.record()?;
        if record.state == CaptureState::Dispatched {
            return Err(Error::Unknown);
        }
        decision.consume_owner(
            handle.session.owner(),
            &(
                "vault-discard-capture/v1",
                record.id,
                record.mutation,
                "issuer-credential-may-remain-live",
            ),
        )?;
        if !record.dispatched && record.candidate.is_none() {
            record.transition(CaptureState::Cancelled)?;
        } else {
            record.transition(CaptureState::Discarded)?;
        }
        record.candidate = None;
        record.submission = None;
        self.save_capture(&mut record)?;
        Ok(record.receipt())
    }
}
