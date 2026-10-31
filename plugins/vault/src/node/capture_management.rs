//! Private human recovery authority, distinct from an agent's source/issuance authority.
use super::*;
use crate::{CaptureHandle, CaptureState};

#[derive(Clone, Serialize)]
pub struct CaptureSummary {
    pub id: Uuid,
    pub original_operation: Uuid,
    pub mutation: Uuid,
    pub state: CaptureState,
    pub reference: SecretReference,
    pub metadata: SecretMetadata,
    pub expected: Option<Uuid>,
    pub latest: Option<Uuid>,
    pub has_candidate: bool,
    pub has_submission: bool,
}
impl VaultNode {
    pub fn capture_summaries(
        &self,
        session: &VaultSession,
        plugin: &PluginGrant,
    ) -> Result<Vec<CaptureSummary>> {
        self.check(session)?;
        plugin.check(plugin.provider(), Permission::Manage)?;
        Ok(self
            .0
            .state
            .borrow()
            .snapshot
            .captures
            .iter()
            .map(|record| CaptureSummary {
                id: record.id,
                original_operation: record.intent.originating_operation,
                mutation: record.mutation,
                state: record.state,
                reference: SecretReference {
                    account: self.identity().account,
                    vault: self.identity().vault,
                    secret: record.intent.secret,
                },
                metadata: record.candidate.as_ref().map_or_else(
                    || record.intent.metadata.clone(),
                    |candidate| candidate.header.metadata.clone(),
                ),
                latest: self
                    .latest(SecretReference {
                        account: self.identity().account,
                        vault: self.identity().vault,
                        secret: record.intent.secret,
                    })
                    .ok()
                    .map(|record| record.header.revision),
                expected: record
                    .candidate
                    .as_ref()
                    .map_or(record.intent.expected, |candidate| {
                        candidate.header.previous
                    }),
                has_candidate: record.candidate.is_some(),
                has_submission: record.submission.is_some(),
            })
            .collect())
    }
    pub fn manage_capture(
        &self,
        session: &VaultSession,
        execution: &ExecutionBinding,
        plugin: &PluginGrant,
        capture: Uuid,
        decision: HumanDecision,
    ) -> Result<CaptureHandle> {
        self.check(session)?;
        plugin.check(plugin.provider(), Permission::Manage)?;
        if !execution.is_live() || !execution.shell().same_owner(session.owner()) {
            return Err(Error::Forbidden);
        }
        let record = self.capture_record(capture)?;
        decision.consume_owner(
            session.owner(),
            &(
                "vault-manage-capture/v1",
                self.identity(),
                capture,
                record.intent.originating_operation,
                record.mutation,
            ),
        )?;
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
            id: capture,
            management: true,
            _lifetime: self.capture_lifetime(capture),
        })
    }
    /// Native management projection; no ciphertext or private value is returned.
    pub fn authority_summary(
        &self,
        session: &VaultSession,
        plugin: &PluginGrant,
    ) -> Result<Option<crate::AuthorityCheck>> {
        self.check(session)?;
        plugin.check(plugin.provider(), Permission::Manage)?;
        Ok(self.0.state.borrow().snapshot.authority.clone())
    }
    pub fn current_revision(
        &self,
        session: &VaultSession,
        plugin: &PluginGrant,
        reference: SecretReference,
    ) -> Result<Option<Uuid>> {
        self.check(session)?;
        plugin.check(plugin.provider(), Permission::Manage)?;
        match self.latest(reference) {
            Ok(record) => Ok(Some(record.header.revision)),
            Err(Error::NotFound) => Ok(None),
            Err(error) => Err(error),
        }
    }
    pub fn mutation_fence_digest(
        &self,
        session: &VaultSession,
        plugin: &PluginGrant,
        mutation: Uuid,
    ) -> Result<Vec<u8>> {
        self.check(session)?;
        plugin.check(plugin.provider(), Permission::Manage)?;
        let state = self.0.state.borrow();
        let command = state
            .snapshot
            .pending
            .iter()
            .find(|command| command.operation == mutation)
            .ok_or(Error::NotFound)?;
        Ok(crate::digest(&command.signing_bytes()?))
    }
    /// An exact immutable original submission must be atomically fenced before rebase/discard.
    pub async fn fence_capture(
        &self,
        handle: &CaptureHandle,
        decision: HumanDecision,
    ) -> Result<crate::CommitOutcome> {
        self.refresh_current(&handle.session).await?;
        let record = handle.record()?;
        let command = record.submission.ok_or(Error::NotFound)?;
        let payload = crate::digest(&command.signing_bytes()?);
        decision.consume_owner(
            handle.session.owner(),
            &(
                "vault-fence-capture/v1",
                record.id,
                record.mutation,
                &payload,
            ),
        )?;
        let outcome = self.cloud().fence(command).await?;
        if outcome != crate::CommitOutcome::Unknown {
            self.recognize_capture(handle).await?;
        }
        Ok(outcome)
    }
    pub fn capture_fence_digest(
        &self,
        session: &VaultSession,
        plugin: &PluginGrant,
        capture: Uuid,
    ) -> Result<Vec<u8>> {
        self.check(session)?;
        plugin.check(plugin.provider(), Permission::Manage)?;
        let record = self.capture_record(capture)?;
        Ok(crate::digest(
            &record.submission.ok_or(Error::NotFound)?.signing_bytes()?,
        ))
    }
}
