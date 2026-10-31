use crate::{EncryptedRecord, Error, Result, VaultIdentity};
use serde::{Deserialize, Serialize};

/// Only ciphertext and safe metadata belong in this backend-neutral owner snapshot.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultSnapshot {
    pub revision: u64,
    pub signature: Vec<u8>,
    pub records: Vec<EncryptedRecord>,
    pub pending: Vec<crate::RecordCommand>,
    pub accepted: Vec<uuid::Uuid>,
    pub rejected: Vec<RejectedMutation>,
    pub cloud_cursor: u64,
    pub captures: Vec<crate::CaptureRecord>,
    pub authority_cursor: u64,
    pub authority: Option<crate::AuthorityCheck>,
}

/// A consuming adapter owns its transaction and durability; no cross-Cloud atomicity is implied.
pub trait VaultStorage {
    /// Uncertain or closed owners must gate new reads/leases until explicit storage recovery.
    fn health(&self) -> Result<()> {
        Ok(())
    }

    fn load(&self, identity: VaultIdentity) -> Result<VaultSnapshot>;

    /// Atomically replace this namespace only if its local revision still equals expected.
    /// Commit acknowledgement must settle before returning success.
    fn compare_exchange(
        &self,
        identity: VaultIdentity,
        expected: u64,
        next: &VaultSnapshot,
    ) -> Result<()>;
}

pub(crate) fn advance(snapshot: &mut VaultSnapshot) -> Result<u64> {
    let old = snapshot.revision;
    snapshot.revision = old.checked_add(1).ok_or(Error::Exhausted)?;
    Ok(old)
}

/// Encrypted value retained after a definitive rejection; resubmission requires a new human action.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RejectedMutation {
    pub command: crate::RecordCommand,
    pub outcome: crate::CommitOutcome,
}

impl VaultSnapshot {
    fn signing_bytes(&self, identity: VaultIdentity, device: uuid::Uuid) -> Result<Vec<u8>> {
        let mut unsigned = self.clone();
        unsigned.signature.clear();
        crate::canonical(&("vault-journal/v1", identity, device, unsigned))
    }
    pub(crate) fn sign(
        &mut self,
        identity: VaultIdentity,
        device: uuid::Uuid,
        keys: &crate::DeviceKeys,
    ) -> Result<()> {
        self.signature = keys.sign(&self.signing_bytes(identity, device)?);
        Ok(())
    }
    pub(crate) fn verify(
        &self,
        identity: VaultIdentity,
        device: uuid::Uuid,
        keys: &crate::DeviceKeys,
    ) -> Result<()> {
        if self.revision == 0
            && self.signature.is_empty()
            && crate::canonical(self)? == crate::canonical(&Self::default())?
        {
            return Ok(());
        }
        crate::verify_signature(
            &keys.signing_public(),
            &self.signing_bytes(identity, device)?,
            &self.signature,
        )
    }
}
