use std::{
    fs,
    io::{Read, Write},
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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JournalUnavailable {
    pub entry_id: String,
    pub message: String,
}

#[derive(Clone, Debug, Default)]
pub struct JournalScan {
    pub entries: Vec<JournalEntry>,
    pub unavailable: Vec<JournalUnavailable>,
}

pub trait Journal: Send + Sync {
    fn admit(&self, operation: &PendingOperation) -> Result<bool>;
    fn finish(&self, receipt: &Receipt) -> Result<()>;
    fn load(&self, operation_id: &str) -> Result<Option<JournalEntry>>;
    fn scan(&self) -> Result<JournalScan> {
        Err(Error::Journal(
            "operation discovery is unavailable for this journal".into(),
        ))
    }
}

/// An owner-specific filesystem journal. Store it in a private host-owned directory.
/// Atomic publication and fsync preserve admission across process crashes on local filesystems.
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

    fn write(path: &Path, value: &JournalEntry) -> Result<bool> {
        let bytes = serde_json::to_vec(value).map_err(|error| Error::Journal(error.to_string()))?;
        if bytes.len() > 1024 * 1024 {
            return Err(Error::Journal(
                "operation record exceeds the recovery limit".into(),
            ));
        }
        let parent = path
            .parent()
            .ok_or_else(|| Error::Journal("journal path has no parent".into()))?;
        let mut temporary = tempfile::Builder::new()
            .prefix(".entry-")
            .tempfile_in(parent)?;
        temporary.write_all(&bytes)?;
        temporary.as_file().sync_all()?;
        // Publish only a fully written inode. Admission and settlement are both
        // immutable, including when two journal adapters race on one directory.
        crate::persistence::publish_new(temporary.path(), path)
    }
}

impl Journal for DirectoryJournal {
    fn admit(&self, operation: &PendingOperation) -> Result<bool> {
        if serde_json::to_vec(operation)
            .map_err(|error| Error::Journal(error.to_string()))?
            .len()
            > 512 * 1024
        {
            return Err(Error::Journal(
                "operation exceeds the admission budget; no effect was dispatched".into(),
            ));
        }
        let _guard = self
            .lock
            .lock()
            .map_err(|_| Error::Journal("journal lock poisoned".into()))?;
        Self::write(
            &self.path(&operation.operation_id)?,
            &JournalEntry::Pending(operation.clone()),
        )
    }

    fn finish(&self, receipt: &Receipt) -> Result<()> {
        if receipt.state == crate::Outcome::InProgress {
            return Err(Error::Journal(
                "a running observation is not a settled receipt".into(),
            ));
        }
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
        if !Self::write(
            &path.with_extension("receipt"),
            &JournalEntry::Settled(receipt.clone()),
        )? {
            let existing = self.load(&receipt.operation_id)?;
            if !matches!(existing, Some(JournalEntry::Settled(ref value)) if serde_json::to_value(value).ok() == serde_json::to_value(receipt).ok())
            {
                return Err(Error::Journal(
                    "confirmed operation receipt is immutable".into(),
                ));
            }
        }
        Ok(())
    }

    fn scan(&self) -> Result<JournalScan> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| Error::Journal("journal lock poisoned".into()))?;
        let mut scan = JournalScan::default();
        let mut paths = fs::read_dir(&self.directory)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        paths.sort();
        let mut unavailable = 0usize;
        for path in paths {
            if path.extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            let entry_id = path
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("invalid")
                .to_owned();
            let parsed = read_entry(&path).ok().flatten();
            let valid = parsed.filter(|entry| {
                let id = match entry {
                    JournalEntry::Pending(value) => &value.operation_id,
                    JournalEntry::Settled(value) => &value.operation_id,
                };
                self.path(id).is_ok_and(|expected| expected == path)
            });
            let current = valid.and_then(|entry| {
                let id = match &entry {
                    JournalEntry::Pending(value) => &value.operation_id,
                    JournalEntry::Settled(value) => &value.operation_id,
                };
                self.load(id).ok().flatten()
            });
            if let Some(entry) = current {
                scan.entries.push(entry);
            } else {
                unavailable += 1;
                if scan.unavailable.len() < 32 {
                    scan.unavailable.push(JournalUnavailable {entry_id,message:"Operation record is unreadable or corrupt; retain it for recovery and do not replay its effect.".into()});
                }
            }
        }
        if unavailable > 32 {
            scan.unavailable.push(JournalUnavailable {
                entry_id: "additional".into(),
                message: format!(
                    "{} additional operation records require recovery.",
                    unavailable - 32
                ),
            });
        }
        Ok(scan)
    }

    fn load(&self, operation_id: &str) -> Result<Option<JournalEntry>> {
        let path = self.path(operation_id)?;
        match read_entry(&path.with_extension("receipt"))? {
            Some(entry @ JournalEntry::Settled(_)) => Ok(Some(entry)),
            Some(JournalEntry::Pending(_)) => {
                Err(Error::Journal("settlement contains an admission".into()))
            }
            None => read_entry(&path),
        }
    }
}

fn read_entry(path: &Path) -> Result<Option<JournalEntry>> {
    let file = match crate::repository::open_regular_file(path) {
        Ok(file) => file,
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err(Error::Journal(
            "operation record exceeds the recovery limit".into(),
        ));
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| Error::Journal(error.to_string()))
}
