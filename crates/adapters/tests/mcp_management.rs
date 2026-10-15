#![cfg(unix)]
use adapters::{ClaudeHarness, CodexHarness, OpenCodeHarness, PiHarness};
use serde_json::json;
use std::fs;
use swarmie::*;

fn mutation(query: &McpQuery, action: &str, entry: Option<&McpEntry>) -> McpMutation {
    serde_json::from_value(json!({"query":query,"action":action,"id":entry.map(|e| &e.id),"expectedRevision":entry.map(|e| &e.revision)})).unwrap()
}
fn definition() -> McpDefinition {
    serde_json::from_value(
        json!({"transport":"stdio","command":"fixture-server","envVars":["MCP_TOKEN"]}),
    )
    .unwrap()
}

#[tokio::test]
async fn native_install_toggle_update_remove_preserve_unrelated_configuration() {
    for harness in [
        Box::new(CodexHarness) as Box<dyn Harness>,
        Box::new(ClaudeHarness),
        Box::new(OpenCodeHarness),
        Box::new(PiHarness),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        fs::create_dir(&project).unwrap();
        let (filename, contents) = match harness.id() {
            "codex" => ("config.toml", "# user comment\nmodel = 'keep-model'\n"),
            "claude" => (".claude.json", "{\"other\":\"keep-value\"}\n"),
            "pi" => ("mcp.json", "{\"other\":\"keep-value\"}\n"),
            _ => (
                "opencode.json",
                "{ // user comment\n \"other\":\"keep-value\",\n}\n",
            ),
        };
        let path = temp.path().join(filename);
        fs::write(&path, contents).unwrap();
        let query = McpQuery {
            harness: Some(harness.id().into()),
            config_home: Some(temp.path().into()),
            cwd: Some(project.clone()),
            ..Default::default()
        };
        let mut install = mutation(&query, "install", None);
        install.name = Some("fixture".into());
        install.definition = Some(definition());
        let entry = harness.mcp_mutate(&install).await.unwrap().unwrap();
        assert_eq!(entry.definition.env_vars, vec!["MCP_TOKEN"]);
        let disabled = harness
            .mcp_mutate(&mutation(&query, "disable", Some(&entry)))
            .await
            .unwrap()
            .unwrap();
        assert!(!disabled.enabled);
        assert!(matches!(
            harness
                .mcp_mutate(&mutation(&query, "enable", Some(&entry)))
                .await,
            Err(Error::Rejected(_))
        ));
        let enabled = harness
            .mcp_mutate(&mutation(&query, "enable", Some(&disabled)))
            .await
            .unwrap()
            .unwrap();
        let mut update = mutation(&query, "update", Some(&enabled));
        let mut changed = definition();
        changed.command = Some("updated-server".into());
        update.definition = Some(changed);
        let updated = harness.mcp_mutate(&update).await.unwrap().unwrap();
        assert_eq!(
            updated.definition.command.as_deref(),
            Some(std::path::Path::new("updated-server"))
        );
        harness
            .mcp_mutate(&mutation(&query, "uninstall", Some(&updated)))
            .await
            .unwrap();
        assert!(harness.mcp_catalog(&query).await.unwrap().is_empty());
        let saved = fs::read_to_string(&path).unwrap();
        assert!(saved.contains(if harness.id() == "codex" {
            "keep-model"
        } else {
            "keep-value"
        }));
        if matches!(harness.id(), "codex" | "opencode") {
            assert!(saved.contains("user comment"));
        }
        assert!(project.is_dir());
    }
}

