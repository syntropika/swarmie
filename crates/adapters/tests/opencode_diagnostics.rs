#![cfg(unix)]
use adapters::OpenCodeHarness;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::Duration,
};
use swarmie::*;

struct Fixture {
    path: PathBuf,
    child: Option<Child>,
}
impl Fixture {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("swarmie-http-diagnostics-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        Self { path, child: None }
    }
    fn options(&self, budget: u64) -> SpawnOptions {
        let mut options = SpawnOptions::new("opencode", &self.path);
        options.config = json!({"executable":PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/opencode_diagnostics.py"),"diagnostics":{"stallTimeoutMs":40,"recording":{"path":self.path.join("wire.jsonl"),"maxBytes":budget}}});
        options
    }
    async fn external(&mut self) -> String {
        let ready = self.path.join("ready");
        self.child = Some(
            Command::new("python3")
                .arg(
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("tests/fixtures/opencode_diagnostics.py"),
                )
                .arg("--ready")
                .arg(&ready)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        tokio::time::timeout(Duration::from_secs(5), async {
            while !ready.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        format!(
            "http://127.0.0.1:{}?credential=external-query-secret",
            std::fs::read_to_string(ready).unwrap()
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
fn diagnostic(event: &BackendEvent, code: &str) -> bool {
    matches!(event, BackendEvent::Content { kind, .. } if matches!(&**kind, EventKind::Diagnostic(data) if data["code"] == code))
}
async fn idle_events(connection: &mut BackendConnection) -> Vec<BackendEvent> {
    let mut events = vec![];
    let deadline = tokio::time::Instant::now() + Duration::from_millis(120);
    while let Ok(Some(event)) = tokio::time::timeout_at(deadline, connection.events.recv()).await {
        events.push(event);
    }
    events
}
async fn finish(connection: &mut BackendConnection) -> Vec<BackendEvent> {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut events = vec![];
        while let Some(event) = connection.events.recv().await {
            assert!(!matches!(event, BackendEvent::Exited { .. }), "unexpected exit: {event:?}");
            assert!(!matches!(&event, BackendEvent::Content {kind,..} if matches!(&**kind,EventKind::Error{..})), "diagnostic became error");
            let terminal = matches!(event, BackendEvent::TurnFinished { .. });
            events.push(event);
            if terminal { return events; }
        }
        panic!("event stream closed before terminal");
    }).await.unwrap()
}
fn path_from(events: &[BackendEvent]) -> PathBuf {
    events
        .iter()
        .find_map(|event| match event {
            BackendEvent::Content { kind, .. } => match &**kind {
                EventKind::Diagnostic(data) if data["code"] == "wire_recording_started" => {
                    data["path"].as_str().map(PathBuf::from)
                }
                _ => None,
            },
            _ => None,
        })
        .expect("actual recording path receipt")
}

#[tokio::test]
async fn native_http_and_sse_are_redacted_and_resume_never_overwrites() {
    let fixture = Fixture::new();
    let mut options = fixture.options(1024 * 1024);
    options
        .execution
        .env
        .insert("DIAGNOSTIC_ENV".into(), "opaque-env-value".into());
    options.execution.mcp_servers = vec![
        McpServer {
            name: "stdio".into(),
            transport: McpTransport::Stdio {
                command: "unused".into(),
                args: vec![],
                env: BTreeMap::from([("ODD".into(), "opaque-stdio-value".into())]),
            },
        },
        McpServer {
            name: "remote".into(),
            transport: McpTransport::Http {
                url: "http://localhost/mcp".into(),
                headers: BTreeMap::from([("X-Odd".into(), "opaque-header-value".into())]),
            },
        },
    ];
    let mut connection = OpenCodeHarness
        .connect(&options, None, false)
        .await
        .unwrap();
    let idle = idle_events(&mut connection).await;
    assert!(
        !idle.iter().any(|event| diagnostic(event, "wire_stalled")),
        "idle SSE must not stall"
    );
    let path = path_from(&idle);
    connection
        .backend
        .prompt(&Prompt::from(
            "echo opaque-env-value opaque-header-value opaque-stdio-value",
        ))
        .await
        .unwrap();
    let events = finish(&mut connection).await;
    assert_eq!(
        events
            .iter()
            .filter(|event| diagnostic(event, "wire_stalled"))
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| diagnostic(event, "wire_resumed"))
            .count(),
        1
    );
    assert!(
        matches!(events.last(),Some(BackendEvent::TurnFinished { outcome, .. }) if outcome.as_str() == "completed")
    );
    assert!(
        !idle_events(&mut connection)
            .await
            .iter()
            .any(|event| diagnostic(event, "wire_stalled"))
    );
    let native = connection.native.clone();
    connection.backend.close().await.unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    for secret in [
        "opaque-env-value",
        "opaque-stdio-value",
        "opaque-header-value",
        "embedded-password",
        "secret-query",
    ] {
        assert!(!text.contains(secret), "leaked {secret}");
    }
    let auth: Value = serde_json::from_str(
        &std::fs::read_to_string(fixture.path.join("test-auth.json")).unwrap(),
    )
    .unwrap();
    assert!(!text.contains(auth["password"].as_str().unwrap()));
    assert!(!text.contains(auth["encoded"].as_str().unwrap()));
    let records = text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(
        records
            .iter()
            .any(|r| r["direction"] == "send" && r["frame"]["method"] == "POST")
    );
    assert!(
        records
            .iter()
            .any(|r| r["direction"] == "receive" && r["frame"]["status"] == 200)
    );
    assert!(
        records.iter().any(|r| r["frame"]["transport"] == "sse"
            && r["frame"]["event"]["type"] == "message.part.delta")
    );
    assert!(
        records
            .iter()
            .filter(|r| r["frame"]["transport"] == "http")
            .all(|r| r["frame"].get("headers").is_none())
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let mut resumed = OpenCodeHarness
        .connect(&options, Some(&native), false)
        .await
        .unwrap();
    let resumed_path = path_from(&idle_events(&mut resumed).await);
    assert_ne!(path, resumed_path);
    assert_eq!(text, std::fs::read_to_string(path).unwrap());
    resumed.backend.close().await.unwrap();
}

#[tokio::test]
async fn external_authenticated_server_records_without_auth_or_query_credentials() {
    let mut fixture = Fixture::new();
    let url = fixture.external().await;
    let mut options = fixture.options(1024 * 1024);
    options.config["url"] = json!(url);
    options.config["username"] = json!("test-user");
    options.config["password"] = json!("external-password");
    let mut connection = OpenCodeHarness
        .connect(&options, None, false)
        .await
        .unwrap();
    let path = path_from(&idle_events(&mut connection).await);
    connection
        .backend
        .prompt(&Prompt::from("external-query-secret external-password"))
        .await
        .unwrap();
    finish(&mut connection).await;
    connection.backend.close().await.unwrap();
    let text = std::fs::read_to_string(path).unwrap();
    for secret in [
        "external-password",
        "external-query-secret",
        "dGVzdC11c2VyOmV4dGVybmFsLXBhc3N3b3Jk",
    ] {
        assert!(!text.contains(secret), "leaked {secret}");
    }
    assert!(text.contains("[REDACTED]"));
}

#[tokio::test]
async fn recording_budget_and_open_failure_warn_once_without_failing_provider() {
    for failed_open in [false, true] {
        let fixture = Fixture::new();
        let mut options = fixture.options(120);
        if failed_open {
            options.config["diagnostics"]["recording"]["path"] =
                json!(fixture.path.join("missing/wire.jsonl"));
        }
        let mut connection = OpenCodeHarness
            .connect(&options, None, false)
            .await
            .unwrap();
        let mut events = idle_events(&mut connection).await;
        connection
            .backend
            .prompt(&Prompt::from("ready"))
            .await
            .unwrap();
        events.extend(finish(&mut connection).await);
        let code = if failed_open {
            "wire_recording_failed"
        } else {
            "wire_recording_full"
        };
        assert_eq!(
            events
                .iter()
                .filter(|event| diagnostic(event, code))
                .count(),
            1
        );
        if !failed_open {
            assert!(
                std::fs::metadata(fixture.path.join("wire.jsonl"))
                    .unwrap()
                    .len()
                    <= 120
            );
        }
        connection.backend.close().await.unwrap();
    }
}

#[tokio::test]
async fn native_http_response_is_bounded() {
    let fixture = Fixture::new();
    let options = fixture.options(1024 * 1024);
    let connection = OpenCodeHarness
        .connect(&options, None, false)
        .await
        .unwrap();
    let error = connection.backend.commands().await.unwrap_err();
    assert!(error.to_string().contains("exceeds 16 MiB"));
    connection.backend.close().await.unwrap();
}

#[tokio::test]
async fn native_sse_frame_is_bounded_without_recording_partial_payload() {
    let fixture = Fixture::new();
    let options = fixture.options(1024 * 1024);
    let mut connection = OpenCodeHarness
        .connect(&options, None, false)
        .await
        .unwrap();
    idle_events(&mut connection).await;
    connection
        .backend
        .prompt(&Prompt::from("oversized"))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(8), async {
        while let Some(event) = connection.events.recv().await {
            if let BackendEvent::Exited { message } = event {
                assert_eq!(message, "OpenCode SSE record too large");
                return;
            }
        }
        panic!("oversized SSE frame was not rejected");
    })
    .await
    .unwrap();
    let text = std::fs::read_to_string(fixture.path.join("wire.jsonl")).unwrap();
    assert!(!text.contains("xxxxxxxx"));
    connection.backend.close().await.unwrap();
}

#[tokio::test]
async fn embedded_basic_auth_is_redacted_even_when_native_payload_echoes_encoded_credentials() {
    let mut fixture = Fixture::new();
    let url = fixture
        .external()
        .await
        .replace("http://", "http://url-user:url-password@");
    let mut options = fixture.options(1024 * 1024);
    options.config["url"] = json!(url);
    let mut connection = OpenCodeHarness
        .connect(&options, None, false)
        .await
        .unwrap();
    let path = path_from(&idle_events(&mut connection).await);
    connection
        .backend
        .prompt(&Prompt::from("ready"))
        .await
        .unwrap();
    finish(&mut connection).await;
    connection.backend.close().await.unwrap();
    let text = std::fs::read_to_string(path).unwrap();
    for secret in [
        "url-password",
        "dXJsLXVzZXI6dXJsLXBhc3N3b3Jk",
        "external-query-secret",
    ] {
        assert!(!text.contains(secret), "leaked {secret}");
    }
}
