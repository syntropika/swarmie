use swarmie::RuntimeExtension;
use swarmie_git::GitExtension;
use serde_json::{Value, json};
use std::path::Path;
fn git(cwd: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}
fn init(root: &Path) {
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.email", "fixture@example.test"]);
    git(root, &["config", "user.name", "Fixture"]);
    std::fs::write(root.join("hello.txt"), "before").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "Initialize fixture"]);
}
#[tokio::test]
async fn staged_commit_push_and_managed_setup_are_real_and_cleanup_refuses_dirty_worktrees() {
    let repo = tempfile::tempdir().unwrap();
    let trees = tempfile::tempdir().unwrap();
    let remote = tempfile::tempdir().unwrap();
    init(repo.path());
    git(remote.path(), &["init", "--bare"]);
    git(
        repo.path(),
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    let extension = GitExtension::new(trees.path()).await.unwrap();
    let project = extension
        .invoke("project.open", json!({"path":repo.path()}))
        .await
        .unwrap();
    let id = &project["id"];
    std::fs::write(repo.path().join("hello.txt"), "staged change").unwrap();
    git(repo.path(), &["add", "hello.txt"]);
    let prompt = extension
        .invoke("text.prompt", json!({"projectId":id,"kind":"commit"}))
        .await
        .unwrap();
    assert!(prompt["prompt"].as_str().unwrap().contains("staged change"));
    extension
        .invoke(
            "commit",
            json!({"projectId":id,"message":"Commit literal $() text\n\nBody stays multiline"}),
        )
        .await
        .unwrap();
    assert!(git(repo.path(), &["log", "-1", "--format=%B"]).contains("literal $() text\n\nBody"));
    let pr_prompt = extension
        .invoke(
            "text.prompt",
            json!({"projectId":id,"kind":"pr","base":"HEAD^"}),
        )
        .await
        .unwrap();
    assert!(
        pr_prompt["prompt"]
            .as_str()
            .unwrap()
            .contains("+staged change")
    );
    assert!(
        extension
            .invoke("text.prompt", json!({"projectId":id,"kind":"pr"}))
            .await
            .is_err()
    );
    extension
        .invoke("push", json!({"projectId":id}))
        .await
        .unwrap();
    assert_eq!(
        git(repo.path(), &["rev-parse", "HEAD"]),
        git(remote.path(), &["rev-parse", "refs/heads/main"])
    );
    extension.invoke("worktree.setup.configure",json!({"projectId":id,"commands":[{"program":"python3","args":["-c","from pathlib import Path; Path('setup-created.txt').write_text('setup')"]}]})).await.unwrap();
    let tree = extension
        .invoke(
            "worktree.create",
            json!({"projectId":id,"branch":"feature/setup","setup":true}),
        )
        .await
        .unwrap();
    assert_eq!(tree["setup"]["completed"], true);
    let path = Path::new(tree["path"].as_str().unwrap());
    assert_eq!(
        std::fs::read_to_string(path.join("setup-created.txt")).unwrap(),
        "setup"
    );
    let cleanup = extension
        .invoke(
            "worktree.cleanup",
            json!({"projectId":id,"worktreeIds":[tree["id"]]}),
        )
        .await
        .unwrap();
    assert_eq!(cleanup["results"][0]["removed"], false);
    assert!(path.exists());
    std::fs::remove_file(path.join("setup-created.txt")).unwrap();
    extension
        .invoke(
            "worktree.remove",
            json!({"projectId":id,"worktreeId":tree["id"]}),
        )
        .await
        .unwrap();
    assert!(!path.exists());
    let restarted = GitExtension::new(trees.path()).await.unwrap();
    let project = restarted
        .invoke("project.open", json!({"path":repo.path()}))
        .await
        .unwrap();
    let tree = restarted
        .invoke(
            "worktree.create",
            json!({"projectId":project["id"],"branch":"feature/restarted","setup":true}),
        )
        .await
        .unwrap();
    assert_eq!(tree["setup"]["completed"], true);
}
#[tokio::test]
async fn github_actions_use_drafts_body_files_structured_arguments_and_viewed_sync_never_merge() {
    let repo = tempfile::tempdir().unwrap();
    let trees = tempfile::tempdir().unwrap();
    let peer = tempfile::tempdir().unwrap();
    init(repo.path());
    let executable = peer.path().join("gh.py");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gh.py"),
        &executable,
    )
    .unwrap();
    let extension = GitExtension::new(trees.path())
        .await
        .unwrap()
        .with_github_executable(&executable);
    let project = extension
        .invoke("project.open", json!({"path":repo.path()}))
        .await
        .unwrap();
    let id = &project["id"];
    assert_eq!(
        extension
            .invoke("integration.status", json!({"projectId":id}))
            .await
            .unwrap()["authenticated"],
        true
    );
    let body = "First paragraph\n\nLiteral $(no-shell) and `text`";
    let pr = extension
        .invoke(
            "pr.create",
            json!({"projectId":id,"title":"Draft fixture","body":body}),
        )
        .await
        .unwrap();
    assert_eq!(pr["draft"], true);
    assert_eq!(
        extension
            .invoke("pr.checks", json!({"projectId":id,"number":7}))
            .await
            .unwrap()[0]["bucket"],
        "fail"
    );
    extension
        .invoke("pr.comment", json!({"projectId":id,"number":7,"body":body}))
        .await
        .unwrap();
    extension
        .invoke(
            "pr.review",
            json!({"projectId":id,"number":7,"event":"approve","body":body}),
        )
        .await
        .unwrap();
    extension
        .invoke(
            "pr.viewed",
            json!({"projectId":id,"number":7,"path":"hello.txt","viewed":true}),
        )
        .await
        .unwrap();
    extension
        .invoke(
            "pr.review",
            json!({"projectId":id,"number":7,"event":"approve"}),
        )
        .await
        .unwrap();
    assert_eq!(
        extension
            .invoke("pr.viewed.list", json!({"projectId":id,"number":7}))
            .await
            .unwrap()["files"][0]["viewerViewedState"],
        "VIEWED"
    );
    assert!(
        extension
            .invoke("pr.merge", json!({"projectId":id,"number":7}))
            .await
            .is_err()
    );
    std::fs::write(executable.with_extension("legacy"), "legacy checks syntax").unwrap();
    let legacy = extension
        .invoke("pr.checks", json!({"projectId":id,"number":7}))
        .await
        .unwrap();
    assert_eq!(legacy[0]["bucket"], "fail");
    assert_eq!(legacy[0]["name"], "native-failure");
    let ledger = std::fs::read_to_string(executable.with_extension("jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(
        ledger
            .iter()
            .any(|v| v["args"].as_array().unwrap().contains(&json!("--draft")))
    );
    assert_eq!(ledger.iter().filter(|v| v["body"] == body).count(), 3);
    assert!(
        !ledger
            .iter()
            .any(|v| v["args"].as_array().unwrap().contains(&json!("merge")))
    );
    for entry in ledger {
        let args = entry["args"].as_array().unwrap();
        if let Some(index) = args.iter().position(|v| v == "--body-file") {
            assert!(!Path::new(args[index + 1].as_str().unwrap()).exists());
        }
    }
}
#[tokio::test]
async fn unborn_repository_can_be_opened_and_missing_github_is_actionable() {
    let repo = tempfile::tempdir().unwrap();
    let trees = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    let extension = GitExtension::new(trees.path())
        .await
        .unwrap()
        .with_github_executable(repo.path().join("missing-gh"));
    let project = extension
        .invoke("project.open", json!({"path":repo.path()}))
        .await
        .unwrap();
    let status = extension
        .invoke("integration.status", json!({"projectId":project["id"]}))
        .await
        .unwrap();
    assert_eq!(status["available"], false);
    assert!(status["message"].as_str().unwrap().contains("install gh"));
}

#[tokio::test]
async fn initialized_submodule_cleanup_preserves_dirty_changes_then_removes_clean_worktree() {
    let repo = tempfile::tempdir().unwrap();
    let module = tempfile::tempdir().unwrap();
    let trees = tempfile::tempdir().unwrap();
    init(repo.path());
    init(module.path());
    git(
        repo.path(),
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            module.path().to_str().unwrap(),
            "module",
        ],
    );
    git(repo.path(), &["commit", "-am", "Add fixture submodule"]);
    let extension = GitExtension::new(trees.path()).await.unwrap();
    let project = extension
        .invoke("project.open", json!({"path":repo.path()}))
        .await
        .unwrap();
    // The local-file exception is explicitly configured for this fixture, never a global policy.
    extension.invoke("worktree.setup.configure", json!({"projectId":project["id"],"commands":[{"program":"git","args":["-c","protocol.file.allow=always","submodule","update","--init","--recursive"]}]})).await.unwrap();
    let tree = extension
        .invoke(
            "worktree.create",
            json!({"projectId":project["id"],"branch":"submodule-fixture","setup":true}),
        )
        .await
        .unwrap();
    assert_eq!(tree["setup"]["completed"], true);
    let path = Path::new(tree["path"].as_str().unwrap());
    assert_eq!(
        std::fs::read_to_string(path.join("module/hello.txt")).unwrap(),
        "before"
    );
    std::fs::write(path.join("module/hello.txt"), "keep my changes").unwrap();
    let parameters = json!({"projectId":project["id"],"worktreeId":tree["id"]});
    assert!(
        extension
            .invoke("worktree.remove", parameters.clone())
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(path.join("module/hello.txt")).unwrap(),
        "keep my changes"
    );
    git(&path.join("module"), &["restore", "hello.txt"]);
    extension
        .invoke("worktree.remove", parameters)
        .await
        .unwrap();
    assert!(!path.exists());
    assert!(repo.path().join("module/hello.txt").exists());
}
