use std::{fs, path::Path, process::Command, sync::Arc};
use swarmie_git::{
    ArchiveDirectory, ArchiveRestoreRequest, Authority, Cancellation, DirectoryJournal, Git,
    Intent, NativeProcess, RepositoryTarget, Request, Response, Result, SetupScript,
    WorktreeRequest,
};
use tempfile::TempDir;

struct Allow;
impl Authority for Allow {
    fn authorize_discovery(&self, _: &str, _: &Path) -> Result<()> {
        Ok(())
    }
    fn authorize(&self, _: &str, _: &Intent) -> Result<()> {
        Ok(())
    }
}

fn native(root: &Path, args: &[&str]) -> String {
    let result = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8_lossy(&result.stdout).trim().into()
}

fn discover(git: &Git, root: &Path, cancel: &Cancellation) -> RepositoryTarget {
    match git
        .execute(
            "user",
            Request::Discover {
                path: root.to_str().unwrap().into(),
            },
            cancel,
        )
        .unwrap()
    {
        Response::Repository {
            repository: Some(target),
        } => target,
        other => panic!("expected discovered repository: {other:?}"),
    }
}

fn fixture() -> (TempDir, Git) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("source");
    fs::create_dir(&root).unwrap();
    native(&root, &["init", "-q"]);
    native(&root, &["config", "user.name", "Test"]);
    native(&root, &["config", "user.email", "test@example.invalid"]);
    fs::write(root.join("file.txt"), "base\n").unwrap();
    fs::write(root.join(".gitignore"), "ignored/\n").unwrap();
    native(&root, &["add", "."]);
    native(&root, &["commit", "-qm", "feat: initial fixture"]);
    let git = Git::new(
        Arc::new(NativeProcess),
        Arc::new(DirectoryJournal::open(temp.path().join("journal")).unwrap()),
        Arc::new(Allow),
    );
    (temp, git)
}

#[test]
fn detached_creation_preserves_selected_staged_working_and_untracked_content() {
    let (temp, git) = fixture();
    let source = temp.path().join("source");
    fs::write(source.join("file.txt"), "staged\n").unwrap();
    native(&source, &["add", "file.txt"]);
    fs::write(source.join("file.txt"), "working\n").unwrap();
    fs::write(source.join("new.txt"), "untracked\n").unwrap();
    let before = native(&source, &["status", "--porcelain=v1"]);
    let cancel = Cancellation::default();
    let target = discover(&git, &source, &cancel);
    let destination = temp.path().join("work");
    let created = git
        .create_worktree(
            "user",
            &target,
            &WorktreeRequest {
                managed_root: temp.path().into(),
                destination: destination.clone(),
                commit: target.head.clone().unwrap(),
                include_paths: vec!["file.txt".into(), "new.txt".into()],
            },
            &cancel,
        )
        .unwrap();
    assert!(
        created.preparation_error.is_none(),
        "{:?}",
        created.preparation_error
    );
    assert!(created.target.branch.is_none());
    assert_eq!(native(&destination, &["show", ":file.txt"]), "staged");
    assert_eq!(
        fs::read_to_string(destination.join("file.txt")).unwrap(),
        "working\n"
    );
    assert_eq!(
        fs::read_to_string(destination.join("new.txt")).unwrap(),
        "untracked\n"
    );
    assert_eq!(native(&source, &["status", "--porcelain=v1"]), before);
}

#[test]
fn dirty_transfer_refuses_a_different_base_before_creation() {
    let (temp, git) = fixture();
    let source = temp.path().join("source");
    let old = native(&source, &["rev-parse", "HEAD"]);
    fs::write(source.join("file.txt"), "second\n").unwrap();
    native(&source, &["commit", "-qam", "feat: second fixture"]);
    fs::write(source.join("file.txt"), "dirty\n").unwrap();
    let cancel = Cancellation::default();
    let target = discover(&git, &source, &cancel);
    let destination = temp.path().join("work");
    assert!(
        git.create_worktree(
            "user",
            &target,
            &WorktreeRequest {
                managed_root: temp.path().into(),
                destination: destination.clone(),
                commit: old,
                include_paths: vec!["file.txt".into()]
            },
            &cancel
        )
        .is_err()
    );
    assert!(!destination.exists());
}