#[tokio::test]
async fn native_credentials_are_hidden_and_preserved_on_edit() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("opencode.jsonc");
    fs::write(&path,"{ // keep comment\n\"mcp\":{\"fixture\":{\"type\":\"remote\",\"url\":\"https://example.com/mcp?token=private-token\",\"headers\":{\"X-Secret\":\"private-header\",\"X-Env\":\"{env:MCP_HEADER}\"},\"timeout\":4321}},\"other\":true}").unwrap();
    let query = McpQuery {
        config_home: Some(temp.path().into()),
        ..Default::default()
    };
    let entry = OpenCodeHarness.mcp_catalog(&query).await.unwrap().remove(0);
    let public = serde_json::to_string(&entry).unwrap();
    assert!(!public.contains("private-token"));
    assert!(!public.contains("private-header"));
    assert_eq!(entry.definition.header_env_vars["X-Env"], "MCP_HEADER");
    let mut update = mutation(&query, "update", Some(&entry));
    let mut settings = entry.definition.clone();
    settings.header_env_vars.clear();
    update.definition = Some(settings);
    OpenCodeHarness.mcp_mutate(&update).await.unwrap();
    let saved = fs::read_to_string(path).unwrap();
    assert!(saved.contains("private-token"));
    assert!(saved.contains("private-header"));
    assert!(!saved.contains("MCP_HEADER"));
    assert!(saved.contains("keep comment"));
    assert!(saved.contains("4321"));
}

