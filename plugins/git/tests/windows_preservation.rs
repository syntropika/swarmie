#![cfg(windows)]

use std::{fs, path::Path, process::Command, sync::Arc};
use swarmie_git::{
    ArchiveDirectory, ArchiveRestoreRequest, Authority, Cancellation, DirectoryJournal, Git,
    Intent, NativeProcess, RepositoryTarget, Request, Response, Result, WorktreeRequest,
};

struct Allow;
impl Authority for Allow {
    fn authorize(&self, _: &str, _: &Intent) -> Result<()> {
        Ok(())
    }
    fn authorize_discovery(&self, _: &str, _: &Path) -> Result<()> {
        Ok(())
    }
}
fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn ps(root: &Path, script: &str) -> String {
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!("$ErrorActionPreference='Stop'; {script}"),
        ])
        .env("SWARMIE_TEST_ROOT", root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn fixture() -> (tempfile::TempDir, Git) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("source");
    fs::create_dir(&root).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.name", "Test"]);
    git(&root, &["config", "user.email", "test@example.invalid"]);
    git(&root, &["config", "commit.gpgsign", "false"]);
    fs::write(root.join("file.txt"), "base\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-qm", "test: baseline"]);
    let capability = Git::new(
        Arc::new(NativeProcess),
        Arc::new(DirectoryJournal::open(temp.path().join("journal")).unwrap()),
        Arc::new(Allow),
    );
    (temp, capability)
}
fn discover(capability: &Git, root: &Path) -> RepositoryTarget {
    let Response::Repository {
        repository: Some(target),
    } = capability
        .execute(
            "test",
            Request::Discover {
                path: root.to_str().unwrap().into(),
            },
            &Cancellation::default(),
        )
        .unwrap()
    else {
        panic!("missing repository")
    };
    target
}
fn restore(archives: &ArchiveDirectory, capability: &Git, target: &RepositoryTarget, temp: &Path) {
    let receipt = archives
        .restore(
            capability,
            "test",
            target,
            &ArchiveRestoreRequest {
                archive_id: "snapshot".into(),
                managed_root: temp.into(),
                destination: temp.join("restored"),
            },
            &Cancellation::default(),
        )
        .unwrap();
    assert!(receipt.restore_verified);
}

#[test]
fn archive_preserves_nested_acls_readonly_attributes_and_detects_permission_changes() {
    let (temp, capability) = fixture();
    let source = temp.path().join("source");
    fs::create_dir(source.join("private")).unwrap();
    fs::write(source.join("private/data"), "staged\n").unwrap();
    git(&source, &["add", "private/data"]);
    fs::write(source.join("private/data"), "working\n").unwrap();
    ps(
        &source,
        r#"
        $path=Join-Path $env:SWARMIE_TEST_ROOT 'private';
        $acl=[IO.Directory]::GetAccessControl($path); $acl.SetAccessRuleProtection($true,$false);
        $user=[Security.Principal.WindowsIdentity]::GetCurrent().User;
        $rule=New-Object Security.AccessControl.FileSystemAccessRule($user,'FullControl','ContainerInherit,ObjectInherit','None','Allow');
        $acl.AddAccessRule($rule); [IO.Directory]::SetAccessControl($path,$acl);
        [IO.File]::SetAttributes((Join-Path $path 'data'),[IO.FileAttributes]'ReadOnly,Hidden,Archive');
    "#,
    );
    let before = ps(
        &source,
        r#"
        [IO.Directory]::GetAccessControl((Join-Path $env:SWARMIE_TEST_ROOT 'private')).GetSecurityDescriptorSddlForm('All');
        [IO.File]::GetAccessControl((Join-Path $env:SWARMIE_TEST_ROOT 'private/data')).GetSecurityDescriptorSddlForm('All');
        [int][IO.File]::GetAttributes((Join-Path $env:SWARMIE_TEST_ROOT 'private/data'));
    "#,
    );
    let target = discover(&capability, &source);
    let archives = ArchiveDirectory::open(temp.path().join("archives")).unwrap();
    archives
        .capture(
            &capability,
            "test",
            "snapshot",
            &target,
            &Cancellation::default(),
        )
        .unwrap();
    restore(&archives, &capability, &target, temp.path());
    let restored = temp.path().join("restored");
    let after = ps(
        &restored,
        r#"
        [IO.Directory]::GetAccessControl((Join-Path $env:SWARMIE_TEST_ROOT 'private')).GetSecurityDescriptorSddlForm('All');
        [IO.File]::GetAccessControl((Join-Path $env:SWARMIE_TEST_ROOT 'private/data')).GetSecurityDescriptorSddlForm('All');
        [int][IO.File]::GetAttributes((Join-Path $env:SWARMIE_TEST_ROOT 'private/data'));
    "#,
    );
    assert_eq!(before, after);
    assert_eq!(
        fs::read_to_string(restored.join("private/data")).unwrap(),
        "working\n"
    );
    assert_eq!(git(&restored, &["show", ":private/data"]), "staged");
    ps(
        &source,
        r#"
        $path=Join-Path $env:SWARMIE_TEST_ROOT 'private/data'; $acl=[IO.File]::GetAccessControl($path);
        $everyone=New-Object Security.Principal.SecurityIdentifier('S-1-1-0');
        $rule=New-Object Security.AccessControl.FileSystemAccessRule($everyone,'Read','Allow');
        $acl.AddAccessRule($rule); [IO.File]::SetAccessControl($path,$acl);
    "#,
    );
    assert!(
        archives
            .cleanup_eligibility(
                &capability,
                "test",
                "snapshot",
                &target,
                &Cancellation::default()
            )
            .unwrap()
            .changed_since_archive
    );
    assert!(source.join("private/data").exists());
    ps(
        temp.path(),
        r#"Get-ChildItem $env:SWARMIE_TEST_ROOT -Recurse -Force -File | ForEach-Object { $_.Attributes='Normal' }"#,
    );
}

