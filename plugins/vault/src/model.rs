use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fmt};
use uuid::Uuid;
use zeroize::Zeroizing;

/// No Debug/Serialize/Clone: callers must opt into handling this buffer explicitly.
pub struct SecretValue(Zeroizing<Vec<u8>>);

impl SecretValue {
    pub fn new(bytes: Vec<u8>) -> Result<Self> {
        let bytes = Zeroizing::new(bytes);
        if bytes.is_empty() || bytes.len() > 65_536 || std::str::from_utf8(&bytes).is_err() {
            return Err(Error::Invalid);
        }
        Ok(Self(bytes))
    }

    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultIdentity {
    pub account: Uuid,
    pub vault: Uuid,
    pub epoch: Uuid,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretReference {
    pub account: Uuid,
    pub vault: Uuid,
    pub secret: Uuid,
}

impl fmt::Display for SecretReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{{{{secret:{}:{}}}}}", self.vault, self.secret)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "ids",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Scope {
    Unrestricted,
    Only(BTreeSet<Uuid>),
}

impl Scope {
    pub fn allows(&self, id: Option<Uuid>) -> bool {
        match self {
            Self::Unrestricted => true,
            Self::Only(ids) => id.is_some_and(|id| ids.contains(&id)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Approval {
    None,
    Operation,
    Conversation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretMetadata {
    pub name: String,
    pub purpose: String,
    pub projects: Scope,
    pub agents: Scope,
    pub approval: Approval,
    pub policy_generation: Uuid,
    pub target_account: Option<String>,
}

impl SecretMetadata {
    pub fn validate(&self) -> Result<()> {
        if self.name.is_empty()
            || self.name.len() > 256
            || self.purpose.len() > 4096
            || matches!(&self.projects,Scope::Only(ids) if ids.len()>256)
            || matches!(&self.agents,Scope::Only(ids) if ids.len()>256)
            || self.name.chars().any(char::is_control)
            || self.target_account.as_ref().is_some_and(|s| s.len() > 1024)
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordKind {
    Value,
    Deleted,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordHeader {
    pub version: u8,
    pub identity: VaultIdentity,
    pub secret: Uuid,
    pub revision: Uuid,
    pub previous: Option<Uuid>,
    pub successor: u64,
    pub metadata: SecretMetadata,
    pub kind: RecordKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncryptedRecord {
    pub header: RecordHeader,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
    pub authentication: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Decrypt,
    Manage,
    EnrollDevices,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceEnvelope {
    pub encapsulation: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentStatement {
    pub version: u8,
    pub identity: VaultIdentity,
    pub recipient: Uuid,
    pub signing_key: Vec<u8>,
    pub encryption_key: Option<Vec<u8>>,
    pub roles: BTreeSet<Role>,
    pub approver: Vec<u8>,
    pub nonce: Uuid,
    pub envelope_digest: Option<Vec<u8>>,
    pub commitment: Vec<u8>,
}

impl EnrollmentStatement {
    pub fn validate(&self) -> Result<()> {
        if self.version != 1
            || self.signing_key.len() != 32
            || self.approver.len() != 32
            || self.commitment.len() != 32
            || self.roles.is_empty()
            || (self.roles.contains(&Role::EnrollDevices) && !self.roles.contains(&Role::Decrypt))
        {
            return Err(Error::Invalid);
        }
        if self.roles.contains(&Role::Decrypt) {
            if self.encryption_key.as_ref().is_none_or(|k| k.len() != 32)
                || self.envelope_digest.as_ref().is_none_or(|k| k.len() != 32)
            {
                return Err(Error::Invalid);
            }
        } else if self.roles != BTreeSet::from([Role::Manage])
            || self.encryption_key.is_some()
            || self.envelope_digest.is_some()
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedEnrollment {
    pub statement: EnrollmentStatement,
    pub signature: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitOutcome {
    Accepted,
    NotDispatched,
    Unknown,
    RejectedVaultExists,
    RejectedStale,
    RejectedNameConflict,
    RejectedAuthorization,
    VaultNotCurrent,
    RootExposed,
}
