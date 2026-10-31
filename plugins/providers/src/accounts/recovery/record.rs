use super::state::account_identity;
use super::{CredentialReference, CredentialSlot, ProtocolBinding, ProviderOwner, RecoveryLimits};
use crate::AccountRecord;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryKind {
    Configure,
    Authorize,
    Renew,
    Defaults,
}

#[derive(Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryPhase {
    Intent,
    ExchangeMayStart,
    IssuedReady,
    CandidateStaging,
    CandidateReady,
    AccountPublication,
    Restore,
    Quarantine,
    Committed,
    Retirement,
}

#[derive(Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplacementKind {
    Publish,
    Restore,
    Quarantine,
    Clear,
}

/// Planned replacement identity, not evidence that a write was dispatched or
/// settled. Recognition still requires the native owner and matching image.
#[derive(Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ReplacementData")]
pub struct StateReplacement {
    kind: ReplacementKind,
    expected_revision: Uuid,
    target_revision: Uuid,
}

impl StateReplacement {
    pub fn new(
        kind: ReplacementKind,
        expected_revision: Uuid,
        target_revision: Uuid,
    ) -> tembra::Result<Self> {
        super::non_nil(expected_revision)?;
        super::non_nil(target_revision)?;
        if expected_revision == target_revision {
            return Err(crate::invalid(
                "a provider replacement requires a new revision",
            ));
        }
        Ok(Self {
            kind,
            expected_revision,
            target_revision,
        })
    }

    pub fn kind(self) -> ReplacementKind {
        self.kind
    }

    pub fn expected_revision(self) -> Uuid {
        self.expected_revision
    }