#[test]
fn selected_transfer_preserves_the_source_acl_inside_a_private_checkout() {
    let (temp, capability) = fixture();
    let source = temp.path().join("source");
    fs::create_dir(source.join("nested")).unwrap();
    fs::write(source.join("nested/selected"), "selected\n").unwrap();
    let before = ps(
        &source,
        r#"[IO.File]::GetAccessControl((Join-Path $env:SWARMIE_TEST_ROOT 'nested/selected')).GetSecurityDescriptorSddlForm('All')"#,
    );
    let target = discover(&capability, &source);
    let destination = temp.path().join("selected");
    let created = capability
        .create_worktree(
            "test",
            &target,
            &WorktreeRequest {
                managed_root: temp.path().into(),
                destination: destination.clone(),
                commit: target.head.clone().unwrap(),
                include_paths: vec!["nested/selected".into()],
            },
            &Cancellation::default(),
        )
        .unwrap();
    assert!(
        created.preparation_error.is_none(),
        "{:?}",
        created.preparation_error
    );
    assert_eq!(
        before,
        ps(
            &destination,
            r#"[IO.File]::GetAccessControl((Join-Path $env:SWARMIE_TEST_ROOT 'nested/selected')).GetSecurityDescriptorSddlForm('All')"#
        )
    );
    assert_eq!(
        fs::read_to_string(source.join("nested/selected")).unwrap(),
        "selected\n"
    );
}

#[test]
fn alternate_streams_are_refused_without_dropping_their_content() {
    let (temp, capability) = fixture();
    let source = temp.path().join("source");
    let stream = source.join("file.txt:private");
    fs::write(&stream, "retained stream").unwrap();
    let target = discover(&capability, &source);
    let archives = ArchiveDirectory::open(temp.path().join("archives")).unwrap();
    let error = archives
        .capture(
            &capability,
            "test",
            "stream",
            &target,
            &Cancellation::default(),
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("alternate data streams"),
        "{error}"
    );
    let destination = temp.path().join("refused");
    let error = capability
        .create_worktree(
            "test",
            &target,
            &WorktreeRequest {
                managed_root: temp.path().into(),
                destination: destination.clone(),
                commit: target.head.clone().unwrap(),
                include_paths: vec!["file.txt".into()],
            },
            &Cancellation::default(),
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("alternate data streams"),
        "{error}"
    );
    assert!(!destination.exists());
    assert_eq!(fs::read_to_string(stream).unwrap(), "retained stream");
}

