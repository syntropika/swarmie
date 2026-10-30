use std::path::Path;

#[cfg(not(windows))]
use std::fs;

use crate::Result;

/// Publish a synced file without replacing a previously admitted identity.
pub(crate) fn publish_new(temporary: &Path, destination: &Path) -> Result<bool> {
    #[cfg(windows)]
    {
        match move_file(temporary, destination, false) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
            Err(error) => Err(error.into()),
        }
    }
    #[cfg(not(windows))]
    {
        match fs::hard_link(temporary, destination) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return Ok(false),
            Err(error) => return Err(error.into()),
        }
        sync_parent(destination)?;
        Ok(true)
    }
}

/// Replace an owner-controlled file after its contents have been synced.
pub(crate) fn replace_file(temporary: &Path, destination: &Path) -> Result<()> {
    #[cfg(windows)]
    move_file(temporary, destination, true)?;
    #[cfg(not(windows))]
    {
        fs::rename(temporary, destination)?;
        sync_parent(destination)?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn sync_parent(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| crate::Error::Invalid("missing file parent".into()))?;
    sync_directory(parent)?;
    Ok(())
}

#[cfg(windows)]
fn move_file(source: &Path, destination: &Path, replace: bool) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    fn wide(path: &Path) -> std::io::Result<Vec<u16>> {
        let mut value: Vec<_> = path.as_os_str().encode_wide().collect();
        if value.contains(&0) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "path contains a null character",
            ));
        }
        value.push(0);
        Ok(value)
    }
    let source = wide(source)?;
    let destination = wide(destination)?;
    let flags = MOVEFILE_WRITE_THROUGH
        | if replace {
            MOVEFILE_REPLACE_EXISTING
        } else {
            0
        };
    // SAFETY: both paths are live, null-terminated UTF-16. No cross-volume copy
    // or delayed operation is permitted. Use the documented write-through move without a power-loss claim.
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), flags) } == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Flush an owner-controlled directory through a handle with platform-required access.
/// This requests OS persistence; it does not establish power-loss durability.
pub(crate) fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        };
        std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?
            .sync_all()?;
    }
    #[cfg(not(windows))]
    std::fs::File::open(path)?.sync_all()?;
    Ok(())
}
