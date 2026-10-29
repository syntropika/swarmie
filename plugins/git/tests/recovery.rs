use std::{fs, path::Path, process::Command, sync::Arc};

use swarmie_git::{
    Authority, Cancellation, DirectoryJournal, Error, Git, Intent, NativeProcess, Operation,
    Outcome, RepositoryTarget, Request, Response, Result,
};

struct Granted(std::path::PathBuf);
impl Authority for Granted {
    fn authorize_discovery(&self, principal: &str, path: &Path) -> Result<()> {
        if principal == "user"
            && path
                .canonicalize()
                .is_ok_and(|path| path.starts_with(&self.0))
        {
            Ok(())
        } else {
            Err(Error::Denied("directory is not granted".into()))
        }
    }
    fn authorize(&self, principal: &str, intent: &Intent) -> Result<()> {
        self.authorize_discovery(
            principal,
            Path::new(
                intent
                    .target
                    .root
                    .as_deref()
                    .unwrap_or(&intent.target.git_dir),
            ),
        )
    }
}

fn command(root: &Path, args: &[&str]) -> String {
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
fn fixture() -> (tempfile::TempDir, Git) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("source");
    fs::create_dir(&root).unwrap();
    command(&root, &["init", "--quiet", "-b", "main"]);
    command(&root, &["config", "user.name", "Fixture User"]);
    command(&root, &["config", "user.email", "fixture@example.invalid"]);
    command(&root, &["config", "commit.gpgsign", "false"]);
    let hooks = directory.path().join("hooks");
    fs::create_dir(&hooks).unwrap();
    command(
        &root,
        &["config", "core.hooksPath", hooks.to_str().unwrap()],
    );
    fs::write(root.join("file.txt"), "first\n").unwrap();
    command(&root, &["add", "file.txt"]);
    command(&root, &["commit", "--quiet", "-m", "feat: fixture"]);
    let git = Git::new(
        Arc::new(NativeProcess),
        Arc::new(DirectoryJournal::open(directory.path().join("journal")).unwrap()),
        Arc::new(Granted(directory.path().canonicalize().unwrap())),
    );
    (directory, git)
}
fn discover(git: &Git, path: &Path) -> RepositoryTarget {
    let Response::Repository {
        repository: Some(target),
    } = git
        .execute(
            "user",
            Request::Discover {
                path: path.to_string_lossy().into(),
            },
            &Cancellation::default(),
        )
        .unwrap()
    else {
        panic!("repository")
    };
    target
}
fn request(git: &Git, value: serde_json::Value) -> Result<Response> {
    git.execute(
        "user",
        serde_json::from_value(value).expect("owner-defined request"),
        &Cancellation::default(),
    )
}

#[test]
fn removed_historical_worktree_does_not_break_the_current_catalog() {
    let (directory, git) = fixture();
    let root = directory.path().join("source");
    let linked = directory.path().join("linked");
    command(
        &root,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            linked.to_str().unwrap(),
            "HEAD",
        ],
    );
    fs::write(linked.join("file.txt"), "staged\n").unwrap();
    let target = discover(&git, &linked);
    let result = git
        .execute(
            "user",
            Request::Mutate {
                operation_id: "old-linked".into(),
                target,
                operation: Operation::Stage {
                    paths: vec!["file.txt".into()],
                },
            },
            &Cancellation::default(),
        )
        .unwrap();
    assert!(matches!(result, Response::Receipt { receipt } if receipt.state == Outcome::Completed));
    command(
        &root,
        &["worktree", "remove", "--force", linked.to_str().unwrap()],
    );
    let target = discover(&git, &root);
    let result = git
        .execute(
            "user",
            Request::Operations { target, page: None },
            &Cancellation::default(),
        )
        .expect("current checkout remains available");
    assert!(
        matches!(result, Response::Operations { operations, unavailable, .. } if operations.len() == 1 && operations[0].state == Outcome::Completed && unavailable.is_empty())
    );
}

#[test]
fn catalog_cannot_trust_a_forged_common_directory() {
    let (directory, git) = fixture();
    let mut target = discover(&git, &directory.path().join("source"));
    target.common_dir = directory.path().join("other/.git").to_string_lossy().into();
    assert!(matches!(
        git.execute(
            "user",
            Request::Operations { target, page: None },
            &Cancellation::default()
        ),
        Err(Error::Stale)
    ));
}

