//! Keep access permissions on a replacement before publishing its contents.

use std::{ffi::c_void, io, os::windows::ffi::OsStrExt, path::Path, ptr};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{
        Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT, SetNamedSecurityInfoW},
        DACL_SECURITY_INFORMATION, GetSecurityDescriptorControl,
        PROTECTED_DACL_SECURITY_INFORMATION, SE_DACL_PROTECTED,
        UNPROTECTED_DACL_SECURITY_INFORMATION,
    },
};

struct Descriptor(*mut c_void);

impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: GetNamedSecurityInfoW returns a LocalAlloc-owned descriptor.
        unsafe {
            LocalFree(self.0);
        }
    }
}

fn wide(path: &Path) -> io::Result<Vec<u16>> {
    let mut text: Vec<_> = path.as_os_str().encode_wide().collect();
    if text.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "file path contains a null character",
        ));
    }
    text.push(0);
    Ok(text)
}

/// Copy the original DACL and its protection state before writing or renaming
/// the temporary. If copying fails, the original is never replaced. This does
/// not copy audit rules, ownership or privileges and is not a backup profile.
pub(super) fn preserve_dacl(original: &Path, temporary: &Path) -> io::Result<()> {
    let original = wide(original)?;
    let mut temporary = wide(temporary)?;
    let mut descriptor = ptr::null_mut();
    let mut dacl = ptr::null_mut();
    // SAFETY: the terminated source and all output pointers remain live.
    let result = unsafe {
        GetNamedSecurityInfoW(
            original.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if result != 0 {
        return Err(io::Error::from_raw_os_error(result as i32));
    }
    let descriptor = Descriptor(descriptor);
    let mut control = 0;
    let mut revision = 0;
    // SAFETY: descriptor is OS-created and both scalar outputs are writable.
    if unsafe { GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let protection = if control & SE_DACL_PROTECTED != 0 {
        PROTECTED_DACL_SECURITY_INFORMATION
    } else {
        UNPROTECTED_DACL_SECURITY_INFORMATION
    };
    // SAFETY: dacl belongs to the retained descriptor; only this newly created
    // temporary's DACL is changed. A null DACL retains the source's meaning.
    let result = unsafe {
        SetNamedSecurityInfoW(
            temporary.as_mut_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | protection,
            ptr::null_mut(),
            ptr::null_mut(),
            dacl,
            ptr::null_mut(),
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(result as i32))
    }
}
