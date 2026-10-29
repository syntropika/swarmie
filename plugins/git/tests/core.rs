use std::{fs, path::Path, process::Command, sync::Arc};

use swarmie_git::{
    Authority, Cancellation, DirectoryJournal, Error, Git, Intent, NativeProcess, Operation,
    Outcome, PendingOperation, Request, Response, Result,
};
use tempfile::TempDir;

struct Permit;

impl Authority for Permit {
    fn authorize(&self, _: &str, _: &Intent) -> Result<()> {
        Ok(())
    }
    fn authorize_discovery(&self, _: &str, _: &Path) -> Result<()> {
        Ok(())
    }
}

fn command(path: &Path, args: &[&str]) -> String {
    let result = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .expect("run Git");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout)
        .expect("UTF-8 Git output")
        .trim()
        .into()
}

fn fixture() -> (TempDir, Git) {
    let directory = tempfile::tempdir().expect("temporary directory");
    command(directory.path(), &["init", "-b", "main"]);
    command(directory.path(), &["config", "user.name", "Test User"]);
    command(
        directory.path(),
        &["config", "user.email", "test@example.invalid"],
    );
    fs::write(directory.path().join("first.txt"), "first\n").expect("write file");
    command(directory.path(), &["add", "first.txt"]);
    command(directory.path(), &["commit", "-m", "feat: initial"]);
    let journal =
        DirectoryJournal::open(directory.path().join(".git/swarmie-journal")).expect("journal");
    let git = Git::new(Arc::new(NativeProcess), Arc::new(journal), Arc::new(Permit));
    (directory, git)
}

fn mutate(git: &Git, path: &Path, id: &str, operation: Operation) -> swarmie_git::Receipt {
    let target = git
        .discover(path, &Cancellation::default())
        .expect("discover")
        .expect("repository");
    match git
        .execute(
            "user",
            Request::Mutate {
                operation_id: id.into(),
                target,
                operation,
            },
            &Cancellation::default(),
        )
        .expect("mutation")
    {
        Response::Receipt { receipt } => *receipt,
        _ => panic!("expected receipt"),
    }
}

#[test]
fn discovery_status_and_path_selection_preserve_other_changes() {
    let (directory, git) = fixture();
    fs::create_dir(directory.path().join("nested")).expect("nested");
    let target = git
        .discover(&directory.path().join("nested"), &Cancellation::default())
        .expect("discover")
        .expect("repository");
    let canonical_root = directory.path().canonicalize().expect("canonical root");
    assert_eq!(target.root.as_deref(), canonical_root.to_str());
    fs::write(directory.path().join("first.txt"), "edited\n").expect("edit");
    fs::write(directory.path().join("space name\nfile.txt"), "new\n").expect("new");
    let receipt = mutate(
        &git,
        directory.path(),
        "stage-one",
        Operation::Stage {
            paths: vec!["space name\nfile.txt".into()],
        },
    );
    assert_eq!(receipt.state, Outcome::Completed);
    assert_eq!(
        command(directory.path(), &["diff", "--cached", "--name-only", "-z"]),
        "space name\nfile.txt\0"
    );
    assert!(command(directory.path(), &["diff", "--name-only"]).contains("first.txt"));
    let target = git
        .discover(directory.path(), &Cancellation::default())
        .expect("discover")
        .expect("repo");
    match git
        .execute(
            "user",
            Request::Status { target, page: None },
            &Cancellation::default(),
        )
        .expect("status")
    {
        Response::Status { status, next_page } => {
            assert_eq!(status.entries.len(), 2);
            assert!(next_page.is_none());
        }
        _ => panic!("expected status"),
    }
}

