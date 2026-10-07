use mpai::{
    Runtime, SpawnOptions,
    mock::{MockHarness, MockScript, MockStep},
};
use mpai_git::GitExtension;
use serde_json::json;
use std::{path::Path, time::Duration};

fn git(path: &Path, args: &[&str]) {
    let result = std::process::Command::new("git")
        .current_dir(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[tokio::test]
async fn restore_changes_only_selected_checkpoint_file_and_refuses_unsafe_checkouts() {
    let repo = tempfile::tempdir().unwrap();
    let trees = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(
        repo.path(),
        &["config", "user.email", "fixture@example.test"],
    );
    git(repo.path(), &["config", "user.name", "Fixture"]);
    std::fs::write(repo.path().join("one.txt"), "before one").unwrap();
    std::fs::write(repo.path().join("two.txt"), "before two").unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-m", "Initial fixture"]);
    let runtime = Runtime::builder()
        .harness(MockHarness::new(MockScript::default().turn(vec![
            MockStep::Sleep { milliseconds: 300 },
            MockStep::text("completed"),
        ])))
        .extension(GitExtension::new(trees.path()).await.unwrap())
        .build();
    let opened = runtime
        .invoke_extension("git", "project.open", json!({"path":repo.path()}))
        .await
        .unwrap();
    let tree = runtime
        .invoke_extension(
            "git",
            "worktree.create",
            json!({"projectId":opened["id"],"branch":"checkpoint-fixture"}),
        )
        .await
        .unwrap();
    let path = Path::new(tree["path"].as_str().unwrap());
    let agent = runtime
        .spawn(SpawnOptions::new("mock", path))
        .await
        .unwrap();
    assert!(matches!(
        runtime
            .invoke_extension(
                "git",
                "worktree.remove",
                json!({"projectId":opened["id"],"worktreeId":tree["id"]})
            )
            .await,
        Err(mpai::Error::Busy)
    ));
    agent.prompt("Change two files").await.unwrap();
    std::fs::write(path.join("one.txt"), "after one").unwrap();
    std::fs::write(path.join("two.txt"), "after two").unwrap();
    git(path, &["add", "."]);
    git(path, &["commit", "-m", "Save completed work"]);
    let session = agent.snapshot().await.id;
    let points = tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let points = runtime
                .invoke_extension("git", "checkpoints", json!({"sessionId":session}))
                .await
                .unwrap();
            if points["checkpoints"][0]["end"].is_string() {
                break points;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let parameters = json!({"projectId":opened["id"],"worktreeId":tree["id"],"sessionId":session,"turnId":points["checkpoints"][0]["turnId"],"paths":["one.txt"]});
    // Completed turns still leave a live native agent able to write this checkout.
    assert!(matches!(
        runtime
            .invoke_extension("git", "checkpoint.restore", parameters.clone())
            .await,
        Err(mpai::Error::Busy)
    ));
    assert_eq!(
        std::fs::read_to_string(path.join("one.txt")).unwrap(),
        "after one"
    );
    agent.close().await.unwrap();
    let nested = path.join("nested");
    std::fs::create_dir(&nested).unwrap();
    let peer = runtime
        .spawn(SpawnOptions::new("mock", &nested))
        .await
        .unwrap();
    assert!(matches!(
        runtime
            .invoke_extension("git", "checkpoint.restore", parameters.clone())
            .await,
        Err(mpai::Error::Busy)
    ));
    peer.close().await.unwrap();
    let mut main = parameters.clone();
    main["worktreeId"] = opened["worktrees"][0]["id"].clone();
    assert!(
        runtime
            .invoke_extension("git", "checkpoint.restore", main)
            .await
            .is_err()
    );
    let mut invalid = parameters.clone();
    invalid["paths"] = json!(["../one.txt"]);
    assert!(
        runtime
            .invoke_extension("git", "checkpoint.restore", invalid)
            .await
            .is_err()
    );
    std::fs::write(path.join("untracked.txt"), "preserve this").unwrap();
    assert!(
        runtime
            .invoke_extension("git", "checkpoint.restore", parameters.clone())
            .await
            .is_err()
    );
    std::fs::remove_file(path.join("untracked.txt")).unwrap();
    let receipt = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match runtime
                .invoke_extension("git", "checkpoint.restore", parameters.clone())
                .await
            {
                Ok(receipt) => break receipt,
                Err(mpai::Error::Busy) => tokio::time::sleep(Duration::from_millis(10)).await,
                Err(error) => panic!("unexpected restore failure: {error}"),
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(receipt["branchChanged"], false);
    assert_eq!(
        std::fs::read_to_string(path.join("one.txt")).unwrap(),
        "before one"
    );
    assert_eq!(
        std::fs::read_to_string(path.join("two.txt")).unwrap(),
        "after two"
    );
    assert_eq!(
        std::fs::read_to_string(repo.path().join("one.txt")).unwrap(),
        "before one"
    );
    assert!(
        runtime
            .invoke_extension("git", "checkpoint.restore", parameters)
            .await
            .is_err()
    );
    git(
        path,
        &["restore", "--source=HEAD", "--staged", "--worktree", "."],
    );
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match runtime
                .invoke_extension(
                    "git",
                    "worktree.remove",
                    json!({"projectId":opened["id"],"worktreeId":tree["id"]}),
                )
                .await
            {
                Ok(_) => break,
                Err(mpai::Error::Busy) => tokio::time::sleep(Duration::from_millis(10)).await,
                Err(error) => panic!("unexpected cleanup failure: {error}"),
            }
        }
    })
    .await
    .unwrap();
    assert!(!path.exists());
}
