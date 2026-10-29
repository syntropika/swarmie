//! A bounded NTFS checkout profile, without privilege changes or backup-policy claims.

use super::WindowsPermissions;
use crate::{Error, Result};
use std::{
    ffi::c_void,
    fs, io,
    mem::size_of,
    os::windows::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    ptr,
};
use windows_sys::Win32::{
    Foundation::{ERROR_ALREADY_EXISTS, ERROR_INSUFFICIENT_BUFFER, LocalFree},
    Security::{
        ACE_HEADER, ACL, ATTRIBUTE_SECURITY_INFORMATION,
        Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW, ConvertSidToStringSidW,
            ConvertStringSecurityDescriptorToSecurityDescriptorW, GetNamedSecurityInfoW,
            SE_FILE_OBJECT, SetNamedSecurityInfoW,
        },
        DACL_SECURITY_INFORMATION, EqualSid, GROUP_SECURITY_INFORMATION, GetAce,
        GetSecurityDescriptorControl, GetSecurityDescriptorDacl, GetSecurityDescriptorGroup,
        GetSecurityDescriptorOwner, GetSecurityDescriptorSacl, GetTokenInformation, IsValidAcl,
        LABEL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, SACL_SECURITY_INFORMATION,
        SCOPE_SECURITY_INFORMATION, SE_DACL_AUTO_INHERITED, SE_SACL_AUTO_INHERITED,
        SECURITY_ATTRIBUTES, SetFileSecurityW, TOKEN_QUERY, TOKEN_USER, TokenUser,
    },
    Storage::FileSystem::{
        BACKUP_DATA, BY_HANDLE_FILE_INFORMATION, BackupRead, BackupSeek, CreateDirectoryW,
        FILE_ATTRIBUTE_ARCHIVE, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_HIDDEN,
        FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_NOT_CONTENT_INDEXED, FILE_ATTRIBUTE_READONLY,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_SYSTEM, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, GetFileInformationByHandle, SetFileAttributesW,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

const ATTRIBUTES: u32 = FILE_ATTRIBUTE_READONLY
    | FILE_ATTRIBUTE_HIDDEN
    | FILE_ATTRIBUTE_SYSTEM
    | FILE_ATTRIBUTE_DIRECTORY
    | FILE_ATTRIBUTE_ARCHIVE
    | FILE_ATTRIBUTE_NORMAL
    | FILE_ATTRIBUTE_NOT_CONTENT_INDEXED;
// Query access-affecting SACL entries without enabling security/backup privileges.
// Audit ACEs need ACCESS_SYSTEM_SECURITY and are outside this checkout profile.
const SECURITY: u32 = OWNER_SECURITY_INFORMATION
    | GROUP_SECURITY_INFORMATION
    | DACL_SECURITY_INFORMATION
    | LABEL_SECURITY_INFORMATION
    | ATTRIBUTE_SECURITY_INFORMATION
    | SCOPE_SECURITY_INFORMATION;
const STRING_SECURITY: u32 = SECURITY | SACL_SECURITY_INFORMATION;

fn unsupported(message: &str) -> Error {
    Error::Invalid(message.into())
}
fn checked(success: i32) -> Result<()> {
    if success == 0 {
        Err(io::Error::last_os_error().into())
    } else {
        Ok(())
    }
}
fn wide(path: &std::ffi::OsStr) -> Result<Vec<u16>> {
    let mut value: Vec<_> = path.encode_wide().collect();
    if value.contains(&0) {
        return Err(unsupported("filesystem value contains a null character"));
    }
    value.push(0);
    Ok(value)
}

struct Local(*mut c_void);
impl Drop for Local {
    fn drop(&mut self) {
        // SAFETY: every non-null allocation is returned by a documented LocalAlloc API.
        unsafe {
            LocalFree(self.0);
        }
    }
}
fn local_text(value: Local) -> Result<String> {
    let pointer = value.0.cast::<u16>();
    let mut length = 0;
    // SAFETY: successful Windows conversion returns a terminated UTF-16 allocation.
    while unsafe { *pointer.add(length) } != 0 {
        length += 1;
        if length > 16 * 1024 {
            return Err(unsupported(
                "security descriptor exceeds this profile's limit",
            ));
        }
    }
    // SAFETY: the checked terminated string remains owned by value.
    String::from_utf16(unsafe { std::slice::from_raw_parts(pointer, length) })
        .map_err(|_| unsupported("invalid Windows security text"))
}
fn parse(sddl: &str) -> Result<Local> {
    if sddl.len() > 16 * 1024 {
        return Err(unsupported(
            "security descriptor exceeds this profile's limit",
        ));
    }
    let text = wide(std::ffi::OsStr::new(sddl))?;
    let mut descriptor = ptr::null_mut();
    // SAFETY: text is terminated and the output pointer is writable.
    checked(unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text.as_ptr(),
            1,
            &mut descriptor,
            ptr::null_mut(),
        )
    })?;
    Ok(Local(descriptor))
}
fn descriptor(path: &Path) -> Result<Local> {
    let path = wide(path.as_os_str())?;
    let mut descriptor = ptr::null_mut();
    // SAFETY: the path is terminated; the API allocates the returned descriptor.
    let error = unsafe {
        GetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            SECURITY,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if error != 0 {
        return Err(io::Error::from_raw_os_error(error as i32).into());
    }
    Ok(Local(descriptor))
}
fn text(descriptor: &Local) -> Result<String> {
    let mut text = ptr::null_mut();
    // SAFETY: descriptor is a live, OS-created security descriptor.
    checked(unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor.0,
            1,
            STRING_SECURITY,
            &mut text,
            ptr::null_mut(),
        )
    })?;
    local_text(Local(text.cast()))
}
struct Parts {
    owner: *mut c_void,
    group: *mut c_void,
    dacl: *mut ACL,
    sacl: *mut ACL,
    control: u16,
}
fn parts(descriptor: &Local) -> Result<Parts> {
    let (mut owner, mut group, mut dacl, mut sacl) = (
        ptr::null_mut(),
        ptr::null_mut(),
        ptr::null_mut(),
        ptr::null_mut(),
    );
    let (mut present, mut defaulted) = (0, 0);
    // SAFETY: pointers borrow the validated descriptor's live LocalAlloc storage.
    unsafe {
        checked(GetSecurityDescriptorOwner(
            descriptor.0,
            &mut owner,
            &mut defaulted,
        ))?;
        checked(GetSecurityDescriptorGroup(
            descriptor.0,
            &mut group,
            &mut defaulted,
        ))?;
        checked(GetSecurityDescriptorDacl(
            descriptor.0,
            &mut present,
            &mut dacl,
            &mut defaulted,
        ))?;
        if owner.is_null()
            || group.is_null()
            || present == 0
            || dacl.is_null()
            || IsValidAcl(dacl) == 0
        {
            return Err(unsupported(
                "missing ownership or discretionary ACL requires another preservation profile",
            ));
        }
        checked(GetSecurityDescriptorSacl(
            descriptor.0,
            &mut present,
            &mut sacl,
            &mut defaulted,
        ))?;
        if present != 0 && !sacl.is_null() {
            if IsValidAcl(sacl) == 0 {
                return Err(unsupported("invalid integrity ACL"));
            }
            for number in 0..(*sacl).AceCount {
                let mut ace = ptr::null_mut();
                checked(GetAce(sacl, number.into(), &mut ace))?;
                // SYSTEM_MANDATORY_LABEL_ACE_TYPE. Resource claims and central
                // access policy require separate authority and are refused.
                if (*ace.cast::<ACE_HEADER>()).AceType != 0x11 {
                    return Err(unsupported(
                        "resource or central access policy requires another preservation profile",
                    ));
                }
            }
        } else {
            sacl = ptr::null_mut();
        }
    }
    let (mut control, mut revision) = (0, 0);
    // SAFETY: the descriptor remains live and both output values are writable.
    checked(unsafe { GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) })?;
    Ok(Parts {
        owner,
        group,
        dacl,
        sacl,
        control,
    })
}
fn acl_bytes(acl: *mut ACL) -> Vec<u8> {
    if acl.is_null() {
        return Vec::new();
    }
    // SAFETY: parts validated this ACL and its OS-created allocation remains live.
    unsafe { std::slice::from_raw_parts(acl.cast::<u8>(), (*acl).AclSize.into()).to_vec() }
}