#[tokio::test]
async fn external_edits_reject_stale_actions_and_duplicate_keys_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("mcp.json");
    fs::write(
        &path,
        "{\"mcpServers\":{\"fixture\":{\"command\":\"server\"}}}",
    )
    .unwrap();
    let query = McpQuery {
        config_home: Some(temp.path().into()),
        ..Default::default()
    };
    let entry = PiHarness.mcp_catalog(&query).await.unwrap().remove(0);
    fs::write(
        &path,
        "{\"mcpServers\":{\"fixture\":{\"command\":\"new-server\"}}}",
    )
    .unwrap();
    assert!(matches!(
        PiHarness
            .mcp_mutate(&mutation(&query, "uninstall", Some(&entry)))
            .await,
        Err(Error::Rejected(_))
    ));
    assert!(fs::read_to_string(&path).unwrap().contains("new-server"));
    fs::write(path, "{\"mcpServers\":{},\"mcpServers\":{}}").unwrap();
    assert!(PiHarness.mcp_catalog(&query).await.is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn native_symlinks_and_file_permissions_survive_atomic_changes() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("shared.toml");
    let alias = temp.path().join("config.toml");
    fs::write(&target, "[mcp_servers.fixture]\ncommand = 'server'\n").unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
    symlink(&target, &alias).unwrap();
    let original = fs::metadata(&target).unwrap();
    let query = McpQuery {
        config_home: Some(temp.path().into()),
        ..Default::default()
    };
    let entry = CodexHarness.mcp_catalog(&query).await.unwrap().remove(0);
    CodexHarness
        .mcp_mutate(&mutation(&query, "disable", Some(&entry)))
        .await
        .unwrap();
    assert!(
        fs::symlink_metadata(alias)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let saved = fs::metadata(target).unwrap();
    assert_eq!(saved.permissions().mode() & 0o777, 0o640);
    assert_eq!((saved.uid(), saved.gid()), (original.uid(), original.gid()));
}

#[tokio::test]
async fn http_environment_references_roundtrip_across_native_harnesses() {
    for harness in [
        Box::new(CodexHarness) as Box<dyn Harness>,
        Box::new(ClaudeHarness),
        Box::new(OpenCodeHarness),
        Box::new(PiHarness),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let query = McpQuery {
            config_home: Some(temp.path().into()),
            ..Default::default()
        };
        let mut install = mutation(&query, "install", None);
        install.name = Some("remote".into());
        install.definition=Some(serde_json::from_value::<McpDefinition>(json!({"transport":"http","url":"https://example.com/mcp","headerEnvVars":{"X-Token":"HEADER_TOKEN"},"bearerTokenEnvVar":"BEARER_TOKEN"})).unwrap());
        let entry = harness.mcp_mutate(&install).await.unwrap().unwrap();
        assert_eq!(entry.definition.header_env_vars["X-Token"], "HEADER_TOKEN");
        assert_eq!(
            entry.definition.bearer_token_env_var.as_deref(),
            Some("BEARER_TOKEN")
        );
    }
}

fn native_options(root: &std::path::Path, config: &std::path::Path) -> SpawnOptions {
    let mut options = SpawnOptions::new("codex", root);
    options.execution.config_home = Some(config.into());
    options.execution.env.insert(
        "SWARMIE_FIXTURE_LEDGER".into(),
        config.join("ledger.json").to_string_lossy().into_owned(),
    );
    options.config = json!({"executable":"python3","args":[std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/codex_features.py")]});
    options
}

#[tokio::test]
async fn native_changes_only_require_reopening_bindings_using_the_selected_source() {
    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("first");
    let other = temp.path().join("other");
    fs::create_dir(&first).unwrap();
    fs::create_dir(&other).unwrap();
    fs::write(
        first.join("config.toml"),
        "[mcp_servers.fixture]\ncommand = 'server'\n",
    )
    .unwrap();
    fs::write(
        other.join("config.toml"),
        "[mcp_servers.fixture]\ncommand = 'other-server'\n",
    )
    .unwrap();
    let runtime = Runtime::builder().harness(CodexHarness).build();
    let a = runtime
        .spawn(native_options(temp.path(), &first))
        .await
        .unwrap();
    let b = runtime
        .spawn(native_options(temp.path(), &other))
        .await
        .unwrap();
    let query = McpQuery {
        harness: Some("codex".into()),
        config_home: Some(first.clone()),
        ..Default::default()
    };
    let entry = runtime
        .mcp_catalog(query.clone())
        .await
        .unwrap()
        .entries
        .remove(0);
    let receipt = runtime
        .mcp_mutate(mutation(&query, "disable", Some(&entry)))
        .await
        .unwrap();
    assert_eq!(receipt.restart_sessions, vec![a.snapshot().await.id]);
    a.close().await.unwrap();
    assert!(
        runtime
            .mcp_catalog(query.clone())
            .await
            .unwrap()
            .restart_sessions
            .is_empty()
    );
    let mut install = mutation(&query, "install", None);
    install.origin = Some("native".into());
    install.name = Some("second-server".into());
    install.definition = Some(definition());
    let receipt = runtime.mcp_mutate(install).await.unwrap();
    assert!(receipt.restart_sessions.is_empty());
    b.close().await.unwrap();
}

#[tokio::test]
async fn native_live_inventory_is_thread_scoped_and_opaque_owners_remain_read_only() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("config");
    fs::create_dir(&config).unwrap();
    fs::write(
        config.join("config.toml"),
        "[mcp_servers.fixture]\nurl = 'https://example.test/mcp'\n",
    )
    .unwrap();
    let runtime = Runtime::builder().harness(CodexHarness).build();
    let a = runtime
        .spawn(native_options(temp.path(), &config))
        .await
        .unwrap();
    let query = McpQuery {
        session_id: Some(a.snapshot().await.id),
        ..Default::default()
    };
    let catalog = runtime.mcp_catalog(query.clone()).await.unwrap();
    assert!(
        !serde_json::to_string(&catalog)
            .unwrap()
            .contains("hidden-tool-schema")
    );
    let entry = catalog
        .entries
        .iter()
        .find(|entry| entry.name == "fixture")
        .unwrap();
    assert_eq!(entry.tools, vec!["fixture_read"]);
    assert_eq!(entry.runtime_status, "authenticationRequired");
    assert!(entry.actions.iter().any(|action| action == "authenticate"));
    let plugin = catalog
        .entries
        .iter()
        .find(|entry| entry.name == "plugin-server")
        .unwrap();
    assert!(plugin.actions.is_empty());
    assert_eq!(plugin.scope, "session");
    assert_eq!(plugin.tools, vec!["plugin_read"]);
    let receipt = runtime
        .mcp_mutate(mutation(&query, "authenticate", Some(entry)))
        .await
        .unwrap();
    assert_eq!(
        receipt.authorization_url.as_deref(),
        Some("https://auth.example.test/authorize")
    );
    a.close().await.unwrap();
}

#[tokio::test]
async fn registry_persists_across_runtime_instances_on_the_same_node() {
    let temp = tempfile::tempdir().unwrap();
    let store = storage::SqliteSessionStore::new(temp.path()).await.unwrap();
    let runtime = Runtime::builder().store(store).harness(PiHarness).build();
    let mut install = mutation(&McpQuery::default(), "install", None);
    install.name = Some("persistent-server".into());
    install.definition = Some(definition());
    let entry = runtime.mcp_mutate(install).await.unwrap().entry.unwrap();
    runtime
        .mcp_mutate(mutation(&McpQuery::default(), "disable", Some(&entry)))
        .await
        .unwrap();
    drop(runtime);
    let store = storage::SqliteSessionStore::new(temp.path()).await.unwrap();
    let runtime = Runtime::builder().store(store).build();
    let entries = runtime
        .mcp_catalog(McpQuery::default())
        .await
        .unwrap()
        .entries;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, entry.id);
    assert!(!entries[0].enabled);
}

#[tokio::test]
async fn malformed_native_values_fail_without_panics_or_exposing_opaque_urls() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("mcp.json");
    let query = McpQuery {
        config_home: Some(temp.path().into()),
        ..Default::default()
    };
    fs::write(&path, "   \n").unwrap();
    assert!(PiHarness.mcp_catalog(&query).await.is_err());
    fs::write(
        &path,
        "{\"mcpServers\":{\"fixture\":{\"url\":\"malformed-private-value\"}}}",
    )
    .unwrap();
    let entry = PiHarness.mcp_catalog(&query).await.unwrap().remove(0);
    assert!(
        !serde_json::to_string(&entry)
            .unwrap()
            .contains("malformed-private-value")
    );
    assert!(entry.credential_fields.iter().any(|field| field == "url"));
}

#[tokio::test]
async fn live_native_environment_overrides_select_the_actual_configuration_source() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("isolated-codex");
    fs::create_dir(&config).unwrap();
    fs::write(
        config.join("config.toml"),
        "[mcp_servers.fixture]\ncommand='server'\n",
    )
    .unwrap();
    let mut options = native_options(temp.path(), &config);
    options.execution.config_home = None;
    options
        .execution
        .env
        .insert("CODEX_HOME".into(), config.to_string_lossy().into_owned());
    let runtime = Runtime::builder().harness(CodexHarness).build();
    let agent = runtime.spawn(options).await.unwrap();
    let query = McpQuery {
        session_id: Some(agent.snapshot().await.id.clone()),
        ..Default::default()
    };
    let catalog = runtime.mcp_catalog(query.clone()).await.unwrap();
    let entry = catalog
        .entries
        .iter()
        .find(|entry| entry.name == "fixture" && entry.scope == "user")
        .unwrap();
    assert_eq!(
        entry.source.as_deref(),
        Some(config.join("config.toml").as_path())
    );
    let receipt = runtime
        .mcp_mutate(mutation(&query, "disable", Some(entry)))
        .await
        .unwrap();
    assert_eq!(receipt.restart_sessions, vec![agent.snapshot().await.id]);
    assert!(
        fs::read_to_string(config.join("config.toml"))
            .unwrap()
            .contains("enabled = false")
    );
    agent.close().await.unwrap();
}

