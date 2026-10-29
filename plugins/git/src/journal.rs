use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{Error, Intent, PendingOperation, Receipt, Result};

/// The embedding host authorizes every dispatched request, including recovery.
pub trait Authority: Send + Sync {
    fn authorize(&self, principal: &str, intent: &Intent) -> Result<()>;

    fn authorize_discovery(&self, _principal: &str, _path: &Path) -> Result<()> {
        Err(Error::Denied(
            "repository discovery requires a host path grant".into(),
        ))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "state", content = "value", rename_all = "snake_case")]
pub enum JournalEntry {
    Pending(PendingOperation),
    Settled(Receipt),
}

/// Admission must be durable before returning true. An existing ID is never replayed.
pub trait Journal: Send + Sync {
    fn admit(&self, operation: &PendingOperation) -> Result<bool>;
    fn finish(&self, receipt: &Receipt) -> Result<()>;
    fn load(&self, operation_id: &str) -> Result<Option<JournalEntry>>;
}

/// An owner-specific filesystem journal. Store it in a private host-owned directory.
/// Atomic rename and fsync preserve admission across process crashes on local filesystems.
pub struct DirectoryJournal {
    directory: PathBuf,
    lock: Mutex<()>,
}

impl DirectoryJournal {
    pub fn open(directory: impl Into<PathBuf>) -> Result<Self> {
        let directory = directory.into();
        fs::create_dir_all(&directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        }
        Ok(Self {
            directory: fs::canonicalize(directory)?,
            lock: Mutex::new(()),
        })
    }

    fn path(&self, operation_id: &str) -> Result<PathBuf> {
        if operation_id.is_empty() || operation_id.len() > 256 {
            return Err(Error::Invalid("invalid operation ID".into()));
        }
        Ok(self.directory.join(format!(
            "{:x}.json",
            Sha256::digest(operation_id.as_bytes())
        )))
    }

    fn write(path: &Path, value: &JournalEntry, create: bool) -> Result<bool> {
        let bytes = serde_json::to_vec(value).map_err(|error| Error::Journal(error.to_string()))?;
        if create {
            let mut file = match OpenOptions::new().create_new(true).write(true).open(path) {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    return Ok(false);
                }
                Err(error) => return Err(error.into()),
            };
            file.write_all(&bytes)?;
            file.sync_all()?;
        } else {
            let temporary = path.with_extension("settling");
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&temporary, path)?;
        }
        if let Some(parent) = path.parent() {
            fs::File::open(parent)?.sync_all()?;
        }
        Ok(true)
    }
}

impl Journal for DirectoryJournal {
    fn admit(&self, operation: &PendingOperation) -> Result<bool> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| Error::Journal("journal lock poisoned".into()))?;
        Self::write(
            &self.path(&operation.operation_id)?,
            &JournalEntry::Pending(operation.clone()),
            true,
        )
    }

    fn finish(&self, receipt: &Receipt) -> Result<()> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| Error::Journal("journal lock poisoned".into()))?;
        let path = self.path(&receipt.operation_id)?;
        let current = self
            .load(&receipt.operation_id)?
            .ok_or_else(|| Error::Journal("operation was not admitted".into()))?;
        match current {
            JournalEntry::Pending(value) => {
                if value.principal != receipt.principal
                    || value.target != receipt.target
                    || serde_json::to_value(value.operation).ok()
                        != serde_json::to_value(&receipt.operation).ok()
                {
                    return Err(Error::Journal(
                        "receipt identity differs from admitted operation".into(),
                    ));
                }
            }
            JournalEntry::Settled(value) => {
                if serde_json::to_value(value).ok() == serde_json::to_value(receipt).ok() {
                    return Ok(());
                }
                return Err(Error::Journal(
                    "confirmed operation receipt is immutable".into(),
                ));
            }
        }
        Self::write(&path, &JournalEntry::Settled(receipt.clone()), false)?;
        Ok(())
    }

    fn load(&self, operation_id: &str) -> Result<Option<JournalEntry>> {
        let bytes = match fs::read(self.path(operation_id)?) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| Error::Journal(error.to_string()))
    }
}