pub(super) fn validate(value: &WindowsPermissions) -> Result<()> {
    if value.windows_attributes & !ATTRIBUTES != 0 {
        return Err(unsupported("unsupported preserved Windows attributes"));
    }
    parts(&parse(&value.windows_sddl)?)?;
    Ok(())
}

pub(super) fn capture(path: &Path, metadata: &fs::Metadata) -> Result<WindowsPermissions> {
    let attributes = metadata.file_attributes();
    if attributes & !ATTRIBUTES != 0 {
        return Err(unsupported(
            "reparse, sparse, encrypted or extended attributes require another preservation profile",
        ));
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the live handle and correctly sized output buffer remain owned here.
    checked(unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut information) })?;
    if information.dwFileAttributes & !ATTRIBUTES != 0
        || (metadata.is_file() && information.nNumberOfLinks != 1)
    {
        return Err(unsupported(
            "hard links or changed attributes require another preservation profile",
        ));
    }
    reject_extra_streams(file)?;
    let descriptor = descriptor(path)?;
    parts(&descriptor)?;
    Ok(WindowsPermissions {
        windows_sddl: text(&descriptor)?,
        windows_attributes: attributes,
    })
}

pub(super) fn restore(path: &Path, value: &WindowsPermissions) -> Result<()> {
    validate(value)?;
    if fs::symlink_metadata(path)?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(unsupported("restoration does not modify reparse points"));
    }
    let expected = parse(&value.windows_sddl)?;
    let current = descriptor(path)?;
    let (wanted, existing) = (parts(&expected)?, parts(&current)?);
    let mut fields = DACL_SECURITY_INFORMATION;
    // SAFETY: both pairs of SID pointers borrow live, OS-validated descriptors.
    unsafe {
        if EqualSid(wanted.owner, existing.owner) == 0 {
            fields |= OWNER_SECURITY_INFORMATION;
        }
        if EqualSid(wanted.group, existing.group) == 0 {
            fields |= GROUP_SECURITY_INFORMATION;
        }
    }
    if acl_bytes(wanted.sacl) != acl_bytes(existing.sacl) {
        fields |= LABEL_SECURITY_INFORMATION;
    }
    let path = wide(path.as_os_str())?;
    // SAFETY: this legacy per-object API deliberately does not propagate directory
    // ACLs to existing children. SetNamedSecurityInfo would rewrite their captured
    // inherited ACEs. The final inventory checks the exact restored descriptors.
    checked(unsafe { SetFileSecurityW(path.as_ptr(), fields, expected.0) })?;
    let mut automatic = 0;
    if wanted.control & SE_DACL_AUTO_INHERITED != 0 {
        automatic |= DACL_SECURITY_INFORMATION;
    }
    if wanted.control & SE_SACL_AUTO_INHERITED != 0 {
        automatic |= LABEL_SECURITY_INFORMATION;
    }
    if automatic != 0 {
        // The legacy call clears the automatic-inheritance model bits. Reapply
        // modern ACLs through their owning API only when the captured descriptor
        // used that model. Parent propagation must reproduce the complete inventory;
        // a changed ACL or control flag prevents a verified restoration receipt.
        // SAFETY: all pointers borrow the live parsed descriptor. No privileges change.
        let error = unsafe {
            SetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                automatic,
                ptr::null_mut(),
                ptr::null_mut(),
                wanted.dacl,
                wanted.sacl,
            )
        };
        if error != 0 {
            return Err(io::Error::from_raw_os_error(error as i32).into());
        }
    }
    // SAFETY: ordinary captured attributes only; directory identity is unchanged.
    checked(unsafe { SetFileAttributesW(path.as_ptr(), value.windows_attributes) })?;
    Ok(())
}

