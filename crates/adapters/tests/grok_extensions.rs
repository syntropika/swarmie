#![cfg(unix)]
use adapters::AcpHarness;
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};
use swarmie::*;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("swarmie-grok-extensions-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    async fn connect(&self, harness: &str, mode: &str) -> BackendConnection {
        let mut options = SpawnOptions::new(harness, &self.0);
        options.config = json!({"executable":"python3","args":[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/grok_extensions.py"),mode]});
        AcpHarness::new(harness, "python3", vec![])
            .connect(&options, None, false)
            .await
            .unwrap()
    }
    fn requests(&self) -> Vec<Value> {
        std::fs::read_to_string(self.0.join("requests.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn qualified_grok_native_operations_preserve_scopes_and_task_ownership() {
    let fixture = Fixture::new();
    let mut connection = fixture.connect("grok", "normal").await;
    assert!(connection.capabilities.steering.is_available());
    assert!(connection.capabilities.rollback.is_available());
    assert!(connection.capabilities.background_tasks.is_available());
    assert_eq!(
        connection.capabilities.steering.source,
        CapabilitySource::Extension
    );
    assert_eq!(
        connection.backend.commands().await.unwrap()[0]["name"],
        "compact"
    );
    connection.backend.rollback(1).await.unwrap();
    connection
        .backend
        .rollback_with(2, RollbackScope::ConversationAndFiles)
        .await
        .unwrap();
    let tasks = connection.backend.tasks().await.unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].description, "Compile project");
    assert_eq!(tasks[0].status, AgentTaskStatus::Running);
    assert!(
        !serde_json::to_string(&tasks)
            .unwrap()
            .contains("private/native-output")
    );
    assert!(connection.backend.stop_task("sibling").await.is_err());
    connection.backend.stop_task("own").await.unwrap();
    assert_eq!(
        connection.backend.tasks().await.unwrap()[0].status,
        AgentTaskStatus::Stopped
    );
    let requests = fixture.requests();
    let rewinds: Vec<_> = requests
        .iter()
        .filter(|frame| frame["method"] == "_x.ai/rewind/execute")
        .collect();
    assert_eq!(rewinds[0]["params"]["targetPromptIndex"], 2);
    assert_eq!(rewinds[0]["params"]["mode"], "conversation_only");
    assert_eq!(rewinds[1]["params"]["targetPromptIndex"], 1);
    assert_eq!(rewinds[1]["params"]["mode"], "all");
    assert_eq!(
        requests
            .iter()
            .filter(|frame| frame["method"] == "_x.ai/task/kill")
            .count(),
        1
    );
    let event = tokio::time::timeout(Duration::from_secs(2), connection.events.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(
        matches!(event,BackendEvent::Content {kind,..} if matches!(*kind,EventKind::TaskUpdated(AgentTask {status:AgentTaskStatus::Stopped,..})))
    );
    connection.backend.close().await.unwrap();
}

#[tokio::test]
async fn generic_unmarked_and_unreviewed_agents_never_receive_vendor_calls() {
    for (harness, mode) in [
        ("generic", "normal"),
        ("grok", "no_marker"),
        ("grok", "future"),
    ] {
        let fixture = Fixture::new();
        let connection = fixture.connect(harness, mode).await;
        assert!(!connection.capabilities.steering.is_available());
        assert!(!connection.capabilities.background_tasks.is_available());
        assert!(
            connection
                .backend
                .steer(&Prompt::from("change direction"))
                .await
                .is_err()
        );
        assert!(connection.backend.rollback(1).await.is_err());
        assert!(connection.backend.tasks().await.is_err());
        assert!(connection.backend.stop_task("own").await.is_err());
        assert!(!fixture.requests().iter().any(|frame| {
            frame["method"]
                .as_str()
                .is_some_and(|method| method.starts_with("_x.ai/"))
        }));
        connection.backend.close().await.unwrap();
    }
}

#[tokio::test]
async fn steering_is_native_interjection_and_usage_cost_respects_partial_bills() {
    for (mode, expected) in [("normal", Some(0.25)), ("partial", None)] {
        let fixture = Fixture::new();
        let mut connection = fixture.connect("grok", mode).await;
        connection
            .backend
            .prompt(&Prompt::from("fixture active turn"))
            .await
            .unwrap();
        connection
            .backend
            .steer(&Prompt::from("change direction"))
            .await
            .unwrap();
        let reported = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let BackendEvent::Content { kind, .. } = connection.events.recv().await.unwrap()
                    && let EventKind::UsageUpdated(usage) = *kind
                {
                    break usage;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(reported.cost_usd, expected);
        assert_eq!(reported.cached_input_tokens, Some(20));
        assert_eq!(reported.cache_write_tokens, Some(10));
        let requests = fixture.requests();
        assert_eq!(
            requests
                .iter()
                .filter(|frame| frame["method"] == "session/prompt")
                .count(),
            1
        );
        assert_eq!(
            requests
                .iter()
                .find(|frame| frame["method"] == "_x.ai/interject")
                .unwrap()["params"]["text"],
            "change direction"
        );
        assert!(matches!(
            connection.backend.rollback(1).await,
            Err(Error::Busy)
        ));
        connection.backend.cancel().await.unwrap();
        connection.backend.close().await.unwrap();
    }
}

#[tokio::test]
async fn invalid_checkpoint_counts_and_native_conflicts_do_not_report_success() {
    let fixture = Fixture::new();
    let connection = fixture.connect("grok", "conflict").await;
    assert!(connection.backend.rollback(0).await.is_err());
    assert!(connection.backend.rollback(4).await.is_err());
    assert!(
        !fixture
            .requests()
            .iter()
            .any(|frame| frame["method"] == "_x.ai/rewind/execute")
    );
    assert!(
        connection
            .backend
            .rollback_with(1, RollbackScope::ConversationAndFiles)
            .await
            .is_err()
    );
    let response = fixture
        .requests()
        .into_iter()
        .find(|frame| frame["method"] == "_x.ai/rewind/execute")
        .unwrap();
    assert_eq!(response["params"]["force"], false);
    connection.backend.close().await.unwrap();
}
