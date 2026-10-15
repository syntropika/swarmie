#![cfg(unix)]
use adapters::{AcpHarness, CodexHarness, PiHarness, setup::providers};
use serde_json::{Value, json};
use std::path::PathBuf;
use swarmie::*;
#[tokio::test]
async fn managed_provider_credentials_reach_native_launch_and_codex_custom_provider_request() {
    for (harness, provider, variable, home_var) in [
        ("codex", "openai", "OPENAI_API_KEY", None),
        (
            "pi",
            "anthropic",
            "ANTHROPIC_API_KEY",
            Some("PI_CODING_AGENT_DIR"),
        ),
        ("grok", "xai", "XAI_API_KEY", Some("GROK_HOME")),
    ] {
        let root =
            std::env::temp_dir().join(format!("swarmie-provider-launch-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/provider_execution.py");
        let mut options = SpawnOptions::new(harness, &root);
        let overlay =
            providers::credential_overlay(harness, provider, "fixture-managed-provider-key", &root)
                .unwrap();
        options.execution.env = overlay.env;
        options.execution.config_home = overlay.config_home;
        options.config = overlay.config;
        let capture = root.join("capture.json");
        options.execution.env.insert(
            "SWARMIE_PROVIDER_CAPTURE".into(),
            capture.to_string_lossy().into_owned(),
        );
        options.config["executable"] = json!("python3");
        options.config["args"] = json!([fixture, harness]);
        let adapter: Box<dyn Harness> = match harness {
            "codex" => Box::new(CodexHarness),
            "pi" => Box::new(PiHarness),
            _ => Box::new(AcpHarness::new("grok", "python3", vec![])),
        };
        let connection = adapter.connect(&options, None, false).await.unwrap();
        let observed: Value =
            serde_json::from_str(&std::fs::read_to_string(&capture).unwrap()).unwrap();
        assert_eq!(observed["env"][variable], "fixture-managed-provider-key");
        if let Some(home_var) = home_var {
            assert!(
                observed["env"][home_var]
                    .as_str()
                    .unwrap()
                    .starts_with(&root.to_string_lossy().into_owned())
            );
        }
        if harness == "pi" {
            let args = observed["args"].as_array().unwrap();
            let index = args.iter().position(|value| value == "--provider").unwrap();
            assert_eq!(args[index + 1], provider);
            assert!(
                connection
                    .backend
                    .configure(json!({"model":"openai/other"}))
                    .await
                    .is_err()
            );
        }
        if harness == "grok" {
            assert_eq!(
                observed["env"]["GROK_AUTH_PATH"],
                json!(
                    PathBuf::from(observed["env"]["GROK_HOME"].as_str().unwrap()).join("auth.json")
                )
            );
            let mut conflicting = options.clone();
            conflicting
                .execution
                .env
                .insert("GROK_AUTH_PATH".into(), "external-auth.json".into());
            assert!(adapter.connect(&conflicting, None, false).await.is_err());
        }
        if harness == "codex" {
            let records: Vec<Value> =
                std::fs::read_to_string(capture.with_extension("requests.jsonl"))
                    .unwrap()
                    .lines()
                    .map(|line| serde_json::from_str(line).unwrap())
                    .collect();
            let request = records
                .iter()
                .find(|record| record["method"] == "thread/start")
                .unwrap();
            assert_eq!(request["params"]["modelProvider"], "swarmie-openai");
            assert_eq!(
                request["params"]["config"]["model_providers.swarmie-openai"]["env_key"],
                "OPENAI_API_KEY"
            );
            assert_eq!(
                request["params"]["config"]["model_providers.swarmie-openai"]["requires_openai_auth"],
                false
            );
            assert!(!request.to_string().contains("fixture-managed-provider-key"));
        }
        connection.backend.close().await.unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