#[test]
fn intent_to_add_is_not_part_of_the_reviewed_commit_tree() {
    let (directory, git) = fixture();
    let root = directory.path().join("source");
    fs::write(root.join("intent.txt"), "not staged\n").unwrap();
    command(&root, &["add", "-N", "intent.txt"]);
    fs::write(root.join("file.txt"), "staged\n").unwrap();
    command(&root, &["add", "file.txt"]);
    let target = discover(&git, &root);
    let reviewed_index = target.index_fingerprint.clone();
    let Response::Receipt { receipt } = git
        .execute(
            "user",
            Request::Mutate {
                operation_id: "commit".into(),
                target,
                operation: Operation::Commit {
                    message: "feat: staged content".into(),
                    reviewed_index,
                    run_hooks: false,
                },
            },
            &Cancellation::default(),
        )
        .unwrap()
    else {
        panic!("receipt")
    };
    assert_eq!(receipt.state, Outcome::Completed);
    assert!(
        !receipt.message.contains("committed tree differs"),
        "{}",
        receipt.message
    );
    assert!(!command(&root, &["ls-tree", "--name-only", "HEAD"]).contains("intent.txt"));
    assert!(command(&root, &["status", "--porcelain"]).contains("intent.txt"));
}

#[test]
fn reconcile_fences_a_never_admitted_operation_before_a_late_request() {
    let (directory, git) = fixture();
    let root = directory.path().join("source");
    fs::write(root.join("file.txt"), "unstaged\n").unwrap();
    let target = discover(&git, &root);
    let reconciled = request(
        &git,
        serde_json::json!({"kind":"reconcile","operation_id":"late","target":target}),
    )
    .unwrap();
    assert!(
        matches!(reconciled, Response::Receipt { receipt } if receipt.state == Outcome::Failed)
    );
    let _late = git.execute(
        "user",
        Request::Mutate {
            operation_id: "late".into(),
            target: target.clone(),
            operation: Operation::Stage {
                paths: vec!["file.txt".into()],
            },
        },
        &Cancellation::default(),
    );
    assert!(command(&root, &["diff", "--cached", "--name-only"]).is_empty());
    let recovered = git
        .execute(
            "user",
            Request::Recover {
                operation_id: "late".into(),
            },
            &Cancellation::default(),
        )
        .unwrap();
    assert!(matches!(recovered, Response::Receipt { receipt } if receipt.state == Outcome::Failed));
}

#[test]
fn identity_read_does_not_require_a_content_observation() {
    let (directory, git) = fixture();
    let root = directory.path().join("source");
    let response = request(
        &git,
        serde_json::json!({"kind":"identify","path":root.to_str().unwrap()}),
    )
    .unwrap();
    assert!(
        matches!(response, Response::Repository { repository: Some(target) } if target.root == Some(root.canonicalize().unwrap().to_string_lossy().into()) && target.fingerprint.is_empty() && target.index_fingerprint.is_empty())
    );
}

struct LateProjection {
    path: std::path::PathBuf,
    status_calls: std::sync::atomic::AtomicUsize,
    armed: std::sync::atomic::AtomicBool,
}
impl swarmie_git::ProcessExecutor for LateProjection {
    fn run(
        &self,
        command: &swarmie_git::CommandSpec,
        cancellation: &Cancellation,
    ) -> Result<swarmie_git::CommandOutput> {
        use std::sync::atomic::Ordering;
        if self.armed.load(Ordering::SeqCst) {
            let extra_status = command.args.iter().any(|arg| arg == "status")
                && self.status_calls.fetch_add(1, Ordering::SeqCst) == 2;
            let extra_files = command.args.iter().any(|arg| arg == "--cached");
            if extra_status || extra_files {
                fs::write(self.path.join("late.txt"), "late\n")?;
            }
        }
        swarmie_git::ProcessExecutor::run(&NativeProcess, command, cancellation)
    }
}