#[test]
fn stale_content_cannot_be_staged() {
    let (directory, git) = fixture();
    fs::write(directory.path().join("first.txt"), "one\n").expect("edit");
    let target = git
        .discover(directory.path(), &Cancellation::default())
        .expect("discover")
        .expect("repo");
    fs::write(directory.path().join("first.txt"), "two\n").expect("edit");
    let outcome = git.execute(
        "user",
        Request::Mutate {
            operation_id: "stale".into(),
            target,
            operation: Operation::Stage {
                paths: vec!["first.txt".into()],
            },
        },
        &Cancellation::default(),
    );
    assert!(
        matches!(outcome, Ok(Response::Receipt { receipt }) if receipt.state == Outcome::Failed)
    );
    assert!(command(directory.path(), &["diff", "--cached", "--name-only"]).is_empty());
}

#[test]
fn reviewed_index_commits_all_and_duplicate_id_never_replays() {
    let (directory, git) = fixture();
    fs::write(directory.path().join("first.txt"), "staged\n").expect("edit");
    mutate(
        &git,
        directory.path(),
        "stage",
        Operation::Stage {
            paths: vec!["first.txt".into()],
        },
    );
    fs::write(directory.path().join("first.txt"), "unstaged\n").expect("edit");
    let target = git
        .discover(directory.path(), &Cancellation::default())
        .expect("discover")
        .expect("repo");
    let request = Request::Mutate {
        operation_id: "commit".into(),
        target: target.clone(),
        operation: Operation::Commit {
            message: "feat: reviewed staged state".into(),
            reviewed_index: target.index_fingerprint.clone(),
        },
    };
    let first = git
        .execute("user", request.clone(), &Cancellation::default())
        .expect("commit");
    let second = git
        .execute("user", request, &Cancellation::default())
        .expect("duplicate");
    match (first, second) {
        (Response::Receipt { receipt: left }, Response::Receipt { receipt: right }) => {
            assert_eq!(left.head, right.head)
        }
        _ => panic!("receipts"),
    }
    assert_eq!(
        command(directory.path(), &["show", "HEAD:first.txt"]),
        "staged"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("first.txt")).expect("read"),
        "unstaged\n"
    );
    assert_eq!(
        command(directory.path(), &["rev-list", "--count", "HEAD"]),
        "2"
    );
}

#[test]
fn pending_admission_is_unknown_and_private_to_principal() {
    let (directory, git) = fixture();
    let target = git
        .discover(directory.path(), &Cancellation::default())
        .expect("discover")
        .expect("repo");
    git.journal
        .admit(&PendingOperation {
            operation_id: "interrupted".into(),
            principal: "user".into(),
            target,
            operation: Operation::Stage {
                paths: vec!["first.txt".into()],
            },
        })
        .expect("admit");
    match git
        .execute(
            "user",
            Request::Recover {
                operation_id: "interrupted".into(),
            },
            &Cancellation::default(),
        )
        .expect("recover")
    {
        Response::Receipt { receipt } => assert_eq!(receipt.state, Outcome::Unknown),
        _ => panic!("receipt"),
    }
    assert!(matches!(
        git.execute(
            "other",
            Request::Recover {
                operation_id: "interrupted".into()
            },
            &Cancellation::default()
        ),
        Err(Error::Denied(_))
    ));
}

#[test]
fn bare_discovery_and_explicit_remote_push_are_supported() {
    let (directory, git) = fixture();
    let bare = tempfile::tempdir().expect("bare path");
    command(bare.path(), &["init", "--bare"]);
    let target = git
        .discover(bare.path(), &Cancellation::default())
        .expect("discover")
        .expect("bare");
    assert!(target.bare);
    assert!(target.root.is_none());
    command(
        directory.path(),
        &[
            "remote",
            "add",
            "review",
            bare.path().to_str().expect("path"),
        ],
    );
    let target = git
        .discover(directory.path(), &Cancellation::default())
        .expect("discover")
        .expect("repo");
    let head = target.head.clone().expect("HEAD");
    let receipt = mutate(
        &git,
        directory.path(),
        "push",
        Operation::Push {
            remote: "review".into(),
            source_oid: head.clone(),
            destination_ref: "refs/heads/review".into(),
        },
    );
    assert_eq!(receipt.state, Outcome::Completed);
    assert_eq!(
        command(bare.path(), &["rev-parse", "refs/heads/review"]),
        head
    );
}

