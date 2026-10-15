#![cfg(unix)]
use adapters::{ClaudeHarness, CodexHarness, PiHarness};
use serde_json::{Value, json};
use std::{fs, path::Path, time::Duration};
use swarmie::*;

fn write_skill(root: &Path, folder: &str, marker: &str) -> std::path::PathBuf {
    let directory = root.join(".agents/skills").join(folder);
    fs::create_dir_all(directory.join("references")).unwrap();
    fs::write(directory.join("SKILL.md"), "---\nname: same-name\ndescription: Read a supporting reference.\n---\nRead references/detail.txt.\n").unwrap();
    fs::write(directory.join("references/detail.txt"), marker).unwrap();
    directory.join("SKILL.md").canonicalize().unwrap()
}

fn options(root: &Path, provider: &str) -> SpawnOptions {
    let home = root.join("home");
    fs::create_dir_all(&home).unwrap();
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    let mut options = SpawnOptions::new(provider, project);
    options
        .execution
        .env
        .insert("HOME".into(), home.to_string_lossy().into_owned());
    options.execution.env.insert(
        "SWARMIE_SKILL_LEDGER".into(),
        root.join(format!("{provider}.jsonl"))
            .to_string_lossy()
            .into_owned(),
    );
    options.execution.config_home = Some(root.join("native-config"));
    options.config = json!({"executable":"python3","args":[Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/shared_skills.py"),provider]});
    options
}

fn ledger(root: &Path, provider: &str) -> Vec<Value> {
    fs::read_to_string(root.join(format!("{provider}.jsonl")))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

async fn finish(connection: &mut BackendConnection) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(event) = connection.events.recv().await {
            if matches!(event, BackendEvent::TurnFinished { .. }) {
                return;
            }
            if matches!(event, BackendEvent::Exited { .. }) {
                panic!("Peer exited before finishing");
            }
        }
        panic!("No terminal event");
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn adapters_bind_native_invocations_to_advertised_origins_and_complete_folders() {
    for harness in [
        Box::new(CodexHarness) as Box<dyn Harness>,
        Box::new(ClaudeHarness),
        Box::new(PiHarness),
    ] {
        let root = tempfile::tempdir().unwrap();
        let options = options(root.path(), harness.id());
        let project = write_skill(&options.cwd, "project", "project-reference");
        let user = write_skill(&root.path().join("home"), "user", "user-reference");
        let mut connection = harness.connect(&options, None, false).await.unwrap();
        assert!(connection.capabilities.native_skill_inputs.is_available());
        let catalog = connection.backend.commands().await.unwrap();
        let entries = catalog.as_array().unwrap();
        assert!(entries.iter().all(|entry| entry["source"] == "skill"));
        assert_eq!(entries.len(), if harness.id() == "pi" { 1 } else { 2 });
        let selected = if harness.id() == "pi" {
            &project
        } else {
            &user
        };
        let prompt = Prompt {
            content: vec![
                PromptContent::Text("Read the reference".into()),
                PromptContent::Skill {
                    name: "same-name".into(),
                    path: selected.to_string_lossy().into_owned(),
                },
            ],
        };
        connection.backend.prompt(&prompt).await.unwrap();
        finish(&mut connection).await;
        let record = ledger(root.path(), harness.id()).pop().unwrap();
        assert_eq!(record["selectedPath"], selected.to_string_lossy().as_ref());
        assert_eq!(
            record["reference"],
            if harness.id() == "pi" {
                "project-reference"
            } else {
                "user-reference"
            }
        );
        if harness.id() == "codex" {
            assert!(
                record["request"]["params"]["input"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|input| input["type"] == "skill"
                        && input["path"] == selected.to_string_lossy().as_ref())
            );
        }
        let wrong = Prompt {
            content: vec![
                PromptContent::Text("wrong".into()),
                PromptContent::Skill {
                    name: "another-name".into(),
                    path: project.to_string_lossy().into_owned(),
                },
            ],
        };
        assert!(matches!(
            connection.backend.prompt(&wrong).await,
            Err(Error::Rejected(_))
        ));
        connection.backend.close().await.unwrap();
        assert!(project.exists() && user.exists());
        if harness.id() == "claude" {
            let launch = &ledger(root.path(), "claude")[0];
            assert_eq!(
                launch["configHome"],
                root.path().join("native-config").to_string_lossy().as_ref()
            );
            let args = launch["launchArgs"].as_array().unwrap();
            let index = args.iter().position(|arg| arg == "--add-dir").unwrap();
            assert!(!Path::new(args[index + 1].as_str().unwrap()).exists());
        }
    }
}

#[tokio::test]
async fn claude_concurrent_overlays_are_independent_and_failed_launch_cleans_only_links() {
    let root = tempfile::tempdir().unwrap();
    let options = options(root.path(), "claude");
    let path = write_skill(&options.cwd, "project", "preserved");
    let first = ClaudeHarness.connect(&options, None, false).await.unwrap();
    let second = ClaudeHarness.connect(&options, None, false).await.unwrap();
    let launches = ledger(root.path(), "claude");
    let overlay = |launch: &Value| {
        let args = launch["launchArgs"].as_array().unwrap();
        std::path::PathBuf::from(
            args[args.iter().position(|arg| arg == "--add-dir").unwrap() + 1]
                .as_str()
                .unwrap(),
        )
    };
    let a = overlay(&launches[0]);
    let b = overlay(&launches[1]);
    assert_ne!(a, b);
    first.backend.close().await.unwrap();
    assert!(!a.exists() && b.exists());
    fs::write(&path, "changed").unwrap();
    let catalog = second.backend.commands().await.unwrap();
    assert_eq!(catalog[0]["availability"], "requires-restart");
    second.backend.close().await.unwrap();
    assert!(!b.exists());
    write_skill(&options.cwd, "project", "preserved");
    let mut failed = options.clone();
    failed
        .execution
        .env
        .insert("SWARMIE_FAIL_INIT".into(), "1".into());
    assert!(ClaudeHarness.connect(&failed, None, false).await.is_err());
    let launches = ledger(root.path(), "claude");
    assert!(!overlay(launches.last().unwrap()).exists());
    assert!(path.exists());
}

#[tokio::test]
async fn disabled_tools_do_not_expose_or_accept_shared_skills() {
    for harness in [
        Box::new(ClaudeHarness) as Box<dyn Harness>,
        Box::new(PiHarness),
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut options = options(root.path(), harness.id());
        write_skill(&options.cwd, "project", "preserved");
        options.execution.tools = ToolPolicy::Disabled;
        let connection = harness.connect(&options, None, false).await.unwrap();
        assert!(!connection.capabilities.native_skill_inputs.is_available());
        assert!(
            connection
                .backend
                .commands()
                .await
                .unwrap()
                .as_array()
                .unwrap()
                .is_empty()
        );
        connection.backend.close().await.unwrap();
    }
}

#[tokio::test]
async fn exact_native_alias_selection_is_unambiguous_when_two_paths_share_one_origin() {
    let root = tempfile::tempdir().unwrap();
    let mut options = options(root.path(), "codex");
    let path = write_skill(&options.cwd, "check", "canonical-reference");
    let first = root.path().join("first-alias");
    let second = root.path().join("second-alias");
    std::os::unix::fs::symlink(path.parent().unwrap(), &first).unwrap();
    std::os::unix::fs::symlink(path.parent().unwrap(), &second).unwrap();
    let first = first.join("SKILL.md");
    let second = second.join("SKILL.md");
    options.execution.env.insert(
        "SWARMIE_NATIVE_SKILL_ALIASES".into(),
        json!([first, second]).to_string(),
    );
    let mut connection = CodexHarness.connect(&options, None, false).await.unwrap();
    let prompt = Prompt {
        content: vec![
            PromptContent::Text("Read reference".into()),
            PromptContent::Skill {
                name: "same-name".into(),
                path: second.to_string_lossy().into_owned(),
            },
        ],
    };
    connection.backend.prompt(&prompt).await.unwrap();
    finish(&mut connection).await;
    let receipt = ledger(root.path(), "codex").pop().unwrap();
    assert_eq!(receipt["selectedPath"], second.to_string_lossy().as_ref());
    assert_eq!(receipt["reference"], "canonical-reference");
    connection.backend.close().await.unwrap();
    assert!(path.exists());
}
