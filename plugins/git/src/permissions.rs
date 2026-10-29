//! Captured filesystem permissions. Unix integers retain their stored representation.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[cfg(windows)]
pub(crate) mod windows;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub(crate) enum Permissions {
    Unix(u32),
    Windows(WindowsPermissions),
}

/// The Windows profile includes discretionary access, ownership, integrity labels
/// and ordinary attributes. Audit policy is not a portable checkout permission.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WindowsPermissions {
    windows_sddl: String,
    windows_attributes: u32,
}

impl Permissions {
    pub(crate) fn capture(path: &Path, metadata: &fs::Metadata) -> Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            let mode = metadata.permissions().mode();
            if mode & 0o7000 != 0 || (metadata.is_file() && metadata.nlink() != 1) {
                return Err(Error::Invalid(
                    "special permission bits and hard links require another preservation profile"
                        .into(),
                ));
            }
            crate::worktrees::reject_extended_metadata(path)?;
            Ok(Self::Unix(mode & 0o777))
        }
        #[cfg(windows)]
        {
            windows::capture(path, metadata).map(Self::Windows)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (path, metadata);
            Err(Error::Invalid(
                "filesystem preservation is not qualified on this platform".into(),
            ))
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        match self {
            Self::Unix(mode) if mode & !0o777 != 0 => {
                Err(Error::Invalid("invalid preserved permission bits".into()))
            }
            #[cfg(unix)]
            Self::Unix(_) => Ok(()),
            #[cfg(windows)]
            Self::Windows(value) => windows::validate(value),
            _ => Err(Error::Invalid(
                "archive permissions require their original filesystem platform".into(),
            )),
        }
    }

    pub(crate) fn restore(&self, path: &Path) -> Result<()> {
        self.validate()?;
        match self {
            #[cfg(unix)]
            Self::Unix(mode) => {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(path, fs::Permissions::from_mode(*mode))?;
                Ok(())
            }
            #[cfg(windows)]
            Self::Windows(value) => windows::restore(path, value),
            _ => Err(Error::Invalid(
                "archive permissions require their original filesystem platform".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Permissions;

    #[test]
    fn stored_unix_permissions_keep_their_original_integer_representation() {
        let permissions: Permissions = serde_json::from_value(serde_json::json!(0o700)).unwrap();
        assert_eq!(permissions, Permissions::Unix(0o700));
        assert_eq!(
            serde_json::to_value(permissions).unwrap(),
            serde_json::json!(0o700)
        );
    }
}