#[test]
fn deny_discovery_before_filesystem_access() {
    struct Deny;
    impl Authority for Deny {
        fn authorize(&self, _: &str, _: &Intent) -> Result<()> {
            Err(Error::Denied("denied".into()))
        }
    }
    let directory = tempfile::tempdir().expect("temp");
    let git = Git::new(
        Arc::new(NativeProcess),
        Arc::new(DirectoryJournal::open(directory.path()).expect("journal")),
        Arc::new(Deny),
    );
    assert!(matches!(
        git.execute(
            "user",
            Request::Discover {
                path: "/path/which/does/not/exist".into()
            },
            &Cancellation::default()
        ),
        Err(Error::Denied(_))
    ));
}

#[cfg(unix)]
#[test]
fn native_process_cancellation_settles_descendants_and_output_is_bounded() {
    use std::{
        collections::BTreeMap,
        thread,
        time::{Duration, Instant},
    };
    use swarmie_git::{CommandSpec, ProcessExecutor};
    let directory = tempfile::tempdir().expect("temp");
    let cancel = Cancellation::default();
    let interrupter = cancel.clone();
    let started = Instant::now();
    let thread = thread::spawn(move || {
        thread::sleep(Duration::from_millis(40));
        interrupter.cancel();
    });
    let outcome = NativeProcess.run(
        &CommandSpec {
            program: "sh".into(),
            cwd: directory.path().into(),
            args: vec!["-c".into(), "sleep 30 & wait".into()],
            env: BTreeMap::new(),
            timeout: Duration::from_secs(2),
            output_limit: 128,
        },
        &cancel,
    );
    thread.join().expect("cancel thread");
    assert!(matches!(outcome, Err(Error::Cancelled)));
    assert!(started.elapsed() < Duration::from_secs(2));
    let output = NativeProcess
        .run(
            &CommandSpec {
                program: "sh".into(),
                cwd: directory.path().into(),
                args: vec![
                    "-c".into(),
                    "i=0; while [ $i -lt 1000 ]; do printf 'abcdefghij'; i=$((i+1)); done".into(),
                ],
                env: BTreeMap::new(),
                timeout: Duration::from_secs(2),
                output_limit: 128,
            },
            &Cancellation::default(),
        )
        .expect("bounded output");
    assert!(output.truncated);
    assert_eq!(output.stdout.len(), 128);
}

#[test]
fn private_index_preserves_native_filters_and_staged_deletions() {
    let (directory, git) = fixture();
    fs::write(
        directory.path().join(".gitattributes"),
        "*.txt filter=uppercase\n",
    )
    .expect("attributes");
    command(
        directory.path(),
        &["config", "filter.uppercase.clean", "tr a-z A-Z"],
    );
    fs::write(directory.path().join("first.txt"), "lowercase\n").expect("edit");
    let receipt = mutate(
        &git,
        directory.path(),
        "filter",
        Operation::Stage {
            paths: vec!["first.txt".into()],
        },
    );
    assert_eq!(receipt.state, Outcome::Completed, "{}", receipt.message);
    assert_eq!(
        command(directory.path(), &["show", ":first.txt"]),
        "LOWERCASE"
    );
    fs::remove_file(directory.path().join("first.txt")).expect("delete working file");
    let receipt = mutate(
        &git,
        directory.path(),
        "delete",
        Operation::Stage {
            paths: vec!["first.txt".into()],
        },
    );
    assert_eq!(receipt.state, Outcome::Completed, "{}", receipt.message);
    assert!(command(directory.path(), &["ls-files", "first.txt"]).is_empty());
    assert!(!directory.path().join(".git/index.lock").exists());
}

