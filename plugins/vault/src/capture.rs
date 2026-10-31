use crate::{
    EncryptedRecord, Error, ExecutionIdentity, Result, SecretMetadata, SecretReference,
    VaultIdentity,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureState {
    AwaitingApproval,
    Admitted,
    Dispatched,
    CandidateHeld,
    Submitted,
    Saved,
    Cancelled,
    ProducerFailedNotIssued,
    IssuedUnknown,
    CandidateLost,
    PendingStore,
    NeedsDecision,
    Stranded,
    Discarded,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureIntent {
    pub identity: VaultIdentity,
    pub originating_operation: Uuid,
    pub execution: ExecutionIdentity,
    pub provider: String,
    pub producer_generation: Uuid,
    pub destination: String,
    pub profile: String,
    pub secret: Uuid,
    pub expected: Option<Uuid>,
    pub metadata: SecretMetadata,
    pub attempt: Uuid,
    pub settlement_ms: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureRecord {
    pub id: Uuid,
    pub journal_revision: u64,
    pub intent: CaptureIntent,
    pub state: CaptureState,
    pub mutation: Uuid,
    pub candidate: Option<EncryptedRecord>,
    pub submission: Option<crate::RecordCommand>,
    pub arrival_late: bool,
    pub dispatched: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedSecretReceipt {
    pub capture: Uuid,
    pub state: CaptureState,
    pub reference: Option<SecretReference>,
}

impl CaptureRecord {
    pub fn prepare(intent: CaptureIntent) -> Result<Self> {
        intent.metadata.validate()?;
        if intent.execution.account != Some(intent.identity.account)
            || intent.provider.is_empty()
            || intent.profile.is_empty()
            || intent.destination.len() > 4096
            || !(1..=300_000).contains(&intent.settlement_ms)
        {
            return Err(Error::Invalid);
        }
        Ok(Self {
            id: Uuid::new_v4(),
            journal_revision: 0,
            intent,
            state: CaptureState::AwaitingApproval,
            mutation: Uuid::new_v4(),
            candidate: None,
            submission: None,
            arrival_late: false,
            dispatched: false,
        })
    }

    pub fn receipt(&self) -> CapturedSecretReceipt {
        CapturedSecretReceipt {
            capture: self.id,
            state: self.state,
            reference: (self.state == CaptureState::Saved).then_some(SecretReference {
                account: self.intent.identity.account,
                vault: self.intent.identity.vault,
                secret: self.intent.secret,
            }),
        }
    }

    pub fn transition(&mut self, next: CaptureState) -> Result<()> {
        use CaptureState::*;
        let valid = matches!(
            (self.state, next),
            (AwaitingApproval, Admitted | Cancelled)
                | (Admitted, Dispatched | Cancelled | NeedsDecision)
                | (
                    Dispatched,
                    CandidateHeld | IssuedUnknown | CandidateLost | ProducerFailedNotIssued
                )
                | (
                    CandidateHeld,
                    CandidateHeld | Submitted | NeedsDecision | Stranded | Discarded
                )
                | (Submitted, Saved | PendingStore | NeedsDecision | Stranded)
                | (
                    PendingStore,
                    Submitted | Saved | NeedsDecision | Stranded | Discarded
                )
                | (
                    NeedsDecision,
                    Admitted | Submitted | CandidateHeld | Saved | Stranded | Discarded | Cancelled
                )
                | (Stranded, CandidateHeld | Saved | Discarded)
                | (
                    ProducerFailedNotIssued | CandidateLost | IssuedUnknown,
                    Discarded
                )
                | (Saved, Saved)
                | (IssuedUnknown, CandidateHeld | Saved | CandidateLost)
        );
        if !valid
            || (next == Admitted && self.dispatched)
            || (matches!(
                next,
                CandidateHeld | Submitted | PendingStore | NeedsDecision | Stranded
            ) && self.state != Admitted
                && self.candidate.is_none())
        {
            return Err(Error::Invalid);
        }
        if next == Dispatched {
            self.dispatched = true;
        }
        self.state = next;
        Ok(())
    }

    /// Recovery does not invoke a producer, mint a new capture, or resurrect authorization.
    pub fn recovered(&mut self) -> Result<()> {
        let before = self.state;
        match self.state {
            CaptureState::AwaitingApproval => self.state = CaptureState::Cancelled,
            CaptureState::Admitted => self.state = CaptureState::NeedsDecision,
            CaptureState::Dispatched => self.state = CaptureState::IssuedUnknown,
            CaptureState::CandidateHeld => self.state = CaptureState::NeedsDecision,
            _ => {}
        }
        if self.state != before {
            self.journal_revision = self
                .journal_revision
                .checked_add(1)
                .ok_or(Error::Exhausted)?;
        }
        Ok(())
    }
}