#[tokio::test]
async fn external_opencode_contexts_ignore_node_registry_injection_and_own_no_native_files() {
    let root = tempfile::tempdir().unwrap();
    let runtime = Runtime::builder().harness(OpenCodeHarness).build();
    let mut install = mutation(&McpQuery::default(), "install", None);
    install.name = Some("fixture".into());
    install.definition = Some(definition());
    runtime.mcp_mutate(install).await.unwrap();
    let mut options = SpawnOptions::new("opencode", root.path());
    options.config = json!({"url":"http://127.0.0.1:1"});
    let capabilities = OpenCodeHarness.mcp_capabilities(&options).await.unwrap();
    assert_eq!(capabilities.metadata["mcpConfigurationScope"], "external");
    assert_eq!(capabilities.metadata["mcpTransports"], json!([]));
    let resolved = runtime.resolve_launch_options(options).await.unwrap();
    assert!(resolved.execution.mcp_servers.is_empty());
}

#[tokio::test]
async fn claude_uses_repository_policy_and_existing_legacy_config_without_moving_files() {
    let temp = tempfile::tempdir().unwrap();
    let project = temp.path().join("repository");
    let nested = project.join("nested");
    let config = temp.path().join("config");
    fs::create_dir_all(&nested).unwrap();
    fs::create_dir(project.join(".git")).unwrap();
    fs::create_dir(&config).unwrap();
    let source = config.join(".config.json");
    fs::write(&source, format!("{{\"mcpServers\":{{\"fixture\":{{\"command\":\"server\"}}}},\"projects\":{{{}:{{\"mcpServers\":{{\"local_fixture\":{{\"command\":\"local-server\"}}}}}}}}}}", serde_json::to_string(&project).unwrap())).unwrap();
    let query = McpQuery {
        cwd: Some(nested),
        config_home: Some(config.clone()),
        ..Default::default()
    };
    let entries = ClaudeHarness.mcp_catalog(&query).await.unwrap();
    assert!(
        entries
            .iter()
            .any(|entry| entry.name == "local_fixture" && entry.scope == "local")
    );
    let entry = entries
        .iter()
        .find(|entry| entry.name == "fixture")
        .unwrap();
    ClaudeHarness
        .mcp_mutate(&mutation(&query, "disable", Some(entry)))
        .await
        .unwrap();
    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(source).unwrap()).unwrap();
    assert_eq!(
        value["projects"][project.to_string_lossy().as_ref()]["disabledMcpServers"],
        json!(["fixture"])
    );
    assert!(!config.join(".claude.json").exists());
    assert!(
        !ClaudeHarness
            .mcp_catalog(&query)
            .await
            .unwrap()
            .into_iter()
            .find(|entry| entry.name == "fixture")
            .unwrap()
            .enabled
    );
}