#[cfg(unix)]
#[test]
fn native_commit_hooks_are_preserved_and_report_tree_changes() {
    use std::os::unix::fs::PermissionsExt;
    let (directory, git) = fixture();
    fs::write(directory.path().join("first.txt"), "reviewed\n").expect("edit");
    mutate(
        &git,
        directory.path(),
        "stage-hook",
        Operation::Stage {
            paths: vec!["first.txt".into()],
        },
    );
    let hook = directory.path().join(".git/hooks/pre-commit");
    fs::write(
        &hook,
        "#!/bin/sh\nprintf 'hook change\\n' > first.txt\ngit add -- first.txt\n",
    )
    .expect("hook");
    fs::set_permissions(hook, fs::Permissions::from_mode(0o755)).expect("executable hook");
    let target = git
        .discover(directory.path(), &Cancellation::default())
        .expect("discover")
        .expect("repo");
    let receipt = mutate(
        &git,
        directory.path(),
        "commit-hook",
        Operation::Commit {
            message: "feat: hook".into(),
            reviewed_index: target.index_fingerprint,
        },
    );
    assert_eq!(receipt.state, Outcome::Completed);
    assert!(
        receipt
            .message
            .contains("Native hooks changed the committed tree")
    );
    assert_eq!(
        command(directory.path(), &["show", "HEAD:first.txt"]),
        "hook change"
    );
    assert_eq!(
        command(directory.path(), &["show", ":first.txt"]),
        "hook change"
    );
    assert!(command(directory.path(), &["status", "--porcelain"]).is_empty());
    fs::remove_file(directory.path().join(".git/hooks/pre-commit")).expect("remove hook");
    let target = git
        .discover(directory.path(), &Cancellation::default())
        .unwrap()
        .unwrap();
    let receipt = mutate(
        &git,
        directory.path(),
        "commit-after-hook",
        Operation::Commit {
            message: "feat: subsequent commit".into(),
            reviewed_index: target.index_fingerprint,
        },
    );
    assert_ne!(receipt.state, Outcome::Completed);
    assert_eq!(
        command(directory.path(), &["show", "HEAD:first.txt"]),
        "hook change"
    );
}

#[test]
fn staging_uses_reviewed_snapshot_when_editor_changes_file_at_dispatch() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use swarmie_git::{CommandOutput, CommandSpec, ProcessExecutor};
    struct EditingExecutor {
        root: std::path::PathBuf,
        edited: AtomicBool,
    }
    impl ProcessExecutor for EditingExecutor {
        fn run(&self, command: &CommandSpec, cancel: &Cancellation) -> Result<CommandOutput> {
            if command.env.contains_key("GIT_WORK_TREE")
                && command.args.iter().any(|arg| arg == "add")
                && !self.edited.swap(true, Ordering::SeqCst)
            {
                fs::write(self.root.join("first.txt"), "concurrent edit\n")?;
            }
            NativeProcess.run(command, cancel)
        }
    }
    let (directory, mut git) = fixture();
    fs::write(directory.path().join("first.txt"), "reviewed edit\n").expect("edit");
    git.executor = Arc::new(EditingExecutor {
        root: directory.path().into(),
        edited: AtomicBool::new(false),
    });
    let receipt = mutate(
        &git,
        directory.path(),
        "snapshot-race",
        Operation::Stage {
            paths: vec!["first.txt".into()],
        },
    );
    assert_eq!(receipt.state, Outcome::Completed, "{}", receipt.message);
    assert_eq!(
        command(directory.path(), &["show", ":first.txt"]),
        "reviewed edit"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("first.txt")).expect("working content"),
        "concurrent edit\n"
    );
}

