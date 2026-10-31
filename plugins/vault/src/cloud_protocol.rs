use crate::{DeviceKeys, EncryptedRecord, Error, Result, canonical};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Exact signed record command persisted before dispatch. Session evidence is transport-owned.
/// This narrow type cannot accidentally persist a usable device-envelope founding action.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordCommand {
    pub operation: Uuid,
    pub nonce: Uuid,
    pub device: Uuid,
    pub action: RecordAction,
    pub signature: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RecordAction {
    Commit { record: EncryptedRecord },
}

impl RecordCommand {
    pub fn prepare(keys: &DeviceKeys, device: Uuid, record: EncryptedRecord) -> Result<Self> {
        let mut command = Self {
            operation: record.header.revision,
            nonce: Uuid::new_v4(),
            device,
            action: RecordAction::Commit { record },
            signature: vec![],
        };
        command.signature = keys.sign(&command.signing_bytes()?);
        Ok(command)
    }
    pub fn record(&self) -> &EncryptedRecord {
        match &self.action {
            RecordAction::Commit { record } => record,
        }
    }
    pub fn signing_bytes(&self) -> Result<Vec<u8>> {
        if self.operation != self.record().header.revision {
            return Err(Error::Integrity);
        }
        canonical(&(
            "vault-command/v1",
            self.operation,
            self.nonce,
            self.device,
            &self.action,
        ))
    }
}

/// Explicit hosted admission cursor; local record counts never substitute for this value.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudRecordPage {
    pub identity: crate::VaultIdentity,
    pub after: u64,
    pub records: Vec<EncryptedRecord>,
    pub next: u64,
    pub more: bool,
    pub current: bool,
    pub exposed: bool,
}