#[test]
fn status_and_files_use_the_same_verified_observation() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    for files in [false, true] {
        let (directory, _) = fixture();
        let root = directory.path().join("source");
        let executor = Arc::new(LateProjection {
            path: root.clone(),
            status_calls: AtomicUsize::new(0),
            armed: AtomicBool::new(false),
        });
        let git = Git::new(
            executor.clone(),
            Arc::new(DirectoryJournal::open(directory.path().join("coherent-journal")).unwrap()),
            Arc::new(Granted(directory.path().canonicalize().unwrap())),
        );
        let target = discover(&git, &root);
        executor.armed.store(true, Ordering::SeqCst);
        let read = if files {
            Request::Files { target, page: None }
        } else {
            Request::Status { target, page: None }
        };
        let result = git.execute("user", read, &Cancellation::default()).unwrap();
        match result {
            Response::Files { paths, .. } => assert!(
                !paths.contains(&"late.txt".into()),
                "file projection reread after verification"
            ),
            Response::Status { status, .. } => assert!(
                !status.entries.iter().any(|entry| entry.path == "late.txt"),
                "status projection reread after verification"
            ),
            _ => panic!("inventory response"),
        }
    }
}

#[test]
fn reconciliation_preserves_admitted_unknown_records_and_principal_identity() {
    use swarmie_git::{Journal, PendingOperation};
    let (directory, git) = fixture();
    let target = discover(&git, &directory.path().join("source"));
    let journal = DirectoryJournal::open(directory.path().join("journal")).unwrap();
    let pending = PendingOperation {
        operation_id: "admitted".into(),
        principal: "user".into(),
        target: target.clone(),
        operation: Operation::Stage {
            paths: vec!["file.txt".into()],
        },
    };
    journal.admit(&pending).unwrap();
    let before = serde_json::to_value(journal.load("admitted").unwrap()).unwrap();
    let response = git
        .execute(
            "user",
            Request::Reconcile {
                operation_id: "admitted".into(),
                target: target.clone(),
            },
            &Cancellation::default(),
        )
        .unwrap();
    assert!(matches!(response, Response::Receipt { receipt } if receipt.state == Outcome::Unknown));
    assert_eq!(
        before,
        serde_json::to_value(journal.load("admitted").unwrap()).unwrap()
    );
    let mut foreign = pending;
    foreign.operation_id = "foreign".into();
    foreign.principal = "someone-else".into();
    journal.admit(&foreign).unwrap();
    assert!(matches!(
        git.execute(
            "user",
            Request::Reconcile {
                operation_id: "foreign".into(),
                target: target.clone()
            },
            &Cancellation::default()
        ),
        Err(Error::Denied(_))
    ));
    let mut changed = target;
    changed.fingerprint = "another-captured-request".into();
    assert!(matches!(
        git.execute(
            "user",
            Request::Reconcile {
                operation_id: "admitted".into(),
                target: changed
            },
            &Cancellation::default()
        ),
        Err(Error::Denied(_))
    ));
}

#[test]
fn an_interrupted_abandonment_remains_a_terminal_fence_after_restart() {
    use swarmie_git::{Journal, PendingOperation};
    let (directory, git) = fixture();
    let root = directory.path().join("source");
    fs::write(root.join("file.txt"), "unstaged\n").unwrap();
    let target = discover(&git, &root);
    let journal = DirectoryJournal::open(directory.path().join("journal")).unwrap();
    journal
        .admit(&PendingOperation {
            operation_id: "interrupted-fence".into(),
            principal: "user".into(),
            target: target.clone(),
            operation: Operation::Abandoned,
        })
        .unwrap();
    drop(git);
    let reopened = Git::new(
        Arc::new(NativeProcess),
        Arc::new(journal),
        Arc::new(Granted(directory.path().canonicalize().unwrap())),
    );
    let response = reopened
        .execute(
            "user",
            Request::Recover {
                operation_id: "interrupted-fence".into(),
            },
            &Cancellation::default(),
        )
        .unwrap();
    assert!(matches!(response, Response::Receipt { receipt } if receipt.state == Outcome::Failed));
    let _late = reopened.execute(
        "user",
        Request::Mutate {
            operation_id: "interrupted-fence".into(),
            target,
            operation: Operation::Stage {
                paths: vec!["file.txt".into()],
            },
        },
        &Cancellation::default(),
    );
    assert!(command(&root, &["diff", "--cached", "--name-only"]).is_empty());
}