#[test]
fn a_foreign_index_lock_is_preserved() {
    let (directory, git) = fixture();
    fs::write(directory.path().join("first.txt"), "reviewed\n").expect("edit");
    let lock = directory.path().join(".git/index.lock");
    fs::write(&lock, "another Git writer").expect("foreign lock");
    let receipt = mutate(
        &git,
        directory.path(),
        "locked",
        Operation::Stage {
            paths: vec!["first.txt".into()],
        },
    );
    assert_eq!(receipt.state, Outcome::Unknown);
    assert_eq!(
        fs::read_to_string(lock).expect("foreign lock intact"),
        "another Git writer"
    );
    assert!(command(directory.path(), &["diff", "--cached", "--name-only"]).is_empty());
}

#[test]
fn unborn_unstage_preserves_later_working_edit() {
    let directory = tempfile::tempdir().expect("temp");
    command(directory.path(), &["init", "-b", "main"]);
    let git = Git::new(
        Arc::new(NativeProcess),
        Arc::new(DirectoryJournal::open(directory.path().join(".git/swarmie")).expect("journal")),
        Arc::new(Permit),
    );
    fs::write(directory.path().join("new.txt"), "first\n").expect("write");
    let staged = mutate(
        &git,
        directory.path(),
        "unborn-stage",
        Operation::Stage {
            paths: vec!["new.txt".into()],
        },
    );
    assert_eq!(staged.state, Outcome::Completed, "{}", staged.message);
    fs::write(directory.path().join("new.txt"), "later\n").expect("edit");
    let unstaged = mutate(
        &git,
        directory.path(),
        "unborn-unstage",
        Operation::Unstage {
            paths: vec!["new.txt".into()],
        },
    );
    assert_eq!(unstaged.state, Outcome::Completed, "{}", unstaged.message);
    assert!(command(directory.path(), &["ls-files"]).is_empty());
    assert_eq!(
        fs::read_to_string(directory.path().join("new.txt")).expect("content"),
        "later\n"
    );
}

#[test]
fn untracked_files_have_a_diff_and_metadata_paths_are_refused() {
    use swarmie_git::DiffMode;
    let (directory, git) = fixture();
    fs::write(directory.path().join("new.txt"), "new line\n").expect("new");
    let target = git
        .discover(directory.path(), &Cancellation::default())
        .expect("discover")
        .expect("repo");
    match git
        .execute(
            "user",
            Request::Diff {
                target: target.clone(),
                path: Some("new.txt".into()),
                staged: false,
                mode: DiffMode::Unified,
            },
            &Cancellation::default(),
        )
        .expect("diff")
    {
        Response::Diff { diff } => assert!(diff.text.contains("+new line")),
        _ => panic!("diff"),
    }
    let rejected = git
        .execute(
            "user",
            Request::Mutate {
                operation_id: "metadata".into(),
                target,
                operation: Operation::Stage {
                    paths: vec![".Git/config".into()],
                },
            },
            &Cancellation::default(),
        )
        .expect("authoritative rejection");
    assert!(matches!(rejected, Response::Receipt {receipt} if receipt.state == Outcome::Failed));
}