struct StreamScan {
    file: fs::File,
    context: *mut c_void,
}
impl Drop for StreamScan {
    fn drop(&mut self) {
        let mut read = 0;
        // SAFETY: abort releases the context while the original handle is live.
        unsafe {
            BackupRead(
                self.file.as_raw_handle(),
                ptr::null_mut(),
                0,
                &mut read,
                1,
                0,
                &mut self.context,
            );
        }
    }
}
fn reject_extra_streams(file: fs::File) -> Result<()> {
    let mut scan = StreamScan {
        file,
        context: ptr::null_mut(),
    };
    for _ in 0..64 {
        // WIN32_STREAM_ID's fixed header is 20 bytes, before its variable name.
        let mut header = [0u8; 20];
        let mut read = 0;
        // SAFETY: synchronous handle, writable buffer and private backup context.
        checked(unsafe {
            BackupRead(
                scan.file.as_raw_handle(),
                header.as_mut_ptr(),
                20,
                &mut read,
                0,
                0,
                &mut scan.context,
            )
        })?;
        if read == 0 {
            return Ok(());
        }
        if read != 20 {
            return Err(unsupported("incomplete filesystem stream header"));
        }
        let id = u32::from_le_bytes(
            header[0..4]
                .try_into()
                .map_err(|_| unsupported("invalid stream ID"))?,
        );
        let size = i64::from_le_bytes(
            header[8..16]
                .try_into()
                .map_err(|_| unsupported("invalid stream size"))?,
        );
        let name = u32::from_le_bytes(
            header[16..20]
                .try_into()
                .map_err(|_| unsupported("invalid stream name"))?,
        );
        if id != BACKUP_DATA || name != 0 || size < 0 {
            return Err(unsupported(
                "alternate data streams or extended metadata require another preservation profile",
            ));
        }
        let (mut low, mut high) = (0, 0);
        // SAFETY: seek only inside the current unnamed data stream; no name bytes.
        checked(unsafe {
            BackupSeek(
                scan.file.as_raw_handle(),
                size as u32,
                (size as u64 >> 32) as u32,
                &mut low,
                &mut high,
                &mut scan.context,
            )
        })?;
        if (u64::from(high) << 32 | u64::from(low)) != size as u64 {
            return Err(unsupported("incomplete filesystem stream observation"));
        }
    }
    Err(unsupported("filesystem stream inventory exceeds its limit"))
}

