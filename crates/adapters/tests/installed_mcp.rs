#![cfg(unix)]
use adapters::PiHarness;
use serde_json::json;
use std::{collections::BTreeMap, fs, path::Path, time::Duration};
use swarmie::*;

#[tokio::test]
#[ignore = "Requires the pinned installed Pi CLI; connects only an isolated local fixture"]
async fn installed_pi_loads_managed_tools_keeps_literal_credentials_and_honors_disabled_tools() {
    let binary = std::env::var_os("SWARMIE_TEST_PI_EXECUTABLE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/harnesses/node_modules/.bin/pi")
        })
        .canonicalize()
        .unwrap();
    let server = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mcp_server.py");
    let root = tempfile::tempdir().unwrap();
    let ledger = root.path().join("ledger.jsonl");
    let home = root.path().join("home");
    fs::create_dir(&home).unwrap();
    let mut options = SpawnOptions::new("pi", root.path());
    options.execution.config_home = Some(home.join(".pi/agent"));
    options
        .execution
        .env
        .insert("HOME".into(), home.to_string_lossy().into_owned());
    options.config =
        json!({"executable":binary,"args":["--mode","rpc","--no-session","--offline"]});
    options.execution.mcp_servers.push(McpServer {
        name: "swarmie_managed_fixture".into(),
        transport: McpTransport::Stdio {
            command: "python3".into(),
            args: vec![server.to_string_lossy().into_owned()],
            env: BTreeMap::from([
                ("MCP_LEDGER".into(), ledger.to_string_lossy().into_owned()),
                ("MCP_TOKEN".into(), "!literal-${HOME}-$!".into()),
            ]),
        },
    });
    let connection = PiHarness.connect(&options, None, false).await.unwrap();
    tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            let rows = connection
                .backend
                .mcp_control(json!({"action":"inspect"}))
                .await
                .unwrap();
            if rows[0]["tools"]
                .as_array()
                .is_some_and(|tools| tools.iter().any(|tool| tool == "fixture_read"))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("The native SDK must actually discover the MCP tool");
    connection.backend.close().await.unwrap();
    let records = fs::read_to_string(&ledger).unwrap();
    let first: serde_json::Value = serde_json::from_str(records.lines().next().unwrap()).unwrap();
    assert_eq!(first["credential"], "!literal-${HOME}-$!");
    assert!(records.contains("tools/list"));
    fs::remove_file(&ledger).unwrap();
    options.execution.tools = ToolPolicy::Disabled;
    let disabled = PiHarness.connect(&options, None, false).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!ledger.exists());
    disabled.backend.close().await.unwrap();
}

#[tokio::test]
async fn native_pi_names_cannot_silently_override_injected_definitions() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("agent");
    fs::create_dir(&config).unwrap();
    fs::write(
        config.join("mcp.json"),
        "{\"mcpServers\":{\"swarmie_managed_fixture\":{\"command\":\"native-server\"}}}",
    )
    .unwrap();
    let mut options = SpawnOptions::new("pi", root.path());
    options.execution.config_home = Some(config);
    options.execution.mcp_servers.push(McpServer {
        name: "swarmie_managed_fixture".into(),
        transport: McpTransport::Stdio {
            command: "injected-server".into(),
            args: vec![],
            env: BTreeMap::new(),
        },
    });
    options.config = json!({"executable":"must-not-launch"});
    assert!(matches!(
        PiHarness.connect(&options, None, false).await,
        Err(Error::Rejected(_))
    ));
}