#[test]
fn split_index_staging_and_commit_preserve_working_and_unrelated_staged_content() {
    let (directory, git) = fixture();
    fs::write(directory.path().join("second.txt"), "second\n").expect("second");
    command(directory.path(), &["add", "second.txt"]);
    command(directory.path(), &["commit", "-m", "feat: second file"]);
    command(directory.path(), &["update-index", "--split-index"]);
    assert!(!command(directory.path(), &["rev-parse", "--shared-index-path"]).is_empty());
    fs::write(directory.path().join("second.txt"), "unrelated staged\n").expect("second staged");
    command(directory.path(), &["add", "second.txt"]);
    fs::write(directory.path().join("first.txt"), "reviewed first\n").expect("first");
    let stage = mutate(
        &git,
        directory.path(),
        "split-stage",
        Operation::Stage {
            paths: vec!["first.txt".into()],
        },
    );
    assert_eq!(stage.state, Outcome::Completed, "{}", stage.message);
    assert_eq!(
        command(directory.path(), &["show", ":second.txt"]),
        "unrelated staged"
    );
    assert_eq!(
        command(directory.path(), &["show", ":first.txt"]),
        "reviewed first"
    );
    fs::write(directory.path().join("first.txt"), "later first\n").expect("later working edit");
    fs::write(directory.path().join("second.txt"), "later second\n").expect("later other edit");
    let target = git
        .discover(directory.path(), &Cancellation::default())
        .expect("discover")
        .expect("repo");
    let commit = mutate(
        &git,
        directory.path(),
        "split-commit",
        Operation::Commit {
            message: "feat: reviewed split index".into(),
            reviewed_index: target.index_fingerprint,
        },
    );
    assert_eq!(commit.state, Outcome::Completed, "{}", commit.message);
    assert_eq!(
        command(directory.path(), &["show", "HEAD:first.txt"]),
        "reviewed first"
    );
    assert_eq!(
        command(directory.path(), &["show", "HEAD:second.txt"]),
        "unrelated staged"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("first.txt")).expect("first working"),
        "later first\n"
    );
    assert_eq!(
        fs::read_to_string(directory.path().join("second.txt")).expect("second working"),
        "later second\n"
    );
    assert!(command(directory.path(), &["diff", "--cached", "--name-only"]).is_empty());
    assert!(!command(directory.path(), &["rev-parse", "--shared-index-path"]).is_empty());
    assert!(
        !fs::read_dir(directory.path().join(".git"))
            .expect("Git directory")
            .any(|entry| entry
                .expect("directory entry")
                .file_name()
                .to_string_lossy()
                .starts_with("swarmie-reviewed-"))
    );
}

#[test]
fn invalid_native_branch_name_is_a_recoverable_confirmed_rejection() {
    let (directory, git) = fixture();
    let target = git
        .discover(directory.path(), &Cancellation::default())
        .expect("discover")
        .expect("repo");
    let original_head = target.head.clone();
    let response = git
        .execute(
            "user",
            Request::Mutate {
                operation_id: "invalid-native-branch".into(),
                target,
                operation: Operation::CreateBranch {
                    name: "invalid..branch".into(),
                },
            },
            &Cancellation::default(),
        )
        .expect("confirmed native validation rejection");
    let Response::Receipt { receipt } = response else {
        panic!("receipt");
    };
    assert_eq!(receipt.state, Outcome::Failed);
    assert!(
        receipt
            .message
            .contains("Request rejected before execution")
    );
    let Response::Receipt { receipt: recovered } = git
        .execute(
            "user",
            Request::Recover {
                operation_id: "invalid-native-branch".into(),
            },
            &Cancellation::default(),
        )
        .expect("recover rejection")
    else {
        panic!("receipt");
    };
    assert_eq!(recovered.state, Outcome::Failed);
    assert_eq!(recovered.message, receipt.message);
    assert_eq!(recovered.head, original_head);
    assert_eq!(
        command(directory.path(), &["symbolic-ref", "--short", "HEAD"]),
        "main"
    );
    assert_eq!(
        command(
            directory.path(),
            &["for-each-ref", "--format=%(refname:short)", "refs/heads"]
        ),
        "main"
    );
}

#[test]
fn rejection_helper_never_replaces_an_existing_admission() {
    use swarmie_git::JournalEntry;
    let (directory, git) = fixture();
    let target = git
        .discover(directory.path(), &Cancellation::default())
        .expect("discover")
        .expect("repo");
    let operation = Operation::Stage {
        paths: vec!["first.txt".into()],
    };
    git.journal
        .admit(&PendingOperation {
            operation_id: "existing-admission".into(),
            principal: "user".into(),
            target: target.clone(),
            operation: operation.clone(),
        })
        .expect("admit");
    let recovered = git
        .reject_before_admission(
            "user",
            "existing-admission".into(),
            target,
            operation,
            Error::Io(std::io::Error::other("unresolved settlement")),
        )
        .expect("recover existing admission");
    assert_eq!(recovered.state, Outcome::Unknown);
    assert!(matches!(
        git.journal.load("existing-admission").expect("load"),
        Some(JournalEntry::Pending(_))
    ));
}

