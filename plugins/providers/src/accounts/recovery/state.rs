use super::{CredentialReference, CredentialSlot, ProviderOwner, RecoveryPhase, RecoveryRecord};
use crate::AccountRecord;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

/// Ordinary-file encoding bounds. These are not provider token limits. Native
/// adapters can impose smaller key, descriptor or credential bounds separately.
#[derive(Clone, Copy)]
pub struct RecoveryLimits {
    reference: usize,
    workflow: usize,
    envelope: usize,
}

impl Default for RecoveryLimits {
    fn default() -> Self {
        Self {
            reference: 4096,
            workflow: 4 * 1024 * 1024,
            envelope: 8 * 1024 * 1024,
        }
    }
}

impl RecoveryLimits {
    pub fn new(reference: usize, workflow: usize, envelope: usize) -> tembra::Result<Self> {
        if reference == 0 || reference > workflow || workflow > envelope {
            return Err(crate::invalid("invalid provider recovery storage bounds"));
        }
        Ok(Self {
            reference,
            workflow,
            envelope,
        })
    }

    pub fn reference_bytes(self) -> usize {
        self.reference
    }

    pub fn workflow_bytes(self) -> usize {
        self.workflow
    }

    pub fn envelope_bytes(self) -> usize {
        self.envelope
    }
}

/// One checked account/reference image. An Active record always remains visible;
/// the value alone cannot establish native readiness or mutation settlement.
#[derive(Clone, Serialize)]
pub struct ProviderState {
    owner: ProviderOwner,
    revision: Uuid,
    accounts: Vec<AccountRecord>,
    credentials: BTreeMap<String, CredentialReference>,
    workflow: Option<Box<RecoveryRecord>>,
}

impl ProviderState {
    pub fn new(
        owner: ProviderOwner,
        revision: Uuid,
        accounts: Vec<AccountRecord>,
        credentials: BTreeMap<String, CredentialReference>,
        workflow: Option<RecoveryRecord>,
        limits: RecoveryLimits,
    ) -> tembra::Result<Self> {
        super::non_nil(revision)?;
        let value = Self {
            owner,
            revision,
            accounts,
            credentials,
            workflow: workflow.map(Box::new),
        };
        value.encode(limits)?;
        Ok(value)
    }

    pub fn decode(
        bytes: &[u8],
        expected_owner: &ProviderOwner,
        limits: RecoveryLimits,
    ) -> tembra::Result<Self> {
        if bytes.len() > limits.envelope {
            return Err(crate::invalid("provider state exceeds its storage bound"));
        }
        let data: StateData = super::decode::decode(bytes)?;
        if &data.owner != expected_owner {
            return Err(crate::invalid("provider state belongs to another owner"));
        }
        Self::new(
            data.owner,
            data.revision,
            data.accounts,
            data.credentials,
            data.workflow,
            limits,
        )
    }

    pub fn encode(&self, limits: RecoveryLimits) -> tembra::Result<Vec<u8>> {
        let mut ids = BTreeSet::new();
        for record in &self.accounts {
            account_identity(record)?;
            if !ids.insert(record.account.id.as_str()) {
                return Err(crate::invalid(
                    "provider state contains duplicate account IDs",
                ));
            }
        }
        for (id, reference) in &self.credentials {
            if !ids.contains(id.as_str())
                || reference.owner() != &self.owner
                || !matches!(reference.slot(), CredentialSlot::Account { account_id } if account_id == id)
            {
                return Err(crate::invalid(
                    "provider state contains an unbound credential reference",
                ));
            }
            reference.check_size(limits.reference)?;
        }
        if let Some(record) = &self.workflow {
            record.validate_for(&self.owner, self.revision, limits)?;
            check_workflow_image(record, &self.accounts, &self.credentials)?;
        }
        super::encoded(self, limits.envelope)
    }

    pub fn owner(&self) -> &ProviderOwner {
        &self.owner
    }

    pub fn revision(&self) -> Uuid {
        self.revision
    }

    pub fn accounts(&self) -> &[AccountRecord] {
        &self.accounts
    }

    pub fn credentials(&self) -> &BTreeMap<String, CredentialReference> {
        &self.credentials
    }

    pub fn workflow(&self) -> Option<&RecoveryRecord> {
        self.workflow.as_deref()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StateData {
    owner: ProviderOwner,
    revision: Uuid,
    accounts: Vec<AccountRecord>,
    credentials: BTreeMap<String, CredentialReference>,
    #[serde(deserialize_with = "Option::deserialize")]
    workflow: Option<RecoveryRecord>,
}

pub(super) fn account_identity(record: &AccountRecord) -> tembra::Result<()> {
    if record.account.id.is_empty() || record.account.provider_id.is_empty() {
        return Err(crate::invalid(
            "provider recovery contains an invalid account identity",
        ));
    }
    Ok(())
}

fn check_workflow_image(
    record: &RecoveryRecord,
    accounts: &[AccountRecord],
    credentials: &BTreeMap<String, CredentialReference>,
) -> tembra::Result<()> {
    use super::{RecoveryAccount, RecoveryKind, ReplacementKind};
    let (before, candidate, id) = match record.account() {
        RecoveryAccount::Captured { before, candidate } => (
            before.as_ref(),
            Some(candidate.as_ref()),
            Some(candidate.account.id.as_str()),
        ),
        RecoveryAccount::Unassociated { returning, .. } => (
            returning.as_ref(),
            None,
            returning.as_ref().map(|record| record.account.id.as_str()),
        ),
    };
    let target = if record.phase() == RecoveryPhase::Committed {
        match record.replacement().map(|plan| plan.kind()) {
            Some(ReplacementKind::Publish) => {
                if candidate.is_none() {
                    return Err(crate::invalid(
                        "account publication has no captured candidate",
                    ));
                }
                Some((candidate, record.references().candidate()))
            }
            Some(ReplacementKind::Restore) => Some((before, record.references().previous())),
            Some(ReplacementKind::Quarantine) => Some((before.or(candidate), None)),
            _ => return Err(crate::invalid("invalid committed provider result")),
        }
    } else if matches!(
        record.phase(),
        RecoveryPhase::Intent
            | RecoveryPhase::ExchangeMayStart
            | RecoveryPhase::IssuedReady
            | RecoveryPhase::CandidateStaging
            | RecoveryPhase::CandidateReady
            | RecoveryPhase::AccountPublication
    ) {
        Some((before, record.references().previous()))
    } else {
        None
    };
    if let (Some(id), Some((expected, reference))) = (id, target) {
        let current = accounts.iter().find(|value| value.account.id == id);
        let encode = |value: Option<&AccountRecord>| {
            serde_json::to_value(value)
                .map_err(|_| crate::invalid("recovery account image could not be encoded"))
        };
        if encode(current)? != encode(expected)?
            || (record.kind() != RecoveryKind::Defaults && credentials.get(id) != reference)
        {
            return Err(crate::invalid(
                "the provider state does not match its recorded account/reference image",
            ));
        }
    }
    Ok(())
}
