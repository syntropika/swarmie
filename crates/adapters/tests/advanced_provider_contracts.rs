#![cfg(unix)]

use adapters::{AcpHarness, OpenCodeHarness, PiHarness};
use swarmie::*;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

// Independent native wire peer. Receipts assert what crosses the process/API boundary.

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let folder = std::env::temp_dir().join(format!("swarmie-contract-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&folder).unwrap();
        Self(folder)
    }
    fn options(&self, provider: &str) -> SpawnOptions {
        let mut options = SpawnOptions::new(provider, &self.0);
        options.config = if provider == "opencode" {
            json!({"executable":PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/advanced_agent.py")})
        } else {
            json!({"executable":"python3","args":[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/advanced_agent.py"),provider]})
        };
        options
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
async fn finish(connection: &mut BackendConnection) -> (Value, Vec<EventKind>) {
    tokio::time::timeout(Duration::from_secs(8), async {
        let mut text = String::new();
        let mut events = vec![];
        while let Some(event) = connection.events.recv().await {
            match event {
                BackendEvent::Content { kind, .. } => {
                    if let EventKind::TextDelta { text: delta, .. } = &*kind {
                        text.push_str(delta);
                    }
                    events.push(*kind);
                }
                BackendEvent::TurnFinished { .. } => break,
                BackendEvent::Exited { message } => panic!("peer exited: {message}"),
                _ => (),
            }
        }
        (serde_json::from_str(&text).unwrap(), events)
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn pi_applies_native_execution_flags_environment_and_authoritative_context() {
    let fixture = Fixture::new();
    let mut options = fixture.options("pi");
    options
        .execution
        .env
        .insert("SWARMIE_TEST_ENV".into(), "isolated".into());
    options.execution.config_home = Some(fixture.0.join("pi-home"));
    options.execution.tools = ToolPolicy::Disabled;
    options.execution.ephemeral = true;
    let mut connection = PiHarness.connect(&options, None, false).await.unwrap();
    assert!(!connection.capabilities.resume.is_available());
    assert!(!connection.capabilities.fork.is_available());
    assert!(connection.capabilities.tools_disabled.is_available());
    connection
        .backend
        .prompt(&Prompt::from("receipt"))
        .await
        .unwrap();
    let (receipt, events) = finish(&mut connection).await;
    assert_eq!(receipt["env"], "isolated");
    assert!(receipt["piHome"].as_str().unwrap().ends_with("pi-home"));
    for flag in ["--no-tools", "--no-extensions", "--no-session"] {
        assert!(receipt["args"].as_array().unwrap().contains(&json!(flag)));
    }
    assert!(events.iter().any(|event| matches!(event, EventKind::ContextUpdated(c) if c.used_tokens == Some(240) && c.context_window_tokens == Some(1000))));
    assert_eq!(
        connection.backend.commands().await.unwrap()[0]["name"],
        "skill:review"
    );
    assert_eq!(
        connection.backend.commands().await.unwrap()[0]["invocation"],
        "/skill:review "
    );
    connection.backend.close().await.unwrap();
}

#[tokio::test]
async fn pi_clone_and_message_fork_return_new_native_references() {
    let fixture = Fixture::new();
    let options = fixture.options("pi");
    let saved = NativeSessionRef {
        provider: "pi".into(),
        id: "original-session".into(),
    };
    let clone = PiHarness
        .connect(&options, Some(&saved), true)
        .await
        .unwrap();
    assert_eq!(clone.native.id, "cloned-session");
    clone.backend.close().await.unwrap();
    let fork = PiHarness
        .fork_before_user(&options, &saved, "entry-7")
        .await
        .unwrap();
    assert_eq!(fork.native.id, "fork:entry-7");
    assert!(!fork.capabilities.fork_at_message.is_available());
    assert!(matches!(
        PiHarness.fork_at(&options, &saved, "entry-7").await,
        Err(Error::Unsupported(_))
    ));
    assert_eq!(saved.id, "original-session");
    fork.backend.close().await.unwrap();
}

#[tokio::test]
async fn unsupported_execution_options_never_silently_start_a_provider() {
    let fixture = Fixture::new();
    let server = McpServer {
        name: "fixture".into(),
        transport: McpTransport::Http {
            url: "http://127.0.0.1:1234/mcp".into(),
            headers: BTreeMap::new(),
        },
    };
    let mut pi = fixture.options("pi");
    pi.execution.mcp_servers.push(server.clone());
    assert!(matches!(
        PiHarness.connect(&pi, None, false).await,
        Err(Error::Unsupported(_))
    ));
    pi.execution.mcp_servers.clear();
    pi.execution.output_schema = Some(json!({"type":"object"}));
    assert!(matches!(
        PiHarness.connect(&pi, None, false).await,
        Err(Error::Unsupported(_))
    ));
    let acp = AcpHarness::new("acp", "python3", vec![]);
    for field in ["home", "tools", "schema", "ephemeral"] {
        let mut options = fixture.options("acp");
        match field {
            "home" => options.execution.config_home = Some(fixture.0.join("unsupported")),
            "tools" => options.execution.tools = ToolPolicy::Disabled,
            "schema" => options.execution.output_schema = Some(json!({"type":"object"})),
            _ => options.execution.ephemeral = true,
        }
        assert!(
            matches!(
                acp.connect(&options, None, false).await,
                Err(Error::Unsupported(_))
            ),
            "{field}"
        );
    }
    let mut open = fixture.options("opencode");
    open.config = json!({"url":"http://127.0.0.1:1"});
    open.execution
        .env
        .insert("SWARMIE_TEST_ENV".into(), "cannot-inject".into());
    assert!(matches!(
        OpenCodeHarness.connect(&open, None, false).await,
        Err(Error::Unsupported(_))
    ));
    open.execution.env.clear();
    open.execution.ephemeral = true;
    assert!(matches!(
        OpenCodeHarness.connect(&open, None, false).await,
        Err(Error::Unsupported(_))
    ));
    open.execution.ephemeral = false;
    open.execution.tools = ToolPolicy::Disabled;
    assert!(matches!(
        OpenCodeHarness.connect(&open, None, false).await,
        Err(Error::Unsupported(_))
    ));
}

#[tokio::test]
async fn acp_negotiates_mcp_and_retains_sparse_tool_commands_and_context_updates() {
    let fixture = Fixture::new();
    let harness = AcpHarness::new("acp", "python3", vec![]);
    let mut options = fixture.options("acp");
    options.execution.mcp_servers.push(McpServer {
        name: "remote".into(),
        transport: McpTransport::Http {
            url: "http://127.0.0.1:1234/mcp".into(),
            headers: BTreeMap::from([("X-Test".into(), "fixture".into())]),
        },
    });
    assert!(matches!(
        harness.connect(&options, None, false).await,
        Err(Error::Unsupported(_))
    ));
    options.config = fixture.options("acp-http").config;
    options.execution.mcp_servers.push(McpServer {
        name: "local".into(),
        transport: McpTransport::Stdio {
            command: "/bin/cat".into(),
            args: vec!["--help".into()],
            env: BTreeMap::from([("KEY".into(), "value".into())]),
        },
    });
    options
        .execution
        .env
        .insert("SWARMIE_TEST_ENV".into(), "acp-env".into());
    let mut connection = harness.connect(&options, None, false).await.unwrap();
    assert_eq!(
        connection.capabilities.metadata["mcpTransports"],
        json!(["stdio", "http"])
    );
    connection
        .backend
        .prompt(&Prompt::from("receipt"))
        .await
        .unwrap();
    let (receipt, events) = finish(&mut connection).await;
    assert_eq!(receipt["env"], "acp-env");
    assert!(
        events
            .iter()
            .any(|event| matches!(event, EventKind::UsageUpdated(value)
        if value.scope == "call" && value.input_tokens == Some(100)
            && value.output_tokens == Some(30) && value.cached_input_tokens == Some(20)
            && value.total_tokens == Some(130)))
    );
    assert!(events.iter().any(|event| matches!(event, EventKind::ToolCompleted(value)
        if value.output == "contents" && value.metadata["content"][0]["content"]["text"] == "contents")));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, EventKind::TextDelta { text, .. } if text.is_empty()))
    );
    assert_eq!(
        receipt["mcp"][0]["headers"],
        json!([{"name":"X-Test","value":"fixture"}])
    );
    assert_eq!(
        receipt["mcp"][1]["env"],
        json!([{"name":"KEY","value":"value"}])
    );
    assert_eq!(
        connection.backend.commands().await.unwrap()[0]["name"],
        "review"
    );
    assert!(events.iter().any(|e|matches!(e,EventKind::ToolCompleted(tool) if tool.name.as_deref()==Some("Read file") && tool.input["path"]=="hello.txt" && tool.output=="contents" && tool.metadata["kind"]=="read")));
    assert!(events.iter().any(|e|matches!(e,EventKind::ContextUpdated(c) if c.used_tokens==Some(400) && c.context_window_tokens==Some(2000))));
    connection.backend.close().await.unwrap();
}

#[tokio::test]
async fn opencode_applies_native_mcp_schema_context_and_explicit_filesystem_rollback() {
    let fixture = Fixture::new();
    let mut options = fixture.options("opencode");
    options.model = Some("fixture/model".into());
    options
        .execution
        .env
        .insert("SWARMIE_TEST_ENV".into(), "open-env".into());
    options.execution.config_home = Some(fixture.0.join("open-home"));
    options.execution.output_schema =
        Some(json!({"type":"object","properties":{"ok":{"type":"boolean"}}}));
    options.execution.mcp_servers.push(McpServer {
        name: "local".into(),
        transport: McpTransport::Stdio {
            command: "/bin/cat".into(),
            args: vec!["--help".into()],
            env: BTreeMap::from([("KEY".into(), "value".into())]),
        },
    });
    let mut connection = OpenCodeHarness
        .connect(&options, None, false)
        .await
        .unwrap();
    assert_eq!(
        connection.backend.commands().await.unwrap()[0]["name"],
        "review"
    );
    assert!(matches!(
        connection.backend.rollback(1).await,
        Err(Error::Unsupported(_))
    ));
    connection
        .backend
        .rollback_with(2, RollbackScope::ConversationAndFiles)
        .await
        .unwrap();
    connection.backend.compact().await.unwrap();
    connection
        .backend
        .prompt(&Prompt::from("receipt"))
        .await
        .unwrap();
    let (receipt, events) = finish(&mut connection).await;
    assert_eq!(receipt["env"], "open-env");
    assert!(
        receipt["openCodeHome"]
            .as_str()
            .unwrap()
            .ends_with("open-home")
    );
    assert_eq!(
        receipt["inline"]["mcp"]["local"]["command"],
        json!(["/bin/cat", "--help"])
    );
    assert_eq!(
        receipt["request"]["format"]["schema"],
        options.execution.output_schema.unwrap()
    );
    let requests = receipt["requests"].as_array().unwrap();
    assert!(
        requests
            .iter()
            .any(|r| r["path"].as_str().unwrap().ends_with("/revert")
                && r["body"]["messageID"] == "user-1")
    );
    assert!(
        requests
            .iter()
            .any(|r| r["path"].as_str().unwrap().ends_with("/summarize")
                && r["body"]["modelID"] == "model")
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e,EventKind::StructuredOutput(v) if v["ok"]==true))
    );
    assert!(events.iter().any(|e|matches!(e,EventKind::ContextUpdated(c) if c.used_tokens==Some(160) && c.context_window_tokens==Some(2000))));
    connection.backend.close().await.unwrap();
    let saved = NativeSessionRef {
        provider: "opencode".into(),
        id: "original-session".into(),
    };
    let mut options = fixture.options("opencode");
    options.model = Some("fixture/model".into());
    let fork = OpenCodeHarness
        .fork_at(&options, &saved, "message-9")
        .await
        .unwrap();
    assert_eq!(fork.native.id, "fork:user-2");
    assert_eq!(
        fork.capabilities.metadata["messageIdAliases"]["message-9"],
        "fork:user-2:cloned-1"
    );
    assert!(
        fork.capabilities.metadata["messageIdAliases"]
            .get("message-10")
            .is_none()
    );
    fork.backend.close().await.unwrap();
    let last = OpenCodeHarness
        .fork_at(&options, &saved, "message-10")
        .await
        .unwrap();
    assert_eq!(last.native.id, "fork:latest");
    last.backend.close().await.unwrap();
    assert!(matches!(
        OpenCodeHarness.fork_at(&options, &saved, "user-1").await,
        Err(Error::Invalid(_))
    ));
}

#[tokio::test]
async fn opencode_composes_native_fork_ids_and_rejects_divergent_clones() {
    let fixture = Fixture::new();
    let mut options = fixture.options("opencode");
    let saved = NativeSessionRef {
        provider: "opencode".into(),
        id: "original-session".into(),
    };
    let cloned = OpenCodeHarness
        .connect(&options, Some(&saved), true)
        .await
        .unwrap();
    let aliases = cloned.capabilities.metadata["messageIdAliases"].clone();
    assert_eq!(aliases["message-9"], "fork:latest:cloned-1");
    let cloned_session = cloned.native.clone();
    cloned.backend.close().await.unwrap();
    options.config["_runtimeMessageIdAliases"] = aliases.clone();
    let resumed = OpenCodeHarness
        .connect(&options, Some(&cloned_session), false)
        .await
        .unwrap();
    assert_eq!(resumed.capabilities.metadata["messageIdAliases"], aliases);
    resumed.backend.close().await.unwrap();
    let branch = OpenCodeHarness
        .fork_at(
            &options,
            &cloned_session,
            aliases["message-9"].as_str().unwrap(),
        )
        .await
        .unwrap();
    let mapped = &branch.capabilities.metadata["messageIdAliases"];
    assert_eq!(mapped["message-9"], "fork:fork:latest:cloned-2:cloned-1");
    assert_eq!(mapped["fork:latest:cloned-1"], mapped["message-9"]);
    assert!(mapped.get("message-10").is_none());
    branch.backend.close().await.unwrap();
    options.config["_runtimeMessageIdAliases"] = json!({"message-9":"missing-native-id"});
    assert!(matches!(
        OpenCodeHarness.connect(&options, Some(&cloned_session), false).await,
        Err(Error::Provider(message)) if message.contains("diverge from native history")
    ));
    options.config["_runtimeMessageIdAliases"] = Value::Null;
    options
        .execution
        .env
        .insert("SWARMIE_TEST_BAD_FORK".into(), "1".into());
    assert!(matches!(
        OpenCodeHarness.connect(&options, Some(&saved), true).await,
        Err(Error::Provider(message)) if message.contains("selected native prefix")
    ));
}

#[tokio::test]
async fn opencode_native_variants_follow_selected_models_and_reset_incompatible_choices() {
    let fixture = Fixture::new();
    let mut options = fixture.options("opencode");
    options.model = Some("fixture/model".into());
    options.config["effort"] = json!("high");
    let mut connection = OpenCodeHarness
        .connect(&options, None, false)
        .await
        .unwrap();
    let models = json!(connection.backend.models().await.unwrap());
    let model = models
        .as_array()
        .unwrap()
        .iter()
        .find(|model| model["id"] == "fixture/model")
        .unwrap();
    assert_eq!(model["metadata"]["variants"], json!(["high", "low"]));
    let descriptors = json!(connection.backend.configuration().await.unwrap());
    assert_eq!(descriptors[1]["id"], "effort");
    assert_eq!(descriptors[1]["value"], "high");
    connection
        .backend
        .prompt(&Prompt::from("receipt"))
        .await
        .unwrap();
    assert_eq!(
        finish(&mut connection).await.0["request"]["variant"],
        "high"
    );
    assert!(matches!(
        connection.backend.configure(json!({"effort":"max"})).await,
        Err(Error::Invalid(_))
    ));
    let changed = connection
        .backend
        .configure(json!({"model":"fixture/other","effort":"max"}))
        .await
        .unwrap();
    assert_eq!(changed, json!({"model":"fixture/other","effort":"max"}));
    assert!(matches!(
        connection
            .backend
            .configure(json!({"model":"fixture/plain","effort":"high"}))
            .await,
        Err(Error::Invalid(_))
    ));
    assert!(matches!(
        connection
            .backend
            .configure(json!({"model":"fixture/missing"}))
            .await,
        Err(Error::Invalid(_))
    ));
    connection
        .backend
        .prompt(&Prompt::from("receipt"))
        .await
        .unwrap();
    let receipt = finish(&mut connection).await.0;
    assert_eq!(receipt["request"]["model"]["modelID"], "other");
    assert_eq!(receipt["request"]["variant"], "max");
    assert_eq!(
        json!(connection.backend.configuration().await.unwrap())[0]["value"],
        "fixture/other"
    );
    let reset = connection
        .backend
        .configure(json!({"model":"fixture/model"}))
        .await
        .unwrap();
    assert_eq!(reset["effort"], Value::Null);
    connection
        .backend
        .prompt(&Prompt::from("receipt"))
        .await
        .unwrap();
    let receipt = finish(&mut connection).await.0;
    assert_eq!(receipt["request"]["model"]["modelID"], "model");
    assert!(receipt["request"].get("variant").is_none());
    let reset = connection
        .backend
        .configure(json!({"model":"fixture/plain"}))
        .await
        .unwrap();
    assert_eq!(connection.backend.configuration().await.unwrap().len(), 1);
    let saved = connection.native.clone();
    connection.backend.close().await.unwrap();
    options.model = reset["model"].as_str().map(String::from);
    options.config["effort"] = reset["effort"].clone();
    let resumed = OpenCodeHarness
        .connect(&options, Some(&saved), false)
        .await
        .unwrap();
    assert_eq!(
        json!(resumed.backend.configuration().await.unwrap())[0]["value"],
        "fixture/plain"
    );
    resumed.backend.close().await.unwrap();
    options.config["effort"] = json!("high");
    assert!(matches!(
        OpenCodeHarness.connect(&options, None, false).await,
        Err(Error::Invalid(_))
    ));
}

#[tokio::test]
async fn opencode_permission_rejection_preserves_message_and_cancel_is_native_request_rejection() {
    let fixture = Fixture::new();
    let options = fixture.options("opencode");
    let mut connection = OpenCodeHarness
        .connect(&options, None, false)
        .await
        .unwrap();
    for (choice, data, expected) in [
        (
            "reject",
            json!({"reason":"Leave this file unchanged"}),
            json!({"reply":"reject","message":"Leave this file unchanged"}),
        ),
        (
            "cancel",
            json!({"message":"Cancel this request"}),
            json!({"reply":"reject","message":"Cancel this request"}),
        ),
        ("once", json!({}), json!({"reply":"once"})),
    ] {
        connection
            .backend
            .prompt(&Prompt::from("permission-request"))
            .await
            .unwrap();
        let request = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(BackendEvent::Content { kind, .. }) = connection.events.recv().await
                    && let EventKind::PermissionRequested(request) = *kind
                {
                    break request;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(request.metadata["cancelSemantics"], "reject_request");
        assert_eq!(
            request
                .options
                .iter()
                .find(|option| option.id == "once")
                .unwrap()
                .kind,
            "approve_once"
        );
        assert_eq!(
            request
                .options
                .iter()
                .find(|option| option.id == "always")
                .unwrap()
                .kind,
            "approve_session"
        );
        let response = PermissionResponse {
            answers: Default::default(),
            id: request.id.clone(),
            option: choice.into(),
            data,
        };
        if choice == "reject" {
            let mut invalid = response.clone();
            invalid.option = "unknown".into();
            assert!(matches!(
                connection.backend.respond_permission(&invalid).await,
                Err(Error::Invalid(_))
            ));
            invalid.option = "reject".into();
            invalid.data = json!({"reason":42});
            assert!(matches!(
                connection.backend.respond_permission(&invalid).await,
                Err(Error::Invalid(_))
            ));
        }
        connection
            .backend
            .respond_permission(&response)
            .await
            .unwrap();
        let (receipt, reason) = tokio::time::timeout(Duration::from_secs(5), async {
            let mut text = String::new();
            loop {
                match connection.events.recv().await.unwrap() {
                    BackendEvent::Content { kind, .. } => {
                        if let EventKind::TextDelta { text: delta, .. } = *kind {
                            text.push_str(&delta);
                        }
                    }
                    BackendEvent::TurnFinished { outcome, .. } => {
                        break (
                            serde_json::from_str::<Value>(&text).unwrap(),
                            outcome.to_string(),
                        );
                    }
                    _ => (),
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(receipt["permissionReply"], expected);
        assert_eq!(reason, "completed");
        assert!(matches!(
            connection.backend.respond_permission(&response).await,
            Err(Error::Invalid(_))
        ));
    }
    connection.backend.close().await.unwrap();
}