#[test]
fn cancellation_before_admission_has_a_failed_receipt_without_effect() {
    let (directory, git) = fixture();
    fs::write(directory.path().join("first.txt"), "working edit\n").expect("edit");
    let target = git
        .discover(directory.path(), &Cancellation::default())
        .expect("discover")
        .expect("repo");
    let cancel = Cancellation::default();
    cancel.cancel();
    let Response::Receipt { receipt } = git
        .execute(
            "user",
            Request::Mutate {
                operation_id: "cancel-before-dispatch".into(),
                target,
                operation: Operation::Stage {
                    paths: vec!["first.txt".into()],
                },
            },
            &cancel,
        )
        .expect("confirmed rejection")
    else {
        panic!("receipt");
    };
    assert_eq!(receipt.state, Outcome::Failed);
    assert!(command(directory.path(), &["diff", "--cached", "--name-only"]).is_empty());
    let Response::Receipt { receipt: recovered } = git
        .execute(
            "user",
            Request::Recover {
                operation_id: "cancel-before-dispatch".into(),
            },
            &Cancellation::default(),
        )
        .expect("recover")
    else {
        panic!("receipt");
    };
    assert_eq!(recovered.state, Outcome::Failed);
}

#[test]
fn large_inventories_are_complete_bounded_and_reject_changed_continuations() {
    let (directory, git) = fixture();
    let folder = directory.path().join("inventory");
    fs::create_dir(&folder).unwrap();
    for index in 0..1400 {
        fs::write(
            folder.join(format!("{index:04}-{}\".txt", "long".repeat(15))),
            "",
        )
        .unwrap();
    }
    let cancellation = Cancellation::default();
    let target = git
        .discover(directory.path(), &cancellation)
        .unwrap()
        .unwrap();
    let mut page = None;
    let mut files = Vec::new();
    let mut first_cursor = None;
    loop {
        let response = git
            .execute(
                "user",
                Request::Files {
                    target: target.clone(),
                    page,
                },
                &cancellation,
            )
            .unwrap();
        assert!(serde_json::to_vec(&response).unwrap().len() < 48 * 1024);
        let Response::Files { paths, next_page } = response else {
            panic!("files response")
        };
        files.extend(paths);
        first_cursor = first_cursor.or_else(|| next_page.clone());
        page = next_page;
        if page.is_none() {
            break;
        }
    }
    assert!(first_cursor.is_some());
    assert_eq!(files.len(), 1401);
    let mut sorted = files.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(files, sorted);
    let mut page = None;
    let mut changes = Vec::new();
    let mut status_cursor = None;
    loop {
        let response = git
            .execute(
                "user",
                Request::Status {
                    target: target.clone(),
                    page,
                },
                &cancellation,
            )
            .unwrap();
        assert!(serde_json::to_vec(&response).unwrap().len() < 48 * 1024);
        let Response::Status { status, next_page } = response else {
            panic!("status response")
        };
        changes.extend(status.entries);
        status_cursor = status_cursor.or_else(|| next_page.clone());
        page = next_page;
        if page.is_none() {
            break;
        }
    }
    assert_eq!(changes.len(), 1400);
    fs::write(folder.join("new.txt"), "new").unwrap();
    assert!(matches!(
        git.execute(
            "user",
            Request::Files {
                target: target.clone(),
                page: first_cursor
            },
            &cancellation
        ),
        Err(Error::Stale)
    ));
    assert!(matches!(
        git.execute(
            "user",
            Request::Status {
                target,
                page: status_cursor
            },
            &cancellation
        ),
        Err(Error::Stale)
    ));
}