#[tokio::test]
#[ignore = "Requires installed Codex and Claude; uses isolated native files and a local MCP peer without inference"]
async fn installed_native_file_adapters_load_and_disable_the_actual_mcp_server() {
    for (harness, scope) in [
        (Box::new(adapters::CodexHarness) as Box<dyn Harness>, "user"),
        (Box::new(adapters::ClaudeHarness), "user"),
        (Box::new(adapters::ClaudeHarness), "project"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().canonicalize().unwrap();
        let cwd = if scope == "project" {
            fs::create_dir(repository.join(".git")).unwrap();
            fs::create_dir(repository.join(".claude")).unwrap();
            fs::write(
                repository.join(".claude/settings.local.json"),
                "{\"enableAllProjectMcpServers\":true}",
            )
            .unwrap();
            let nested = repository.join("nested");
            fs::create_dir(&nested).unwrap();
            nested
        } else {
            repository
        };
        let home = cwd.join("home");
        let config = cwd.join("config");
        fs::create_dir(&home).unwrap();
        fs::create_dir(&config).unwrap();
        let ledger = cwd.join("ledger.jsonl");
        if harness.id() == "codex" {
            fs::write(config.join("config.toml"),"model = \"fixture\"\nmodel_provider = \"fixture\"\n[model_providers.fixture]\nname = \"Isolated fixture\"\nbase_url = \"http://127.0.0.1:1/v1\"\nwire_api = \"responses\"\nrequires_openai_auth = false\n").unwrap();
        }
        let query = McpQuery {
            harness: Some(harness.id().into()),
            cwd: Some(cwd.clone()),
            config_home: Some(config.clone()),
            owner_home: Some(home.clone()),
            ..Default::default()
        };
        let mut install:McpMutation=serde_json::from_value(json!({"query":query,"action":"install","name":"fixture","definition":{"transport":"stdio","command":"python3","args":[Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mcp_server.py")],"envVars":["MCP_LEDGER","MCP_TOKEN"]}})).unwrap();
        install.scope = Some(scope.into());
        install.query.owner_home = Some(home.clone());
        harness.mcp_mutate(&install).await.unwrap();
        let mut options = SpawnOptions::new(harness.id(), &cwd);
        options.execution.config_home = Some(config);
        options
            .execution
            .env
            .insert("HOME".into(), home.to_string_lossy().into_owned());
        options
            .execution
            .env
            .insert("MCP_LEDGER".into(), ledger.to_string_lossy().into_owned());
        options
            .execution
            .env
            .insert("MCP_TOKEN".into(), "isolated-fixture-value".into());
        for (key, value) in [
            ("ANTHROPIC_API_KEY", "isolated-fixture-key"),
            ("ANTHROPIC_BASE_URL", "http://127.0.0.1:1"),
            ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1"),
            ("CLAUDE_CODE_DISABLE_AUTO_UPDATE", "1"),
            ("CLAUDE_CODE_OAUTH_TOKEN", ""),
            ("ANTHROPIC_AUTH_TOKEN", ""),
            ("CLAUDE_CODE_USE_BEDROCK", "0"),
            ("CLAUDE_CODE_USE_VERTEX", "0"),
            ("CLAUDE_CODE_USE_FOUNDRY", "0"),
        ] {
            options.execution.env.insert(key.into(), value.into());
        }
        let connection = harness.connect(&options, None, false).await.unwrap();
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let rows = connection
                    .backend
                    .mcp_control(json!({"action":"inspect"}))
                    .await
                    .unwrap();
                if rows.as_array().is_some_and(|rows| {
                    rows.iter()
                        .any(|row| row["name"] == "fixture" && row["runtimeStatus"] == "connected")
                }) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .expect("The native CLI must connect the server registered in its own source file");
        connection.backend.close().await.unwrap();
        let first: serde_json::Value =
            serde_json::from_str(fs::read_to_string(&ledger).unwrap().lines().next().unwrap())
                .unwrap();
        assert_eq!(first["credential"], "isolated-fixture-value");
        let entry = harness
            .mcp_catalog(&query)
            .await
            .unwrap()
            .into_iter()
            .find(|entry| entry.name == "fixture")
            .unwrap();
        let mut disable:McpMutation=serde_json::from_value(json!({"query":query,"action":"disable","id":entry.id,"expectedRevision":entry.revision})).unwrap();
        disable.query.owner_home = Some(home);
        harness.mcp_mutate(&disable).await.unwrap();
        fs::remove_file(&ledger).unwrap();
        let disabled = harness.connect(&options, None, false).await.unwrap();
        disabled
            .backend
            .mcp_control(json!({"action":"inspect"}))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(
            !ledger.exists(),
            "{} restarted a disabled native MCP",
            harness.id()
        );
        disabled.backend.close().await.unwrap();
    }
}
