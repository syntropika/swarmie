use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Exact non-secret digest of one injected driver's protocol configuration.
/// Equality does not establish registration, account or recovery authority.
#[derive(Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProtocolBinding([u8; 32]);

impl ProtocolBinding {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// The exact authorized principal and stable profile identity. Native adapters
/// compare this value with their injected owner before protected-store access.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "OwnerData")]
pub struct ProviderOwner {
    principal: String,
    profile: Uuid,
}

impl ProviderOwner {
    pub fn new(principal: String, profile: Uuid) -> tembra::Result<Self> {
        super::non_nil(profile)?;
        if principal.is_empty() {
            return Err(crate::invalid("provider recovery requires a principal"));
        }
        Ok(Self { principal, profile })
    }

    pub fn principal(&self) -> &str {
        &self.principal
    }

    pub fn profile(&self) -> Uuid {
        self.profile
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnerData {
    principal: String,
    profile: Uuid,
}

impl TryFrom<OwnerData> for ProviderOwner {
    type Error = &'static str;

    fn try_from(value: OwnerData) -> Result<Self, Self::Error> {
        Self::new(value.principal, value.profile).map_err(|_| "invalid provider recovery owner")
    }
}

/// Slot input is validated when constructing a CredentialReference. Account
/// identifiers retain their exact UTF-8 bytes, including separators.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CredentialSlot {
    Account { account_id: String },
    Operation { operation: Uuid },
}

/// Address of an immutable protected generation. It contains no credential,
/// digest, filesystem path or permission. Native key-size preflight is separate.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ReferenceData")]
pub struct CredentialReference {
    owner: ProviderOwner,
    slot: CredentialSlot,
    generation: Uuid,
}

impl CredentialReference {
    pub fn new(
        owner: ProviderOwner,
        slot: CredentialSlot,
        generation: Uuid,
    ) -> tembra::Result<Self> {
        super::non_nil(generation)?;
        match &slot {
            CredentialSlot::Account { account_id } if account_id.is_empty() => {
                return Err(crate::invalid(
                    "a credential reference requires an account ID",
                ));
            }
            CredentialSlot::Operation { operation } => super::non_nil(*operation)?,
            CredentialSlot::Account { .. } => {}
        }
        Ok(Self {
            owner,
            slot,
            generation,
        })
    }

    pub fn owner(&self) -> &ProviderOwner {
        &self.owner
    }

    pub fn slot(&self) -> &CredentialSlot {
        &self.slot
    }

    pub fn generation(&self) -> Uuid {
        self.generation
    }

    /// Checks the configured ordinary-file encoding bound, without native I/O.
    /// This does not prove that a platform accepts its derived keys/descriptor.
    pub fn check_size(&self, maximum: usize) -> tembra::Result<()> {
        super::encoded(self, maximum).map(|_| ())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceData {
    owner: ProviderOwner,
    slot: CredentialSlot,
    generation: Uuid,
}

impl TryFrom<ReferenceData> for CredentialReference {
    type Error = &'static str;

    fn try_from(value: ReferenceData) -> Result<Self, Self::Error> {
        Self::new(value.owner, value.slot, value.generation)
            .map_err(|_| "invalid protected credential reference")
    }
}