#[tokio::test]
async fn claude_linked_worktrees_share_the_main_checkout_policy() {
    let temp = tempfile::tempdir().unwrap();
    let main = temp.path().join("main");
    let linked = temp.path().join("linked");
    let gitdir = main.join(".git/worktrees/linked");
    let home = temp.path().join("home");
    fs::create_dir_all(&gitdir).unwrap();
    fs::create_dir(&linked).unwrap();
    fs::create_dir_all(home.join(".claude")).unwrap();
    fs::write(
        linked.join(".git"),
        format!("gitdir: {}\n", gitdir.display()),
    )
    .unwrap();
    fs::write(gitdir.join("commondir"), "../..\n").unwrap();
    fs::write(
        gitdir.join("gitdir"),
        format!("{}\n", linked.join(".git").display()),
    )
    .unwrap();
    let source = home.join(".claude/.config.json");
    fs::write(
        &source,
        "{\"mcpServers\":{\"fixture\":{\"command\":\"server\"}}}",
    )
    .unwrap();
    let query = McpQuery {
        cwd: Some(linked.clone()),
        owner_home: Some(home.clone()),
        ..Default::default()
    };
    let entry = ClaudeHarness.mcp_catalog(&query).await.unwrap().remove(0);
    let mut disable = mutation(&query, "disable", Some(&entry));
    disable.query.owner_home = Some(home.clone());
    ClaudeHarness.mcp_mutate(&disable).await.unwrap();
    let config: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&source).unwrap()).unwrap();
    assert_eq!(
        config["projects"][main.to_string_lossy().as_ref()]["disabledMcpServers"],
        json!(["fixture"])
    );
    assert!(config["projects"][linked.to_string_lossy().as_ref()].is_null());
    assert!(!home.join(".claude.json").exists());
}