#[cfg(unix)]
#[test]
fn failed_setup_is_reported_without_deleting_checkout() {
    let (temp, git) = fixture();
    let source = temp.path().join("source");
    let cancel = Cancellation::default();
    let target = discover(&git, &source, &cancel);
    let result = git
        .run_setup(
            "user",
            &target,
            &SetupScript {
                enabled: true,
                shell: "/bin/sh".into(),
                arguments: vec!["-c".into()],
                script: "printf 'setup failed' >&2; exit 7".into(),
                working_directory: ".".into(),
            },
            &cancel,
        )
        .unwrap();
    assert_eq!(result.status, 7);
    assert!(result.stderr.contains("setup failed"));
    assert!(source.join("file.txt").exists());
}

#[test]
fn archive_restores_detached_commits_index_working_and_ignored_files() {
    let (temp, git) = fixture();
    let source = temp.path().join("source");
    native(&source, &["checkout", "--detach", "-q"]);
    fs::write(source.join("file.txt"), "detached\n").unwrap();
    native(&source, &["commit", "-qam", "feat: detached fixture"]);
    fs::write(source.join("file.txt"), "staged\n").unwrap();
    native(&source, &["add", "file.txt"]);
    fs::write(source.join("file.txt"), "working\n").unwrap();
    fs::write(source.join("new.txt"), "untracked\n").unwrap();
    fs::create_dir(source.join("ignored")).unwrap();
    fs::write(source.join("ignored/cache"), "precious ignored content").unwrap();
    fs::create_dir(source.join("empty")).unwrap();
    let cancel = Cancellation::default();
    let target = discover(&git, &source, &cancel);
    let archives = ArchiveDirectory::open(temp.path().join("archives")).unwrap();
    let captured = archives
        .capture(&git, "user", "chat-1", &target, &cancel)
        .unwrap();
    assert!(!captured.restore_verified);
    let destination = temp.path().join("restored");
    let restored = archives
        .restore(
            &git,
            "user",
            &target,
            &ArchiveRestoreRequest {
                archive_id: "chat-1".into(),
                managed_root: temp.path().into(),
                destination: destination.clone(),
            },
            &cancel,
        )
        .unwrap();
    assert!(restored.restore_verified);
    assert_eq!(
        native(&destination, &["rev-parse", "HEAD"]),
        target.head.unwrap()
    );
    assert_eq!(native(&destination, &["show", ":file.txt"]), "staged");
    assert_eq!(
        fs::read_to_string(destination.join("file.txt")).unwrap(),
        "working\n"
    );
    assert_eq!(
        fs::read_to_string(destination.join("ignored/cache")).unwrap(),
        "precious ignored content"
    );
    assert!(destination.join("empty").is_dir());
}

#[test]
fn changed_source_is_never_deleted_using_an_old_archive() {
    let (temp, git) = fixture();
    let source = temp.path().join("source");
    let cancel = Cancellation::default();
    let target = discover(&git, &source, &cancel);
    let archives = ArchiveDirectory::open(temp.path().join("archives")).unwrap();
    archives
        .capture(&git, "user", "chat", &target, &cancel)
        .unwrap();
    fs::write(source.join("later.txt"), "not archived").unwrap();
    let eligibility = archives
        .cleanup_eligibility(&git, "user", "chat", &target, &cancel)
        .unwrap();
    assert!(!eligibility.eligible);
    assert!(eligibility.changed_since_archive);
    assert!(source.join("later.txt").exists());
}

#[test]
fn archives_are_scoped_to_the_capturing_principal() {
    let (temp, git) = fixture();
    let source = temp.path().join("source");
    let cancel = Cancellation::default();
    let target = discover(&git, &source, &cancel);
    let archives = ArchiveDirectory::open(temp.path().join("archives")).unwrap();
    archives
        .capture(&git, "alice", "private", &target, &cancel)
        .unwrap();
    assert_eq!(
        archives
            .list(&git, "alice", &target, &cancel)
            .unwrap()
            .archives
            .len(),
        1
    );
    assert!(
        archives
            .list(&git, "bob", &target, &cancel)
            .unwrap()
            .archives
            .is_empty()
    );
    assert!(
        archives
            .cleanup_eligibility(&git, "bob", "private", &target, &cancel)
            .is_err()
    );
    assert!(
        archives
            .restore(
                &git,
                "bob",
                &target,
                &ArchiveRestoreRequest {
                    archive_id: "private".into(),
                    managed_root: temp.path().into(),
                    destination: temp.path().join("stolen")
                },
                &cancel
            )
            .is_err()
    );
    assert!(!temp.path().join("stolen").exists());
}

