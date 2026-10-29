use std::{fs, path::Path, process::Command, sync::Arc};
use swarmie_git::{
    ArchiveDirectory, ArchiveRestoreRequest, Authority, Cancellation, DirectoryJournal, Git,
    Intent, NativeProcess, Result, SetupScript, WorktreeRequest,
};
use tempfile::TempDir;

struct Allow;
impl Authority for Allow {
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
    let target = git.discover(&source, &cancel).unwrap().unwrap();
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
    let target = git.discover(&source, &cancel).unwrap().unwrap();
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
    let target = git.discover(&source, &cancel).unwrap().unwrap();
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
    let target = git.discover(&source, &cancel).unwrap().unwrap();
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
    let target = git.discover(&source, &cancel).unwrap().unwrap();
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
    let target = git.discover(&source, &cancel).unwrap().unwrap();
    let archives = ArchiveDirectory::open(temp.path().join("archives")).unwrap();
    archives
        .capture(&git, "alice", "private", &target, &cancel)
        .unwrap();
    assert_eq!(
        archives
            .list(&git, "alice", &target, &cancel)
            .unwrap()
            .len(),
        1
    );
    assert!(
        archives
            .list(&git, "bob", &target, &cancel)
            .unwrap()
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
    let target = git.discover(&source, &cancel).unwrap().unwrap();
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
    let target = git.discover(&source, &cancel).unwrap().unwrap();
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
    assert_eq!(listed.len(), 1);
    assert!(listed[0].restore_verified);
}

#[test]
fn an_owned_archive_can_restore_into_an_explicitly_authorized_other_repository() {
    let (temp, git) = fixture();
    let source = temp.path().join("source");
    let cancel = Cancellation::default();
    let target = git.discover(&source, &cancel).unwrap().unwrap();
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
    let other_target = git.discover(&other, &cancel).unwrap().unwrap();
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
    let target = git.discover(&source, &cancel).unwrap().unwrap();
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
    assert_eq!(listed.len(), 1);
    assert!(!listed[0].restore_verified);
}