#[test]
fn hard_links_are_refused_without_replacing_source_files() {
    let (temp, capability) = fixture();
    let source = temp.path().join("source");
    fs::hard_link(source.join("file.txt"), source.join("alias")).unwrap();
    let target = discover(&capability, &source);
    let archives = ArchiveDirectory::open(temp.path().join("archives")).unwrap();
    let error = archives
        .capture(
            &capability,
            "test",
            "links",
            &target,
            &Cancellation::default(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("hard links"), "{error}");
    assert_eq!(fs::read_to_string(source.join("alias")).unwrap(), "base\n");
}

#[test]
fn directory_junctions_are_refused_without_traversing_their_target() {
    let (temp, capability) = fixture();
    let source = temp.path().join("source");
    let outside = temp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel"), "untouched").unwrap();
    ps(
        temp.path(),
        r#"New-Item -ItemType Junction -Path (Join-Path $env:SWARMIE_TEST_ROOT 'source/junction') -Target (Join-Path $env:SWARMIE_TEST_ROOT 'outside') | Out-Null"#,
    );
    let target = discover(&capability, &source);
    let archives = ArchiveDirectory::open(temp.path().join("archives")).unwrap();
    let error = archives
        .capture(
            &capability,
            "test",
            "junction",
            &target,
            &Cancellation::default(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("reparse"), "{error}");
    assert_eq!(
        fs::read_to_string(outside.join("sentinel")).unwrap(),
        "untouched"
    );
    fs::remove_dir(source.join("junction")).unwrap();
}

#[test]
fn foreign_archive_permissions_are_rejected_before_restore_creates_a_checkout() {
    use sha2::{Digest, Sha256};
    let (temp, capability) = fixture();
    let source = temp.path().join("source");
    let target = discover(&capability, &source);
    let root = temp.path().join("archives");
    let archives = ArchiveDirectory::open(&root).unwrap();
    archives
        .capture(
            &capability,
            "test",
            "snapshot",
            &target,
            &Cancellation::default(),
        )
        .unwrap();
    let archive = fs::read_dir(&root).unwrap().next().unwrap().unwrap().path();
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(archive.join("manifest.json")).unwrap()).unwrap();
    manifest["content"]["root_mode"] = serde_json::json!(0o755);
    let bytes = serde_json::to_vec(&manifest).unwrap();
    fs::write(archive.join("manifest.json"), &bytes).unwrap();
    fs::write(
        archive.join("manifest.sha256"),
        format!("{:x}", Sha256::digest(&bytes)),
    )
    .unwrap();
    let destination = temp.path().join("refused");
    let error = archives
        .restore(
            &capability,
            "test",
            &target,
            &ArchiveRestoreRequest {
                archive_id: "snapshot".into(),
                managed_root: temp.path().into(),
                destination: destination.clone(),
            },
            &Cancellation::default(),
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("original filesystem platform"),
        "{error}"
    );
    assert!(!destination.exists());
}

#[test]
fn archive_store_discretionary_access_is_restricted_to_the_execution_user() {
    let (temp, _) = fixture();
    let root = temp.path().join("archives");
    ArchiveDirectory::open(&root).unwrap();
    let result = ps(
        &root,
        r#"
        $acl=[IO.Directory]::GetAccessControl($env:SWARMIE_TEST_ROOT);
        if(-not $acl.AreAccessRulesProtected) { throw 'store inherits permissions' };
        $rules=$acl.GetAccessRules($true,$true,[Security.Principal.SecurityIdentifier]);
        if($rules.Count -ne 1) { throw 'store has extra access rules' };
        if($rules[0].IdentityReference -ne [Security.Principal.WindowsIdentity]::GetCurrent().User) { throw 'wrong store principal' };
        if($rules[0].AccessControlType -ne 'Allow') { throw 'missing store access' };
        'private'
    "#,
    );
    assert_eq!(result, "private");
}

fn integrity_label(path: &Path) -> String {
    use std::{os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{
                ConvertSecurityDescriptorToStringSecurityDescriptorW, GetNamedSecurityInfoW,
                SE_FILE_OBJECT,
            },
            LABEL_SECURITY_INFORMATION, SACL_SECURITY_INFORMATION,
        },
    };
    let name: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut descriptor = ptr::null_mut();
    let mut text = ptr::null_mut();
    let mut length = 0;
    // SAFETY: terminated path and writable outputs; only the integrity label is queried.
    assert_eq!(
        unsafe {
            GetNamedSecurityInfoW(
                name.as_ptr(),
                SE_FILE_OBJECT,
                LABEL_SECURITY_INFORMATION,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut descriptor,
            )
        },
        0
    );
    // SAFETY: descriptor remains an owned Windows allocation; conversion allocates text.
    assert_ne!(
        unsafe {
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                descriptor,
                1,
                SACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION,
                &mut text,
                &mut length,
            )
        },
        0
    );
    assert!(length > 0 && length < 16384);
    let mut count = 0;
    // SAFETY: a successful conversion returns a terminated UTF-16 allocation.
    while unsafe { *text.add(count) } != 0 {
        count += 1;
        assert!(count < 16384);
    }
    // SAFETY: the checked terminated string remains in the live conversion allocation.
    let label = String::from_utf16(unsafe { std::slice::from_raw_parts(text, count) }).unwrap();
    // SAFETY: both conversion allocations require LocalFree after observation.
    unsafe {
        LocalFree(text.cast());
        LocalFree(descriptor);
    }
    label
}

