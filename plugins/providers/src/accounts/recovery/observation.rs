use super::{ProviderOwner, ProviderState, RecoveryLimits};
use crate::AccountRecord;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use uuid::Uuid;
use zeroize::Zeroizing;

/// Complete is the only variant containing secret material. Missing, partial,
/// corrupt and unavailable reads must not become a fallback credential. This
/// type intentionally has no Debug, serialization or public State projection.
pub enum CredentialRead {
    Complete(Zeroizing<String>),
    Missing,
    Partial,
    Invalid,
}

/// Positive provisioning-plan data from an injected backend. Constructing this
/// value grants no initialization permission. Native must retain its independent
/// real provisioning receipt, physical lease and durable planned target.
#[derive(Clone)]
pub struct ProvisioningPlan {
    owner: ProviderOwner,
    initial_target: Uuid,
}

impl ProvisioningPlan {
    pub fn new(owner: ProviderOwner, initial_target: Uuid) -> tembra::Result<Self> {
        super::non_nil(initial_target)?;
        Ok(Self {
            owner,
            initial_target,
        })
    }

    pub fn owner(&self) -> &ProviderOwner {
        &self.owner
    }

    pub fn initial_target(&self) -> Uuid {
        self.initial_target
    }
}

/// Exact ordinary-file predecessor data, not authority to adopt, read secrets
/// or replace a file. Native must retain/recheck the actual bytes under its lease.
#[derive(Clone)]
pub struct LegacyPredecessor {
    owner: ProviderOwner,
    image_digest: [u8; 32],
    image_bytes: usize,
}

impl LegacyPredecessor {
    pub fn owner(&self) -> &ProviderOwner {
        &self.owner
    }

    pub fn image_bytes(&self) -> usize {
        self.image_bytes
    }

    /// Byte identity alone does not prove native settlement or safe legacy
    /// credential adoption. It cannot establish that a refresh was not consumed.
    pub fn matches_bytes(&self, owner: &ProviderOwner, bytes: &[u8]) -> bool {
        owner == &self.owner
            && bytes.len() == self.image_bytes
            && <[u8; 32]>::from(Sha256::digest(bytes)) == self.image_digest
    }
}

/// Bounded legacy metadata and its exact predecessor. It contains no protected
/// credential observation and cannot install a legacy selector fallback.
pub struct LegacyObservation {
    bytes: Vec<u8>,
    records: Vec<AccountRecord>,
    predecessor: LegacyPredecessor,
}

impl LegacyObservation {
    pub fn from_bytes(
        owner: ProviderOwner,
        bytes: Vec<u8>,
        limits: RecoveryLimits,
    ) -> tembra::Result<Self> {
        if bytes.len() > limits.envelope_bytes() {
            return Err(crate::invalid(
                "legacy provider data exceeds its storage bound",
            ));
        }
        let records: Vec<AccountRecord> = super::decode::decode(&bytes)?;
        let mut ids = BTreeSet::new();
        for record in &records {
            super::state::account_identity(record)?;
            if !ids.insert(record.account.id.as_str()) {
                return Err(crate::invalid(
                    "legacy provider data contains duplicate account IDs",
                ));
            }
        }
        let predecessor = LegacyPredecessor {
            owner,
            image_digest: Sha256::digest(&bytes).into(),
            image_bytes: bytes.len(),
        };
        Ok(Self {
            bytes,
            records,
            predecessor,
        })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn records(&self) -> &[AccountRecord] {
        &self.records
    }

    pub fn predecessor(&self) -> &LegacyPredecessor {
        &self.predecessor
    }

    pub fn into_parts(self) -> (Vec<AccountRecord>, LegacyPredecessor) {
        (self.records, self.predecessor)
    }
}

/// Classified storage observation. No variant grants native readiness, and
/// Absent cannot initialize an empty state. Corruption/unavailability are errors
/// from the future storage port, not additional empty-state variants here.
pub enum StateLoad {
    Initialized(ProviderState),
    Provisioning(ProvisioningPlan),
    Absent,
    Legacy(LegacyObservation),
}
