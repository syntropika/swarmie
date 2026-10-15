#![cfg(unix)]
use adapters::{ClaudeHarness, CodexHarness, PiHarness};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
};
use swarmie::*;

fn skill(root: &Path, folder: &str) -> PathBuf {
    let directory = root.join(".agents/skills").join(folder);
    fs::create_dir_all(directory.join("references")).unwrap();
    fs::write(directory.join("SKILL.md"), "---\nname: shared-check\ndescription: Inspect the isolated shared skill fixture and read its supporting reference.\n---\nRead references/detail.txt.\n").unwrap();
    fs::write(
        directory.join("references/detail.txt"),
        "ISOLATED_SHARED_REFERENCE",
    )
    .unwrap();
    directory.join("SKILL.md").canonicalize().unwrap()
}

#[tokio::test]
#[ignore = "Requires installed Codex, Claude and the pinned Pi fixture binary; creates isolated homes only"]
async fn installed_harnesses_advertise_shared_origins_and_preserve_native_project_trust() {
    let pi = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/harnesses/node_modules/.bin/pi")
        .canonicalize()
        .expect("Install the fixture dependencies before this optional acceptance test");
    for harness in [
        Box::new(CodexHarness) as Box<dyn Harness>,
        Box::new(ClaudeHarness),
        Box::new(PiHarness),
    ] {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let project = root.path().join("project");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&home).unwrap();
        let a = skill(&project, "project-check");
        let b = skill(&home, "user-check");
        let mut options = SpawnOptions::new(harness.id(), &project);
        options
            .execution
            .env
            .insert("HOME".into(), home.to_string_lossy().into_owned());
        options.execution.env.insert(
            "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC".into(),
            "1".into(),
        );
        options
            .execution
            .env
            .insert("CLAUDE_CODE_DISABLE_AUTO_UPDATE".into(), "1".into());
        options
            .execution
            .env
            .insert("ANTHROPIC_API_KEY".into(), "isolated-fixture-key".into());
        options
            .execution
            .env
            .insert("ANTHROPIC_BASE_URL".into(), "http://127.0.0.1:1".into());
        for key in [
            "CLAUDE_CODE_OAUTH_TOKEN",
            "ANTHROPIC_AUTH_TOKEN",
            "CLAUDE_CODE_USE_BEDROCK",
            "CLAUDE_CODE_USE_VERTEX",
            "CLAUDE_CODE_USE_FOUNDRY",
        ] {
            options.execution.env.insert(
                key.into(),
                if key.contains("USE_") { "0" } else { "" }.into(),
            );
        }
        let config = home.join(format!("native-{}", harness.id()));
        fs::create_dir_all(&config).unwrap();
        options.execution.config_home = Some(config.clone());
        if harness.id() == "codex" {
            fs::write(config.join("config.toml"), "model = \"fixture\"\nmodel_provider = \"fixture\"\n[model_providers.fixture]\nname = \"Isolated fixture\"\nbase_url = \"http://127.0.0.1:1/v1\"\nwire_api = \"responses\"\nrequires_openai_auth = false\n").unwrap();
        } else if harness.id() == "pi" {
            fs::write(
                config.join("settings.json"),
                "{\"defaultProjectTrust\":\"always\"}",
            )
            .unwrap();
            options.config =
                json!({"executable":pi,"args":["--mode","rpc","--no-session","--offline"]});
        }
        let connection = harness.connect(&options, None, false).await.unwrap();
        let catalog = connection.backend.commands().await.unwrap();
        let entries = catalog
            .as_array()
            .unwrap()
            .iter()
            .filter(|entry| entry["source"] == "skill" && entry["name"] == "shared-check")
            .collect::<Vec<_>>();
        eprintln!(
            "{} native shared origins: {:?}",
            harness.id(),
            entries
                .iter()
                .map(|entry| &entry["path"])
                .collect::<Vec<_>>()
        );
        assert!(
            entries
                .iter()
                .any(|entry| Path::new(entry["path"].as_str().unwrap())
                    .canonicalize()
                    .unwrap()
                    == a),
            "{catalog}"
        );
        if harness.id() != "pi" {
            assert!(
                entries
                    .iter()
                    .any(|entry| Path::new(entry["path"].as_str().unwrap())
                        .canonicalize()
                        .unwrap()
                        == b),
                "{catalog}"
            );
            assert_eq!(entries.len(), 2);
        }
        connection.backend.close().await.unwrap();
        assert!(a.exists() && b.exists());
        if harness.id() == "pi" {
            fs::write(
                config.join("settings.json"),
                "{\"defaultProjectTrust\":\"never\"}",
            )
            .unwrap();
            let restricted = harness.connect(&options, None, false).await.unwrap();
            let catalog = restricted.backend.commands().await.unwrap();
            assert!(!catalog.as_array().unwrap().iter().any(|entry| {
                entry["path"]
                    .as_str()
                    .is_some_and(|path| Path::new(path).canonicalize().ok() == Some(a.clone()))
            }));
            restricted.backend.close().await.unwrap();
        }
    }
}