#[tokio::test]
async fn claude_project_consent_uses_the_owned_repository_and_respects_home_fallback() {
    let temp = tempfile::tempdir().unwrap();
    let repository = temp.path().join("repository");
    let nested = repository.join("nested");
    let config = temp.path().join("config");
    let home = temp.path().join("home");
    fs::create_dir_all(&nested).unwrap();
    fs::create_dir(repository.join(".git")).unwrap();
    fs::create_dir(repository.join(".claude")).unwrap();
    fs::create_dir(&config).unwrap();
    fs::create_dir(&home).unwrap();
    fs::write(
        repository.join(".mcp.json"),
        "{\"mcpServers\":{\"fixture\":{\"command\":\"server\"}}}",
    )
    .unwrap();
    fs::write(
        repository.join(".claude/settings.local.json"),
        "{\"keep\":true}",
    )
    .unwrap();
    let mut query = McpQuery {
        cwd: Some(nested.clone()),
        config_home: Some(config),
        owner_home: Some(home.clone()),
        ..Default::default()
    };
    let entry = ClaudeHarness.mcp_catalog(&query).await.unwrap().remove(0);
    let mut disable = mutation(&query, "disable", Some(&entry));
    disable.query.owner_home = Some(home.clone());
    let disabled = ClaudeHarness.mcp_mutate(&disable).await.unwrap().unwrap();
    assert!(!disabled.enabled);
    let policy: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(repository.join(".claude/settings.local.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(policy["keep"], true);
    assert_eq!(policy["disabledMcpjsonServers"], json!(["fixture"]));
    assert!(!nested.join(".claude").exists());
    let mut enable = mutation(&query, "enable", Some(&disabled));
    enable.query.owner_home = Some(home);
    assert!(
        ClaudeHarness
            .mcp_mutate(&enable)
            .await
            .unwrap()
            .unwrap()
            .enabled
    );

    query.owner_home = Some(repository.clone());
    let entry = ClaudeHarness.mcp_catalog(&query).await.unwrap().remove(0);
    let mut disable = mutation(&query, "disable", Some(&entry));
    disable.query.owner_home = Some(repository.clone());
    ClaudeHarness.mcp_mutate(&disable).await.unwrap();
    let policy: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(nested.join(".claude/settings.local.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(policy["disabledMcpjsonServers"], json!(["fixture"]));
    let root_policy: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(repository.join(".claude/settings.local.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(root_policy["disabledMcpjsonServers"], json!([]));
}

#[tokio::test]
async fn claude_legacy_consent_remains_visible_and_cannot_be_falsely_enabled() {
    let temp = tempfile::tempdir().unwrap();
    let nested = temp.path().join("nested");
    fs::create_dir_all(nested.join(".claude")).unwrap();
    fs::create_dir(temp.path().join(".git")).unwrap();
    fs::write(
        temp.path().join(".mcp.json"),
        "{\"mcpServers\":{\"fixture\":{\"command\":\"server\"}}}",
    )
    .unwrap();
    let legacy = nested.join(".claude/settings.local.json");
    let original = "{\"disabledMcpjsonServers\":[\"fixture\"],\"keep\":true}";
    fs::write(&legacy, original).unwrap();
    let query = McpQuery {
        cwd: Some(nested),
        config_home: Some(temp.path().join("config")),
        ..Default::default()
    };
    let entry = ClaudeHarness.mcp_catalog(&query).await.unwrap().remove(0);
    assert!(!entry.enabled);
    assert!(!entry.actions.iter().any(|action| action == "enable"));
    assert!(
        ClaudeHarness
            .mcp_mutate(&mutation(&query, "enable", Some(&entry)))
            .await
            .is_err()
    );
    assert_eq!(fs::read_to_string(&legacy).unwrap(), original);
    fs::write(&legacy, "{\"disabledMcpjsonServers\":[]}").unwrap();
    assert!(
        ClaudeHarness
            .mcp_mutate(&mutation(&query, "disable", Some(&entry)))
            .await
            .is_err()
    );
    assert!(
        ClaudeHarness
            .mcp_catalog(&query)
            .await
            .unwrap()
            .remove(0)
            .enabled
    );
}