#[cfg(unix)]
#[test]
fn working_preview_rejects_special_files_without_waiting_for_a_writer() {
    use std::{
        os::unix::{fs::OpenOptionsExt, net::UnixListener},
        sync::mpsc,
        time::Duration,
    };
    let (directory, git) = fixture();
    let fifo = directory.path().join("pipe");
    assert!(
        Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let _socket = UnixListener::bind(directory.path().join("socket")).unwrap();
    let cancellation = Cancellation::default();
    let target = git
        .discover(directory.path(), &cancellation)
        .unwrap()
        .unwrap();
    let (sender, receiver) = mpsc::channel();
    let worker = git.clone();
    let captured = target.clone();
    let thread = std::thread::spawn(move || {
        sender
            .send(worker.execute(
                "user",
                Request::File {
                    target: captured,
                    path: "pipe".into(),
                    revision: None,
                },
                &Cancellation::default(),
            ))
            .unwrap();
    });
    let result = receiver.recv_timeout(Duration::from_secs(5));
    if result.is_err() {
        // Release an old blocking reader so a failing regression never strands the suite.
        let _writer = fs::OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&fifo);
    }
    thread.join().unwrap();
    assert!(matches!(
        result.expect("preview must settle without a FIFO writer"),
        Err(Error::Invalid(_))
    ));
    for path in ["socket", "."] {
        assert!(
            git.execute(
                "user",
                Request::File {
                    target: target.clone(),
                    path: path.into(),
                    revision: None
                },
                &cancellation
            )
            .is_err()
        );
    }
    std::os::unix::fs::symlink("first.txt", directory.path().join("link")).unwrap();
    let response = git
        .execute(
            "user",
            Request::File {
                target: target.clone(),
                path: "link".into(),
                revision: None,
            },
            &cancellation,
        )
        .unwrap();
    assert!(matches!(response, Response::File { text, truncated: false } if text == "first\n"));
    fs::write(directory.path().join("large.txt"), vec![b'x'; 64 * 1024]).unwrap();
    let response = git
        .execute(
            "user",
            Request::File {
                target,
                path: "large.txt".into(),
                revision: None,
            },
            &cancellation,
        )
        .unwrap();
    assert!(
        matches!(response, Response::File { text, truncated: true } if text.len() <= 16 * 1024)
    );
}

#[cfg(unix)]
#[test]
fn failing_native_hooks_keep_their_staged_edits_without_creating_a_commit() {
    use std::os::unix::fs::PermissionsExt;
    let (directory, git) = fixture();
    fs::write(directory.path().join("first.txt"), "reviewed\n").unwrap();
    mutate(
        &git,
        directory.path(),
        "stage-failing-hook",
        Operation::Stage {
            paths: vec!["first.txt".into()],
        },
    );
    let hook = directory.path().join(".git/hooks/pre-commit");
    fs::write(
        &hook,
        "#!/bin/sh\nprintf 'hook edit\\n' > first.txt\ngit add -- first.txt\nexit 1\n",
    )
    .unwrap();
    fs::set_permissions(hook, fs::Permissions::from_mode(0o755)).unwrap();
    let target = git
        .discover(directory.path(), &Cancellation::default())
        .unwrap()
        .unwrap();
    let head = target.head.clone();
    let receipt = mutate(
        &git,
        directory.path(),
        "commit-failing-hook",
        Operation::Commit {
            message: "feat: rejected by hook".into(),
            reviewed_index: target.index_fingerprint,
        },
    );
    assert_eq!(receipt.state, Outcome::Failed);
    assert_eq!(receipt.head, head);
    assert!(
        receipt
            .message
            .contains("index written during the failed commit was kept")
    );
    assert_eq!(
        command(directory.path(), &["show", ":first.txt"]),
        "hook edit"
    );
    assert_eq!(
        command(directory.path(), &["show", "HEAD:first.txt"]),
        "first"
    );
    assert!(command(directory.path(), &["diff"]).is_empty());
    assert!(!directory.path().join(".git/index.lock").exists());
}
