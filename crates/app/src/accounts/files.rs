// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
//! Private owner metadata; account refresh credentials are stored only in the platform vault.
use std::{io::Write, path::Path};
use swarmie::{Error, Result};
pub fn directory(root: &Path) -> Result<()> {
    std::fs::create_dir_all(root)?;
    if std::fs::symlink_metadata(root)?.file_type().is_symlink() {
        return Err(Error::Storage(
            "account directory cannot be a symlink".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(windows)]
    protect_windows(root)?;
    Ok(())
}
pub fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
        Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 64 * 1024 => {
            return Err(Error::Storage("invalid account metadata".into()));
        }
        Ok(_) => {}
    }
    Ok(Some(serde_json::from_slice(&std::fs::read(path)?)?))
}
pub fn write(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    // Atomic publication replaces only metadata owned by this account installation.
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| -> Result<()> {
        let mut file = options.open(&temporary)?;
        file.write_all(&serde_json::to_vec(value)?)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}
pub fn lock(path: &Path) -> Result<std::fs::File> {
    if std::fs::symlink_metadata(path)
        .is_ok_and(|meta| !meta.is_file() || meta.file_type().is_symlink())
    {
        return Err(Error::Storage("invalid account owner lock".into()));
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    fs2::FileExt::try_lock_exclusive(&file).map_err(|_| Error::Busy)?;
    Ok(file)
}

#[cfg(windows)]
fn protect_windows(path: &Path) -> Result<()> {
    use std::os::windows::{ffi::OsStrExt, fs::MetadataExt};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
                SE_FILE_OBJECT, SetNamedSecurityInfoW,
            },
            DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl,
            PROTECTED_DACL_SECURITY_INFORMATION,
        },
    };
    if std::fs::symlink_metadata(path)?.file_attributes() & 0x400 != 0 {
        return Err(Error::Storage(
            "account directory cannot be a reparse point".into(),
        ));
    }
    let sid = crate::runtime::windows_pipe::user_sid()?;
    let descriptor: Vec<u16> = format!("D:P(A;OICI;FA;;;{sid})")
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        let mut security = std::ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            descriptor.as_ptr(),
            SDDL_REVISION_1,
            &mut security,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut present = 0;
        let mut defaulted = 0;
        let mut dacl = std::ptr::null_mut();
        let result = if GetSecurityDescriptorDacl(security, &mut present, &mut dacl, &mut defaulted)
            == 0
            || present == 0
        {
            Err(std::io::Error::last_os_error())
        } else {
            let error = SetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                dacl,
                std::ptr::null_mut(),
            );
            if error != 0 {
                Err(std::io::Error::from_raw_os_error(error as i32))
            } else {
                Ok(())
            }
        };
        LocalFree(security);
        result?;
    }
    Ok(())
}