struct Gate {
    marker: &'static str,
    armed: std::sync::atomic::AtomicBool,
    entered: std::sync::mpsc::Sender<()>,
    release: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
}
impl swarmie_git::ProcessExecutor for Gate {
    fn run(
        &self,
        command: &swarmie_git::CommandSpec,
        cancellation: &Cancellation,
    ) -> Result<swarmie_git::CommandOutput> {
        if command.args.iter().any(|arg| arg == self.marker)
            && self.armed.swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            self.entered.send(()).unwrap();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(10))
                .expect("release fixture gate");
        }
        swarmie_git::ProcessExecutor::run(&NativeProcess, command, cancellation)
    }
}

#[test]
fn reconciliation_races_before_admission_and_during_execution_without_replay() {
    use std::sync::atomic::{AtomicBool, Ordering};
    for admitted in [false, true] {
        let (directory, _) = fixture();
        let root = directory.path().join("source");
        fs::write(root.join("file.txt"), "staged once\n").unwrap();
        let (entered, enter) = std::sync::mpsc::channel();
        let (release, resume) = std::sync::mpsc::channel();
        let gate = Arc::new(Gate {
            marker: if admitted { "add" } else { "status" },
            armed: AtomicBool::new(false),
            entered,
            release: std::sync::Mutex::new(resume),
        });
        let git = Git::new(
            gate.clone(),
            Arc::new(DirectoryJournal::open(directory.path().join("racing-journal")).unwrap()),
            Arc::new(Granted(directory.path().canonicalize().unwrap())),
        );
        let target = discover(&git, &root);
        gate.armed.store(true, Ordering::SeqCst);
        std::thread::scope(|scope| {
            let original = scope.spawn(|| {
                git.execute(
                    "user",
                    Request::Mutate {
                        operation_id: "racing".into(),
                        target: target.clone(),
                        operation: Operation::Stage {
                            paths: vec!["file.txt".into()],
                        },
                    },
                    &Cancellation::default(),
                )
            });
            enter
                .recv_timeout(std::time::Duration::from_secs(10))
                .expect("original reached gate");
            let response = git.execute(
                "user",
                Request::Reconcile {
                    operation_id: "racing".into(),
                    target: target.clone(),
                },
                &Cancellation::default(),
            );
            release.send(()).unwrap();
            let Response::Receipt { receipt } = response.unwrap() else {
                panic!("receipt")
            };
            assert_eq!(
                receipt.state,
                if admitted {
                    Outcome::InProgress
                } else {
                    Outcome::Failed
                }
            );
            let Response::Receipt { receipt } = original.join().unwrap().unwrap() else {
                panic!("original receipt")
            };
            assert_eq!(
                receipt.state,
                if admitted {
                    Outcome::Completed
                } else {
                    Outcome::Failed
                }
            );
        });
        assert_eq!(
            !command(&root, &["diff", "--cached", "--name-only"]).is_empty(),
            admitted
        );
    }
}

#[test]
fn missing_historical_pending_worktree_still_fences_new_changes() {
    use swarmie_git::{Journal, PendingOperation};
    let (directory, git) = fixture();
    let root = directory.path().join("source");
    let linked = directory.path().join("linked");
    command(
        &root,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            linked.to_str().unwrap(),
            "HEAD",
        ],
    );
    let old = discover(&git, &linked);
    let journal = DirectoryJournal::open(directory.path().join("journal")).unwrap();
    journal
        .admit(&PendingOperation {
            operation_id: "unsettled".into(),
            principal: "user".into(),
            target: old,
            operation: Operation::Stage {
                paths: vec!["file.txt".into()],
            },
        })
        .unwrap();
    command(
        &root,
        &["worktree", "remove", "--force", linked.to_str().unwrap()],
    );
    let target = discover(&git, &root);
    let response = git
        .execute(
            "user",
            Request::Operations { target, page: None },
            &Cancellation::default(),
        )
        .unwrap();
    assert!(
        matches!(response, Response::Operations { operations, .. } if operations.len() == 1 && operations[0].state == Outcome::Unknown)
    );
}