#[test]
fn selected_staged_deletion_remains_deleted_in_new_checkout() {
    let (temp, git) = fixture();
    let source = temp.path().join("source");
    native(&source, &["rm", "-q", "file.txt"]);
    let cancel = Cancellation::default();
    let target = discover(&git, &source, &cancel);
    let destination = temp.path().join("work");
    let created = git
        .create_worktree(
            "user",
            &target,
            &WorktreeRequest {
                managed_root: temp.path().into(),
                destination: destination.clone(),
                commit: target.head.clone().unwrap(),
                include_paths: vec!["file.txt".into()],
            },
            &cancel,
        )
        .unwrap();
    assert!(created.preparation_error.is_none());
    assert!(!destination.join("file.txt").exists());
    assert_eq!(
        native(&destination, &["status", "--porcelain=v1"]),
        "D  file.txt"
    );
}

#[test]
fn the_same_archive_can_restore_into_multiple_new_destinations() {
    let (temp, git) = fixture();
    let source = temp.path().join("source");
    let cancel = Cancellation::default();
    let target = discover(&git, &source, &cancel);
    let archives = ArchiveDirectory::open(temp.path().join("archives")).unwrap();
    archives
        .capture(&git, "user", "repeat", &target, &cancel)
        .unwrap();
    for name in ["first-restore", "second-restore"] {
        let destination = temp.path().join(name);
        let result = archives
            .restore(
                &git,
                "user",
                &target,
                &ArchiveRestoreRequest {
                    archive_id: "repeat".into(),
                    managed_root: temp.path().into(),
                    destination: destination.clone(),
                },
                &cancel,
            )
            .unwrap();
        assert!(result.restore_verified);
        let canonical_destination = destination.canonicalize().unwrap();
        assert_eq!(
            result.restored_target.unwrap().root.as_deref(),
            canonical_destination.to_str()
        );
        assert_eq!(
            fs::read_to_string(destination.join("file.txt")).unwrap(),
            "base\n"
        );
    }
    let listed = archives.list(&git, "user", &target, &cancel).unwrap();
    assert_eq!(listed.archives.len(), 1);
    assert!(listed.archives[0].restore_verified);
}

#[test]
fn an_owned_archive_can_restore_into_an_explicitly_authorized_other_repository() {
    let (temp, git) = fixture();
    let source = temp.path().join("source");
    let cancel = Cancellation::default();
    let target = discover(&git, &source, &cancel);
    let archives = ArchiveDirectory::open(temp.path().join("archives")).unwrap();
    archives
        .capture(&git, "user", "portable", &target, &cancel)
        .unwrap();
    let other = temp.path().join("other");
    fs::create_dir(&other).unwrap();
    native(&other, &["init", "-q"]);
    native(&other, &["config", "user.name", "Test"]);
    native(&other, &["config", "user.email", "test@example.invalid"]);
    fs::write(other.join("other.txt"), "other history\n").unwrap();
    native(&other, &["add", "."]);
    native(&other, &["commit", "-qm", "feat: separate repository"]);
    let other_target = discover(&git, &other, &cancel);
    let destination = temp.path().join("portable-restore");
    let result = archives
        .restore(
            &git,
            "user",
            &other_target,
            &ArchiveRestoreRequest {
                archive_id: "portable".into(),
                managed_root: temp.path().into(),
                destination: destination.clone(),
            },
            &cancel,
        )
        .unwrap();
    assert!(result.restore_verified);
    assert_eq!(
        result.restored_target.unwrap().common_dir,
        other_target.common_dir
    );
    assert_eq!(
        native(&destination, &["rev-parse", "HEAD"]),
        target.head.unwrap()
    );
    assert_eq!(
        native(&other, &["rev-parse", "HEAD"]),
        other_target.head.unwrap()
    );
    assert!(destination.join("file.txt").is_file());
    assert!(!destination.join("other.txt").exists());
}

#[test]
fn an_incomplete_restoration_proof_does_not_mark_an_archive_verified() {
    let (temp, git) = fixture();
    let source = temp.path().join("source");
    let cancel = Cancellation::default();
    let target = discover(&git, &source, &cancel);
    let archive_root = temp.path().join("archives");
    let archives = ArchiveDirectory::open(&archive_root).unwrap();
    archives
        .capture(&git, "user", "interrupted-proof", &target, &cancel)
        .unwrap();
    let entry = fs::read_dir(&archive_root)
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    fs::write(
        entry.path().join("verified-restoration-incomplete.json"),
        "{\"restore_verified\":true",
    )
    .unwrap();
    let listed = archives.list(&git, "user", &target, &cancel).unwrap();
    assert_eq!(listed.archives.len(), 1);
    assert!(!listed.archives[0].restore_verified);
}

