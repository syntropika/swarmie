use swarmie::Runtime;
use swarmie::TurnOutcome;
use swarmie_git::GitExtension;
use protocol::RuntimeService;
use serde_json::{Value, json};
use std::path::Path;

fn git(path: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .current_dir(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn repository(path: &Path) {
    git(path, &["init", "-b", "main"]);
    git(path, &["config", "user.email", "fixture@example.test"]);
    git(path, &["config", "user.name", "Fixture"]);
    std::fs::write(path.join("hello.txt"), "before\n").unwrap();
    git(path, &["add", "."]);
    git(path, &["commit", "-m", "Initial fixture"]);
}
async fn invoke(service: &RuntimeService, command: &str, arguments: Value) -> swarmie::Result<Value> {
    service
        .invoke(
            "extension/invoke",
            json!({"extensionId":"git","command":command,"arguments":arguments}),
        )
        .await
}

#[tokio::test]
async fn git_extension_is_optional_and_runs_a_real_worktree_review_workflow() {
    let plain = RuntimeService::new(Runtime::default());
    assert_eq!(
        plain.invoke("extension/list", json!({})).await.unwrap(),
        json!([])
    );
    assert!(
        invoke(&plain, "project.open", json!({"path":"."}))
            .await
            .is_err()
    );
    let repo = tempfile::tempdir().unwrap();
    let trees = tempfile::tempdir().unwrap();
    repository(repo.path());
    let service = RuntimeService::new(
        Runtime::builder()
            .extension(GitExtension::new(trees.path()).await.unwrap())
            .build(),
    );
    let commands = service.invoke("extension/list", json!({})).await.unwrap();
    assert!(
        commands[0]["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["name"] == "review")
    );
    assert!(
        !commands[0]["commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["name"] == "pr.merge")
    );
    let opened = invoke(&service, "project.open", json!({"path":repo.path()}))
        .await
        .unwrap();
    let project = &opened["id"];
    let tree = invoke(
        &service,
        "worktree.create",
        json!({"projectId":project,"branch":"feature/review"}),
    )
    .await
    .unwrap();
    let path = Path::new(tree["path"].as_str().unwrap());
    assert_eq!(
        std::fs::read_to_string(path.join("hello.txt")).unwrap(),
        "before\n"
    );
    std::fs::write(path.join("hello.txt"), "after\n").unwrap();
    std::fs::write(path.join("new file.txt"), "new content\n").unwrap();
    git(path, &["add", "hello.txt"]);
    let review = invoke(
        &service,
        "review",
        json!({"projectId":project,"worktreeId":tree["id"]}),
    )
    .await
    .unwrap();
    let diff = review["diff"].as_str().unwrap();
    assert!(diff.contains("-before") && diff.contains("+after") && diff.contains("+new content"));
    assert_eq!(review["files"].as_array().unwrap().len(), 2);
    assert_eq!(
        std::fs::read_to_string(repo.path().join("hello.txt")).unwrap(),
        "before\n"
    );
    assert!(
        invoke(
            &service,
            "worktree.remove",
            json!({"projectId":project,"worktreeId":tree["id"]})
        )
        .await
        .is_err()
    );
    assert!(path.exists());
    assert!(
        invoke(
            &service,
            "worktree.remove",
            json!({"projectId":project,"worktreeId":opened["worktrees"][0]["id"]})
        )
        .await
        .is_err()
    );
    git(path, &["restore", "--staged", "hello.txt"]);
    git(path, &["restore", "hello.txt"]);
    std::fs::remove_file(path.join("new file.txt")).unwrap();
    invoke(
        &service,
        "worktree.remove",
        json!({"projectId":project,"worktreeId":tree["id"]}),
    )
    .await
    .unwrap();
    assert!(!path.exists());
    git(
        repo.path(),
        &["show-ref", "--verify", "refs/heads/feature/review"],
    );
    assert!(
        invoke(
            &service,
            "worktree.create",
            json!({"projectId":project,"branch":"-bad"})
        )
        .await
        .is_err()
    );
    assert!(
        invoke(
            &service,
            "review",
            json!({"projectId":project,"worktreeId":"/tmp"})
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn review_handles_renames_binary_files_symlinks_and_bounded_patches() {
    let repo = tempfile::tempdir().unwrap();
    let trees = tempfile::tempdir().unwrap();
    repository(repo.path());
    let service = RuntimeService::new(
        Runtime::builder()
            .extension(GitExtension::new(trees.path()).await.unwrap())
            .build(),
    );
    let opened = invoke(&service, "project.open", json!({"path":repo.path()}))
        .await
        .unwrap();
    git(repo.path(), &["mv", "hello.txt", "renamed file.txt"]);
    std::fs::write(repo.path().join("binary.dat"), [0, 1, 2, 0, 255]).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("/etc/passwd", repo.path().join("external-link")).unwrap();
    let params = json!({"projectId":opened["id"],"worktreeId":opened["worktrees"][0]["id"]});
    let review = invoke(&service, "review", params.clone()).await.unwrap();
    assert!(
        review["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["previousPath"] == "hello.txt")
    );
    let diff = review["diff"].as_str().unwrap();
    assert!(diff.contains("Binary files"));
    #[cfg(unix)]
    {
        assert!(diff.contains("+/etc/passwd"));
        assert!(!diff.contains("root:x:"));
    }
    std::fs::write(repo.path().join("large.txt"), "line\n".repeat(30_000)).unwrap();
    let review = invoke(&service, "review", params).await.unwrap();
    assert_eq!(review["truncated"], true);
    assert!(review["diff"].as_str().unwrap().len() <= 96 * 1024);
}

#[tokio::test]
async fn review_scopes_stage_literal_names_and_keep_turn_checkpoints_isolated() {
    use swarmie::{
        Capabilities, EventKind, NativeSessionRef, Prompt, RuntimeEvent, RuntimeExtension,
        SessionSnapshot, SessionState, SpawnOptions,
    };
    let repo = tempfile::tempdir().unwrap();
    let trees = tempfile::tempdir().unwrap();
    repository(repo.path());
    let extension = GitExtension::new(trees.path()).await.unwrap();
    let opened = extension
        .invoke("project.open", json!({"path":repo.path()}))
        .await
        .unwrap();
    let params = json!({"projectId":opened["id"],"worktreeId":opened["worktrees"][0]["id"]});
    std::fs::write(repo.path().join("hello.txt"), "staged\n").unwrap();
    git(repo.path(), &["add", "hello.txt"]);
    std::fs::write(repo.path().join("hello.txt"), "before turn\n").unwrap();
    let mut p = params.clone();
    p["scope"] = json!("staged");
    let review = extension.invoke("review", p).await.unwrap();
    assert!(review["diff"].as_str().unwrap().contains("+staged"));
    let mut p = params.clone();
    p["scope"] = json!("unstaged");
    let review = extension.invoke("review", p).await.unwrap();
    assert!(review["diff"].as_str().unwrap().contains("+before turn"));
    let id = uuid::Uuid::new_v4().to_string();
    let turn = uuid::Uuid::new_v4().to_string();
    let snapshot = SessionSnapshot {
        working_context: None,
        execution_policy: Default::default(),
        id: id.clone(),
        native: NativeSessionRef {
            provider: "fixture".into(),
            id: "native".into(),
        },
        options: SpawnOptions::new("fixture", repo.path()),
        capabilities: Capabilities::default(),
        state: SessionState::Idle,
        sequence: 0,
        turn: None,
        turns_completed: 0,
        queue: Default::default(),
        metadata: Default::default(),
    };
    extension
        .before_prompt(&snapshot, &mut Prompt::from("change the file"))
        .await
        .unwrap();
    let start = RuntimeEvent {
        session_id: id.clone(),
        sequence: 1,
        turn_id: Some(turn.clone()),
        source: "runtime".into(),
        timestamp_ms: 0,
        kind: EventKind::TurnStarted,
        raw: None,
    };
    extension.on_event(&start).await;
    std::fs::write(repo.path().join("hello.txt"), "after turn\n").unwrap();
    extension
        .on_event(&RuntimeEvent {
            kind: EventKind::TurnEnded {
                outcome: TurnOutcome::Completed,

                native_reason: None,
            },
            ..start
        })
        .await;
    std::fs::write(repo.path().join("hello.txt"), "later edit\n").unwrap();
    let mut p = params.clone();
    p["scope"] = json!("turn");
    p["sessionId"] = json!(id);
    p["turnId"] = json!(turn);
    let review = extension.invoke("review", p).await.unwrap();
    let diff = review["diff"].as_str().unwrap();
    assert!(
        diff.contains("-before turn")
            && diff.contains("+after turn")
            && !diff.contains("later edit")
    );
    git(repo.path(), &["gc", "--prune=now"]);
    let restarted = GitExtension::new(trees.path()).await.unwrap();
    restarted
        .invoke("project.open", json!({"path":repo.path()}))
        .await
        .unwrap();
    let mut restored = params.clone();
    restored["scope"] = json!("turn");
    restored["sessionId"] = json!(id);
    restored["turnId"] = json!(turn);
    let restored = restarted.invoke("review", restored).await.unwrap();
    assert_eq!(restored["diff"], review["diff"]);
    let staged = std::process::Command::new("git")
        .current_dir(repo.path())
        .args(["show", ":hello.txt"])
        .output()
        .unwrap();
    assert_eq!(staged.stdout, b"staged\n");
    std::fs::write(repo.path().join("[literal].txt"), "literal\n").unwrap();
    std::fs::write(repo.path().join("l.txt"), "other\n").unwrap();
    let mut p = params.clone();
    p["paths"] = json!(["[literal].txt"]);
    extension.invoke("stage", p.clone()).await.unwrap();
    let mut staged = params.clone();
    staged["scope"] = json!("staged");
    let review = extension.invoke("review", staged.clone()).await.unwrap();
    assert!(
        review["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["path"] == "[literal].txt")
    );
    assert!(
        !review["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["path"] == "l.txt")
    );
    extension.invoke("unstage", p).await.unwrap();
    let review = extension.invoke("review", staged).await.unwrap();
    assert!(
        !review["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["path"] == "[literal].txt")
    );
    let mut p = params;
    p["scope"] = json!("base");
    p["base"] = json!("--output=/tmp/injection");
    assert!(extension.invoke("review", p).await.is_err());
}