#[test]
fn mandatory_integrity_labels_survive_archive_restoration() {
    let (temp, capability) = fixture();
    let source = temp.path().join("source");
    let file = source.join("file.txt");
    let output = Command::new("icacls.exe")
        .arg(&file)
        .args(["/setintegritylevel", "L"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let before = integrity_label(&file);
    assert!(before.contains("LW"), "{before}");
    let target = discover(&capability, &source);
    let archives = ArchiveDirectory::open(temp.path().join("archives")).unwrap();
    archives
        .capture(
            &capability,
            "test",
            "snapshot",
            &target,
            &Cancellation::default(),
        )
        .unwrap();
    restore(&archives, &capability, &target, temp.path());
    assert_eq!(
        before,
        integrity_label(&temp.path().join("restored/file.txt"))
    );
}

#[test]
fn modern_parent_and_legacy_child_inheritance_controls_are_preserved() {
    let (temp, capability) = fixture();
    let source = temp.path().join("source");
    ps(
        &source,
        r#"
        $user=[Security.Principal.WindowsIdentity]::GetCurrent().User;
        $acl=[IO.Directory]::GetAccessControl($env:SWARMIE_TEST_ROOT);
        $rule=New-Object Security.AccessControl.FileSystemAccessRule($user,'FullControl','ContainerInherit,ObjectInherit','None','Allow');
        $acl.AddAccessRule($rule); [IO.Directory]::SetAccessControl($env:SWARMIE_TEST_ROOT,$acl);
        Add-Type 'using System.Runtime.InteropServices; public class LegacyAcl { [DllImport("advapi32.dll",CharSet=CharSet.Unicode,SetLastError=true)] public static extern bool SetFileSecurityW(string p,uint f,byte[] d); }';
        $file=Join-Path $env:SWARMIE_TEST_ROOT 'file.txt'; $acl=[IO.File]::GetAccessControl($file);
        if(-not [LegacyAcl]::SetFileSecurityW($file,4,$acl.GetSecurityDescriptorBinaryForm())) { throw 'legacy ACL assignment failed' };
    "#,
    );
    let inspect = r#"
        [IO.Directory]::GetAccessControl($env:SWARMIE_TEST_ROOT).GetSecurityDescriptorSddlForm('All');
        [IO.File]::GetAccessControl((Join-Path $env:SWARMIE_TEST_ROOT 'file.txt')).GetSecurityDescriptorSddlForm('All');
    "#;
    let before = ps(&source, inspect);
    let lines: Vec<_> = before.lines().collect();
    assert!(lines[0].contains("D:AI"), "{before}");
    assert!(!lines[1].contains("D:AI"), "{before}");
    let target = discover(&capability, &source);
    let archives = ArchiveDirectory::open(temp.path().join("archives")).unwrap();
    archives
        .capture(
            &capability,
            "test",
            "snapshot",
            &target,
            &Cancellation::default(),
        )
        .unwrap();
    restore(&archives, &capability, &target, temp.path());
    assert_eq!(before, ps(&temp.path().join("restored"), inspect));
}

#[test]
fn selected_transfer_never_reports_success_when_private_parent_inheritance_differs() {
    let (temp, capability) = fixture();
    let source = temp.path().join("source");
    let inspect = r#"[IO.File]::GetAccessControl((Join-Path $env:SWARMIE_TEST_ROOT 'file.txt')).GetSecurityDescriptorSddlForm('All')"#;
    ps(
        &source,
        r#"
        $file=Join-Path $env:SWARMIE_TEST_ROOT 'file.txt'; $acl=[IO.File]::GetAccessControl($file);
        $user=[Security.Principal.WindowsIdentity]::GetCurrent().User;
        $rule=New-Object Security.AccessControl.FileSystemAccessRule($user,'FullControl','Allow');
        $acl.AddAccessRule($rule); [IO.File]::SetAccessControl($file,$acl);
    "#,
    );
    let before = ps(&source, inspect);
    assert!(before.contains("D:AI"), "{before}");
    let target = discover(&capability, &source);
    let destination = temp.path().join("selected");
    let created = capability
        .create_worktree(
            "test",
            &target,
            &WorktreeRequest {
                managed_root: temp.path().into(),
                destination: destination.clone(),
                commit: target.head.clone().unwrap(),
                include_paths: vec!["file.txt".into()],
            },
            &Cancellation::default(),
        )
        .unwrap();
    assert!(
        created
            .preparation_error
            .as_deref()
            .is_some_and(|error| error.contains("could not be restored exactly")),
        "{created:?}"
    );
    assert!(destination.exists());
    assert_eq!(before, ps(&source, inspect));
    assert_eq!(
        fs::read_to_string(source.join("file.txt")).unwrap(),
        "base\n"
    );
}