fn private_descriptor() -> Result<Local> {
    let mut token = ptr::null_mut();
    // SAFETY: request read-only information from this process's token.
    checked(unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) })?;
    // SAFETY: successful OpenProcessToken transfers a uniquely owned handle.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    let mut length = 0;
    // SAFETY: size query writes only the required byte count.
    let success = unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            ptr::null_mut(),
            0,
            &mut length,
        )
    };
    if success != 0
        || io::Error::last_os_error().raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32)
        || length as usize > 64 * 1024
        || (length as usize) < size_of::<TOKEN_USER>()
    {
        return Err(unsupported("cannot determine private directory owner"));
    }
    let mut buffer = vec![0usize; (length as usize).div_ceil(size_of::<usize>())];
    // SAFETY: aligned allocation holds the queried TOKEN_USER and embedded SID.
    checked(unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            length,
            &mut length,
        )
    })?;
    // SAFETY: successful size-checked query initialized TOKEN_USER.
    let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
    let mut sid = ptr::null_mut();
    // SAFETY: SID borrows the live token information buffer; output uses LocalAlloc.
    checked(unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid) })?;
    let sid = local_text(Local(sid.cast()))?;
    parse(&format!("O:{sid}D:P(A;OICI;FA;;;{sid})"))
}

pub(crate) fn create_private_directory(path: &Path) -> Result<()> {
    let descriptor = private_descriptor()?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    let name = wide(path.as_os_str())?;
    // SAFETY: security descriptor and terminated path remain live during creation.
    if unsafe { CreateDirectoryW(name.as_ptr(), &attributes) } == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_ALREADY_EXISTS as i32) {
            return Err(error.into());
        }
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(unsupported(
                "preservation directory is not a physical directory",
            ));
        }
    }
    Ok(())
}

pub(crate) fn protect_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(unsupported("archive storage requires a physical directory"));
    }
    let descriptor = private_descriptor()?;
    let path = wide(path.as_os_str())?;
    // SAFETY: this host-owned store receives an owner-only protected DACL. Existing
    // child ACLs are not propagated or altered by this per-object call.
    checked(unsafe { SetFileSecurityW(path.as_ptr(), DACL_SECURITY_INFORMATION, descriptor.0) })
}

/// Keep populated files protected while their parent ACLs are restored.
pub(crate) fn protect_file(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(unsupported("private file requires a physical regular file"));
    }
    let descriptor = private_descriptor()?;
    let path = wide(path.as_os_str())?;
    // SAFETY: this newly created owner-controlled file gets a protected user-only DACL.
    checked(unsafe { SetFileSecurityW(path.as_ptr(), DACL_SECURITY_INFORMATION, descriptor.0) })
}
