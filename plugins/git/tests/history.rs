use std::{
    path::Path,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use swarmie_git::{
    Authority, Cancellation, CommandOutput, CommandSpec, DirectoryJournal, Git, Intent,
    NativeProcess, ProcessExecutor, Request, Response, Result,
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

#[derive(Default)]
struct Traced {
    logs: AtomicUsize,
}

impl ProcessExecutor for Traced {
    fn run(&self, command: &CommandSpec, cancellation: &Cancellation) -> Result<CommandOutput> {
        if command.args.iter().any(|value| value == "log") {
            self.logs.fetch_add(1, Ordering::Relaxed);
        }
        NativeProcess.run(command, cancellation)
    }
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}

fn fixture(format: &str) -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    git(
        directory.path(),
        &["init", "-b", "main", &format!("--object-format={format}")],
    );
    git(directory.path(), &["config", "user.name", "History Author"]);
    git(
        directory.path(),
        &["config", "user.email", "history@example.invalid"],
    );
    git(directory.path(), &["config", "commit.gpgsign", "false"]);
    git(
        directory.path(),
        &["commit", "--allow-empty", "-m", "test: initial"],
    );
    git(
        directory.path(),
        &[
            "commit",
            "--allow-empty",
            "-m",
            "test: unicode café\ncontinued subject\n\nbody",
        ],
    );
    directory
}

fn inspect(root: &Path, expected_logs: usize) {
    let state = tempfile::tempdir().unwrap();
    let traced = Arc::new(Traced::default());
    let capability = Git::new(
        traced.clone(),
        Arc::new(DirectoryJournal::open(state.path()).unwrap()),
        Arc::new(Allow),
    );
    let Response::Repository {
        repository: Some(target),
    } = capability
        .execute(
            "test",
            Request::Discover {
                path: root.to_string_lossy().into_owned(),
            },
            &Cancellation::default(),
        )
        .unwrap()
    else {
        panic!("missing repository")
    };
    let Response::History { mut commits } = capability
        .execute(
            "test",
            Request::History { target, limit: 200 },
            &Cancellation::default(),
        )
        .unwrap()
    else {
        panic!("missing history")
    };
    let native = git(
        root,
        &[
            "log",
            "--format=%H%x00%P%x00%an%x00%aI%x00%s%x00",
            "HEAD",
            "--",
        ],
    );
    let fields: Vec<_> = native.split('\0').collect();
    let expected: Vec<_> = fields.as_chunks::<5>().0.iter().map(|record| serde_json::json!({
        "oid": record[0].trim_matches('\n'), "parents": record[1].split_whitespace().collect::<Vec<_>>(),
        "author": record[2], "authored_at": record[3].strip_suffix('Z').map_or_else(|| record[3].to_owned(), |value| format!("{value}+00:00")), "subject": record[4]
    })).collect();
    for commit in &mut commits {
        if let Some(value) = commit.authored_at.strip_suffix('Z') {
            commit.authored_at = format!("{value}+00:00");
        }
    }
    assert_eq!(
        serde_json::to_value(commits).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert_eq!(
        traced.logs.load(Ordering::Relaxed),
        expected_logs,
        "ordinary metadata uses library decoding; recoding uses the native compatibility path"
    );
}

#[test]
fn library_history_matches_native_subject_author_time_and_parents() {
    inspect(fixture("sha1").path(), 0);
}

#[test]
fn library_history_supports_sha256_objects() {
    inspect(fixture("sha256").path(), 0);
}

#[test]
fn library_history_retains_native_shallow_parent_boundaries() {
    let source = fixture("sha1");
    let destination = tempfile::tempdir().unwrap();
    git(
        source.path(),
        &[
            "clone",
            "--depth=1",
            "--no-local",
            source.path().to_str().unwrap(),
            destination.path().to_str().unwrap(),
        ],
    );
    inspect(destination.path(), 0);
}

#[test]
fn library_history_honors_repository_object_replacements() {
    let source = fixture("sha1");
    let original = git(source.path(), &["rev-parse", "HEAD"]);
    let replacement = git(source.path(), &["rev-parse", "HEAD~1"]);
    git(source.path(), &["replace", &original, &replacement]);
    inspect(source.path(), 0);
}

#[test]
fn native_history_recodes_declared_non_utf8_commit_messages() {
    let source = fixture("sha1");
    let message = source.path().join("message.bin");
    std::fs::write(&message, b"test: caf\xe9\n").unwrap();
    git(
        source.path(),
        &[
            "-c",
            "i18n.commitEncoding=ISO-8859-1",
            "commit",
            "--quiet",
            "--allow-empty",
            "-F",
            message.to_str().unwrap(),
        ],
    );
    inspect(source.path(), 1);
}

#[test]
fn library_history_reads_packed_objects() {
    let source = fixture("sha1");
    git(source.path(), &["gc", "--prune=now"]);
    inspect(source.path(), 0);
}

#[test]
fn library_history_uses_the_captured_linked_worktree_identity() {
    let source = fixture("sha1");
    let destination = tempfile::tempdir().unwrap();
    let checkout = destination.path().join("checkout");
    git(
        source.path(),
        &[
            "worktree",
            "add",
            "--detach",
            checkout.to_str().unwrap(),
            "HEAD",
        ],
    );
    inspect(&checkout, 0);
}

#[test]
fn oversized_commit_data_is_an_explicit_observation_limit() {
    let source = fixture("sha1");
    let message = source.path().join("large-message");
    std::fs::write(&message, vec![b'a'; 4 * 1024 * 1024 + 1]).unwrap();
    git(
        source.path(),
        &[
            "commit",
            "--quiet",
            "--allow-empty",
            "-F",
            message.to_str().unwrap(),
        ],
    );
    let state = tempfile::tempdir().unwrap();
    let capability = Git::new(
        Arc::new(NativeProcess),
        Arc::new(DirectoryJournal::open(state.path()).unwrap()),
        Arc::new(Allow),
    );
    let Response::Repository {
        repository: Some(target),
    } = capability
        .execute(
            "test",
            Request::Discover {
                path: source.path().to_string_lossy().into_owned(),
            },
            &Cancellation::default(),
        )
        .unwrap()
    else {
        panic!("missing repository")
    };
    assert!(matches!(
        capability.execute(
            "test",
            Request::History { target, limit: 200 },
            &Cancellation::default()
        ),
        Err(swarmie_git::Error::OutputLimit)
    ));
}