    pub fn target_revision(self) -> Uuid {
        self.target_revision
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReplacementData {
    kind: ReplacementKind,
    expected_revision: Uuid,
    target_revision: Uuid,
}

impl TryFrom<ReplacementData> for StateReplacement {
    type Error = &'static str;

    fn try_from(value: ReplacementData) -> Result<Self, Self::Error> {
        Self::new(value.kind, value.expected_revision, value.target_revision)
            .map_err(|_| "invalid provider replacement plan")
    }
}

/// Account images are private persistence inputs. RecoveryRecord checks their
/// identities; the concrete driver still validates protocol/association facts.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RecoveryAccount {
    Unassociated {
        provider: String,
        #[serde(deserialize_with = "Option::deserialize")]
        returning: Option<AccountRecord>,
    },
    Captured {
        #[serde(deserialize_with = "Option::deserialize")]
        before: Option<AccountRecord>,
        candidate: Box<AccountRecord>,
    },
}

impl RecoveryAccount {
    fn validate(&self) -> tembra::Result<()> {
        match self {
            Self::Unassociated {
                provider,
                returning,
            } => {
                if provider.is_empty() {
                    return Err(crate::invalid("a recovery account requires a provider"));
                }
                if let Some(record) = returning {
                    account_identity(record)?;
                    if record.account.provider_id != *provider {
                        return Err(crate::invalid(
                            "the returning recovery account has another provider",
                        ));
                    }
                }
            }
            Self::Captured { before, candidate } => {
                account_identity(candidate)?;
                if let Some(before) = before {
                    account_identity(before)?;
                    if before.account.id != candidate.account.id
                        || before.account.provider_id != candidate.account.provider_id
                    {
                        return Err(crate::invalid(
                            "recovery account images have different identities",
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn previous_id(&self) -> Option<&str> {
        match self {
            Self::Unassociated { returning, .. } => returning.as_ref(),
            Self::Captured { before, .. } => before.as_ref(),
        }
        .map(|record| record.account.id.as_str())
    }

    fn candidate_id(&self) -> Option<&str> {
        match self {
            Self::Captured { candidate, .. } => Some(&candidate.account.id),
            Self::Unassociated { .. } => None,
        }
    }
}

/// At most three immutable references. No encoded credential belongs here.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryReferences {
    #[serde(deserialize_with = "Option::deserialize")]
    previous: Option<CredentialReference>,
    #[serde(deserialize_with = "Option::deserialize")]
    issued: Option<CredentialReference>,
    #[serde(deserialize_with = "Option::deserialize")]
    candidate: Option<CredentialReference>,
}

impl RecoveryReferences {
    pub fn new(
        previous: Option<CredentialReference>,
        issued: Option<CredentialReference>,
        candidate: Option<CredentialReference>,
    ) -> Self {
        Self {
            previous,
            issued,
            candidate,
        }
    }

    pub fn previous(&self) -> Option<&CredentialReference> {
        self.previous.as_ref()
    }

    pub fn issued(&self) -> Option<&CredentialReference> {
        self.issued.as_ref()
    }

    pub fn candidate(&self) -> Option<&CredentialReference> {
        self.candidate.as_ref()
    }

    fn values(&self) -> impl Iterator<Item = &CredentialReference> {
        [
            self.previous.as_ref(),
            self.issued.as_ref(),
            self.candidate.as_ref(),
        ]
        .into_iter()
        .flatten()
    }
}

/// Bounded private workflow data. Its phase is an observation, not permission to
/// refresh, restore, erase, initialize or report a native write as settled.
#[derive(Clone, Serialize, Deserialize)]
#[serde(try_from = "RecordData")]
pub struct RecoveryRecord {
    operation: Uuid,
    base_revision: Uuid,
    kind: RecoveryKind,
    protocol_binding: Option<ProtocolBinding>,
    account: RecoveryAccount,
    references: RecoveryReferences,
    phase: RecoveryPhase,
    replacement: Option<StateReplacement>,
}

impl RecoveryRecord {
    pub fn new(
        operation: Uuid,
        base_revision: Uuid,
        kind: RecoveryKind,
        protocol_binding: Option<ProtocolBinding>,
        account: RecoveryAccount,
        references: RecoveryReferences,
    ) -> tembra::Result<Self> {
        super::non_nil(operation)?;
        super::non_nil(base_revision)?;
        account.validate()?;
        if matches!(kind, RecoveryKind::Renew | RecoveryKind::Defaults)
            && !matches!(
                &account,
                RecoveryAccount::Captured {
                    before: Some(_),
                    ..
                }
            )
        {
            return Err(crate::invalid(
                "recovery requires a captured existing account",
            ));
        }
        if kind == RecoveryKind::Authorize && references.issued().is_none() {
            return Err(crate::invalid(
                "authorization recovery requires its planned escrow",
            ));
        }
        if kind == RecoveryKind::Renew
            && (references.previous().is_none() || references.candidate().is_none())
        {
            return Err(crate::invalid(
                "renewal recovery requires both planned account references",
            ));
        }
        if (matches!(kind, RecoveryKind::Authorize | RecoveryKind::Renew)
            || references.values().next().is_some())
            && protocol_binding.is_none()
        {
            return Err(crate::invalid(
                "credential recovery requires a captured protocol binding",
            ));
        }
        if kind == RecoveryKind::Defaults
            && (protocol_binding.is_some() || references.values().next().is_some())
        {
            return Err(crate::invalid(
                "defaults recovery cannot contain credentials or protocol binding",
            ));
        }
        check_account_reference(references.previous(), account.previous_id())?;
        check_account_reference(references.candidate(), account.candidate_id())?;
        if let Some(reference) = references.issued()
            && !matches!(reference.slot(), CredentialSlot::Operation { operation: original } if *original == operation)
        {
            return Err(crate::invalid(
                "issued recovery material belongs to another operation",
            ));
        }
        Ok(Self {
            operation,
            base_revision,
            kind,
            protocol_binding,
            account,
            references,
            phase: RecoveryPhase::Intent,
            replacement: None,
        })
    }

    pub fn with_phase(
        mut self,
        phase: RecoveryPhase,
        replacement: Option<StateReplacement>,
    ) -> tembra::Result<Self> {
        let valid = matches!(
            (phase, replacement.map(StateReplacement::kind)),
            (
                RecoveryPhase::AccountPublication,
                Some(ReplacementKind::Publish)
            ) | (RecoveryPhase::Restore, Some(ReplacementKind::Restore))
                | (RecoveryPhase::Quarantine, Some(ReplacementKind::Quarantine))
                | (
                    RecoveryPhase::Committed,
                    Some(
                        ReplacementKind::Publish
                            | ReplacementKind::Restore
                            | ReplacementKind::Quarantine,
                    ),
                )
                | (
                    RecoveryPhase::Retirement,
                    None | Some(ReplacementKind::Clear)
                )
                | (
                    RecoveryPhase::Intent
                        | RecoveryPhase::ExchangeMayStart
                        | RecoveryPhase::IssuedReady
                        | RecoveryPhase::CandidateStaging
                        | RecoveryPhase::CandidateReady,
                    None,
                )
        );
        if !valid
            || (phase == RecoveryPhase::ExchangeMayStart
                && !matches!(self.kind, RecoveryKind::Authorize | RecoveryKind::Renew))
            || (phase == RecoveryPhase::IssuedReady
                && (self.kind != RecoveryKind::Authorize
                    || !matches!(&self.account, RecoveryAccount::Unassociated { .. })
                    || self.references.issued().is_none()
                    || self.protocol_binding.is_none()))
            || (matches!(
                phase,
                RecoveryPhase::CandidateReady | RecoveryPhase::AccountPublication
            ) && !matches!(&self.account, RecoveryAccount::Captured { .. }))
            || (self.kind == RecoveryKind::Defaults
                && matches!(
                    phase,
                    RecoveryPhase::CandidateStaging
                        | RecoveryPhase::CandidateReady
                        | RecoveryPhase::Quarantine
                ))
        {
            return Err(crate::invalid(
                "invalid provider recovery phase or replacement plan",
            ));
        }
        self.phase = phase;
        self.replacement = replacement;
        Ok(self)
    }

    pub fn operation(&self) -> Uuid {
        self.operation
    }

    pub fn base_revision(&self) -> Uuid {
        self.base_revision
    }

    pub fn kind(&self) -> RecoveryKind {
        self.kind
    }

    pub fn protocol_binding(&self) -> Option<ProtocolBinding> {
        self.protocol_binding
    }

    pub fn account(&self) -> &RecoveryAccount {
        &self.account
    }

    pub fn references(&self) -> &RecoveryReferences {
        &self.references
    }

    pub fn phase(&self) -> RecoveryPhase {
        self.phase
    }

    pub fn replacement(&self) -> Option<StateReplacement> {
        self.replacement
    }

    pub(super) fn validate_for(
        &self,
        owner: &ProviderOwner,
        revision: Uuid,
        limits: RecoveryLimits,
    ) -> tembra::Result<()> {
        for reference in self.references.values() {
            if reference.owner() != owner {
                return Err(crate::invalid(
                    "a recovery reference belongs to another owner",
                ));
            }
            reference.check_size(limits.reference_bytes())?;
        }
        if let Some(replacement) = self.replacement {
            let expected = if self.phase == RecoveryPhase::Committed {
                replacement.target_revision()
            } else {
                replacement.expected_revision()
            };
            if expected != revision {
                return Err(crate::invalid(
                    "the recovery plan does not match the state revision",
                ));
            }
        }
        super::encoded(self, limits.workflow_bytes()).map(|_| ())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordData {
    operation: Uuid,
    base_revision: Uuid,
    kind: RecoveryKind,
    #[serde(deserialize_with = "Option::deserialize")]
    protocol_binding: Option<ProtocolBinding>,
    account: RecoveryAccount,
    references: RecoveryReferences,
    phase: RecoveryPhase,
    #[serde(deserialize_with = "Option::deserialize")]
    replacement: Option<StateReplacement>,
}

impl TryFrom<RecordData> for RecoveryRecord {
    type Error = &'static str;

    fn try_from(value: RecordData) -> Result<Self, Self::Error> {
        Self::new(
            value.operation,
            value.base_revision,
            value.kind,
            value.protocol_binding,
            value.account,
            value.references,
        )
        .and_then(|record| record.with_phase(value.phase, value.replacement))
        .map_err(|_| "invalid provider recovery record")
    }
}

fn check_account_reference(
    reference: Option<&CredentialReference>,
    id: Option<&str>,
) -> tembra::Result<()> {
    if let Some(reference) = reference
        && !matches!(reference.slot(), CredentialSlot::Account { account_id } if Some(account_id.as_str()) == id)
    {
        return Err(crate::invalid(
            "a recovery reference does not match its captured account",
        ));
    }
    Ok(())
}