#[test]
fn clean_worktree_creation_does_not_require_the_transfer_index_profile() {
    let (temp, git) = fixture();
    let source = temp.path().join("source");
    native(&source, &["update-index", "--skip-worktree", "file.txt"]);
    let cancel = Cancellation::default();
    let target = discover(&git, &source, &cancel);
    let created = git
        .create_worktree(
            "user",
            &target,
            &WorktreeRequest {
                managed_root: temp.path().into(),
                destination: temp.path().join("clean"),
                commit: target.head.clone().unwrap(),
                include_paths: vec![],
            },
            &cancel,
        )
        .unwrap();
    assert!(created.preparation_error.is_none());
}

#[cfg(unix)]
#[test]
fn archive_and_selected_transfer_preserve_private_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let (temp, git) = fixture();
    let source = temp.path().join("source");
    fs::create_dir(source.join("private")).unwrap();
    fs::set_permissions(source.join("private"), fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(source.join("private/secret"), "sensitive").unwrap();
    fs::set_permissions(
        source.join("private/secret"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let cancel = Cancellation::default();
    let target = discover(&git, &source, &cancel);
    let transferred = temp.path().join("transferred");
    let created = git
        .create_worktree(
            "user",
            &target,
            &WorktreeRequest {
                managed_root: temp.path().into(),
                destination: transferred.clone(),
                commit: target.head.clone().unwrap(),
                include_paths: vec!["private/secret".into()],
            },
            &cancel,
        )
        .unwrap();
    assert!(created.preparation_error.is_none());
    let archives = ArchiveDirectory::open(temp.path().join("archives")).unwrap();
    archives
        .capture(&git, "user", "private-permissions", &target, &cancel)
        .unwrap();
    let restored = temp.path().join("restored-private");
    archives
        .restore(
            &git,
            "user",
            &target,
            &ArchiveRestoreRequest {
                archive_id: "private-permissions".into(),
                managed_root: temp.path().into(),
                destination: restored.clone(),
            },
            &cancel,
        )
        .unwrap();
    for root in [transferred, restored] {
        assert_eq!(
            fs::metadata(root.join("private"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(root.join("private/secret"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn damaged_archive_metadata_is_reported_without_hiding_valid_archives() {
    let (temp, git) = fixture();
    let source = temp.path().join("source");
    let cancel = Cancellation::default();
    let target = discover(&git, &source, &cancel);
    let root = temp.path().join("archives");
    let archives = ArchiveDirectory::open(&root).unwrap();
    archives
        .capture(&git, "user", "valid", &target, &cancel)
        .unwrap();
    let damaged = root.join("interrupted-capture");
    fs::create_dir(&damaged).unwrap();
    fs::write(damaged.join("manifest.json"), "{\"principal\":").unwrap();
    let listed = archives.list(&git, "user", &target, &cancel).unwrap();
    assert_eq!(listed.archives.len(), 1);
    assert_eq!(listed.archives[0].archive_id, "valid");
    assert_eq!(listed.unavailable.len(), 1);
    assert_eq!(listed.unavailable[0].entry_id, "interrupted-capture");
    assert!(damaged.join("manifest.json").exists());
}

#[cfg(unix)]
#[test]
fn preservation_rejects_special_permissions_and_tracks_mode_changes() {
    use std::os::unix::fs::PermissionsExt;
    let (temp, git) = fixture();
    let source = temp.path().join("source");
    fs::write(source.join("private"), "secret").unwrap();
    fs::set_permissions(source.join("private"), fs::Permissions::from_mode(0o600)).unwrap();
    let cancel = Cancellation::default();
    let target = discover(&git, &source, &cancel);
    let archives = ArchiveDirectory::open(temp.path().join("archives")).unwrap();
    archives
        .capture(&git, "user", "mode-test", &target, &cancel)
        .unwrap();
    fs::set_permissions(source.join("private"), fs::Permissions::from_mode(0o644)).unwrap();
    let eligibility = archives
        .cleanup_eligibility(&git, "user", "mode-test", &target, &cancel)
        .unwrap();
    assert!(eligibility.changed_since_archive);
    assert!(!eligibility.eligible);
    fs::set_permissions(source.join("private"), fs::Permissions::from_mode(0o4600)).unwrap();
    let target = discover(&git, &source, &cancel);
    let error = archives
        .capture(&git, "user", "unsupported-mode", &target, &cancel)
        .unwrap_err();
    assert!(error.to_string().contains("special permission"));
}

#[test]
fn archive_manifest_has_bounded_metadata_and_raw_deduplicated_content() {
    let (temp, git) = fixture();
    let source = temp.path().join("source");
    let content = vec![173; 1024 * 1024];
    fs::write(source.join("large-a"), &content).unwrap();
    fs::write(source.join("large-b"), &content).unwrap();
    let cancel = Cancellation::default();
    let target = discover(&git, &source, &cancel);
    let root = temp.path().join("archives");
    let archives = ArchiveDirectory::open(&root).unwrap();
    archives
        .capture(&git, "user", "raw-content", &target, &cancel)
        .unwrap();
    let archive = fs::read_dir(&root).unwrap().next().unwrap().unwrap().path();
    assert!(fs::metadata(archive.join("manifest.json")).unwrap().len() < 16 * 1024);
    let large_blobs = fs::read_dir(archive.join("blobs"))
        .unwrap()
        .filter_map(|entry| {
            let entry = entry.unwrap();
            (entry.metadata().unwrap().len() == content.len() as u64).then_some(entry.path())
        })
        .collect::<Vec<_>>();
    assert_eq!(large_blobs.len(), 1);
    assert_eq!(fs::read(&large_blobs[0]).unwrap(), content);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&large_blobs[0]).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn clean_worktree_creation_accepts_a_gitlink_without_transferring_it() {
    let (temp, git) = fixture();
    let source = temp.path().join("source");
    let head = native(&source, &["rev-parse", "HEAD"]);
    native(
        &source,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            "160000",
            &head,
            "submodule",
        ],
    );
    native(&source, &["commit", "-qm", "feat: add gitlink fixture"]);
    let cancel = Cancellation::default();
    let target = discover(&git, &source, &cancel);
    let created = git
        .create_worktree(
            "user",
            &target,
            &WorktreeRequest {
                managed_root: temp.path().into(),
                destination: temp.path().join("with-gitlink"),
                commit: target.head.clone().unwrap(),
                include_paths: vec![],
            },
            &cancel,
        )
        .unwrap();
    assert!(created.preparation_error.is_none());
}

#[test]
fn corrupted_content_is_refused_before_restore_creates_a_checkout() {
    let (temp, git) = fixture();
    let source = temp.path().join("source");
    let cancel = Cancellation::default();
    let target = discover(&git, &source, &cancel);
    let root = temp.path().join("archives");
    let archives = ArchiveDirectory::open(&root).unwrap();
    archives
        .capture(&git, "user", "corrupt", &target, &cancel)
        .unwrap();
    let archive = fs::read_dir(&root).unwrap().next().unwrap().unwrap().path();
    let blob = fs::read_dir(archive.join("blobs"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::write(&blob, "damaged").unwrap();
    let destination = temp.path().join("refused-restore");
    let error = archives
        .restore(
            &git,
            "user",
            &target,
            &ArchiveRestoreRequest {
                archive_id: "corrupt".into(),
                managed_root: temp.path().into(),
                destination: destination.clone(),
            },
            &cancel,
        )
        .unwrap_err();
    assert!(matches!(error, swarmie_git::Error::NotStarted(_)));
    assert!(!destination.exists());
    assert!(blob.exists());
}

#[cfg(target_os = "macos")]
#[test]
fn darwin_extended_acl_is_refused_without_claiming_preservation() {
    let (temp, git) = fixture();
    let source = temp.path().join("source");
    fs::write(source.join("acl-content"), "protected").unwrap();
    let result = Command::new("chmod")
        .arg("+a")
        .arg("everyone allow read")
        .arg(source.join("acl-content"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let cancel = Cancellation::default();
    let target = discover(&git, &source, &cancel);
    let archives = ArchiveDirectory::open(temp.path().join("archives")).unwrap();
    let error = archives
        .capture(&git, "user", "acl", &target, &cancel)
        .unwrap_err();
    assert!(error.to_string().contains("ACL"), "{error}");
    assert!(source.join("acl-content").is_file());
}

#[cfg(target_os = "linux")]
#[test]
fn extended_attributes_are_refused_instead_of_silently_discarded() {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let (temp, git) = fixture();
    let source = temp.path().join("source");
    let private = source.join("metadata");
    fs::write(&private, "with metadata").unwrap();
    let path = CString::new(private.as_os_str().as_bytes()).unwrap();
    let value = b"retained";
    let result = unsafe {
        libc::setxattr(
            path.as_ptr(),
            c"user.swarmie-test".as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
        )
    };
    assert_eq!(result, 0, "{}", std::io::Error::last_os_error());
    let cancel = Cancellation::default();
    let target = discover(&git, &source, &cancel);
    let archives = ArchiveDirectory::open(temp.path().join("archives")).unwrap();
    let error = archives
        .capture(&git, "user", "metadata", &target, &cancel)
        .unwrap_err();
    assert!(error.to_string().contains("extended attributes"), "{error}");
    assert_eq!(fs::read_to_string(private).unwrap(), "with metadata");
}
