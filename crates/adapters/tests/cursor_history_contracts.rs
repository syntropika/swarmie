#![cfg(unix)]
use adapters::CursorHarness;
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};
use swarmie::*;
struct Fixture {
    root: PathBuf,
    source: PathBuf,
    overlay: AgentToolExecutionOverlay,
    query: NativeHistoryQuery,
}
impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("swarmie-cursor-history-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("source")).unwrap();
        std::fs::write(root.join("source/index.db"), b"native SDK fixture index").unwrap();
        let source = root.join("history.json");
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cursor_sdk_bridge.py");
        let launcher = root.join("bridge");
        let quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
        std::fs::write(
            &launcher,
            format!(
                "#!/bin/sh\nexec python3 {} --workspace {} --history-fixture {} \"$@\"\n",
                quote(&fixture.to_string_lossy()),
                quote(&root.to_string_lossy()),
                quote(&source.to_string_lossy())
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            overlay: AgentToolExecutionOverlay {
                config: json!({"executable":launcher}),
                env: std::collections::BTreeMap::from([(
                    "CURSOR_API_KEY".into(),
                    "private-history-key-must-not-cross-rpc".into(),
                )]),
                config_home: Some(root.join("source")),
            },
            query: NativeHistoryQuery {
                harness: "cursor".into(),
                cwd: Some(root.clone()),
                config_home: None,
                limit: 2,
            },
            root,
            source,
        }
    }
    fn agent(&self, id: &str, time: &str) -> Value {
        json!({"agentId":id,"name":format!("History {id}"),"lastModified":time,"local":{"cwd":self.root},"status":"AGENT_INFO_STATUS_FINISHED"})
    }
    fn data(&self) -> Value {
        let a = self.agent("agent-a", "2026-10-14T10:00:00Z");
        let b = self.agent("agent-b", "2026-10-14T11:00:00Z");
        let other = json!({"agentId":"agent-other","local":{"cwd":self.root.join("another")},"name":"Different workspace"});
        json!({"pages":{"":{"items":[other,a],"nextCursor":"second"},"second":{"items":[{"agentId":"bc-cloud","cloud":{"repos":[]}},b]}},"agents":{"agent-a":a,"agent-b":b},"messages":{}})
    }
    fn save(&self, data: &Value) {
        std::fs::write(&self.source, serde_json::to_vec(data).unwrap()).unwrap();
    }
    fn requests(&self) -> Vec<Value> {
        std::fs::read_to_string(self.root.join("requests.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
    fn readonly(&self) {
        let rows = self.requests();
        assert!(rows.iter().all(|row| matches!(
            row["method"].as_str(),
            Some(
                "Ping"
                    | "GetVersion"
                    | "ListAgents"
                    | "GetAgent"
                    | "ListAgentMessages"
                    | "Shutdown"
            )
        )));
        assert!(
            !serde_json::to_string(&rows)
                .unwrap()
                .contains("private-history-key")
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
#[tokio::test]
async fn discovery_paginates_and_scopes_sdk_agents_without_cli_or_account_actions() {
    let fixture = Fixture::new();
    let data = fixture.data();
    fixture.save(&data);
    let found = CursorHarness
        .native_history_with(&fixture.query, &fixture.overlay)
        .await
        .unwrap();
    assert_eq!(
        found
            .iter()
            .map(|session| session.native.id.as_str())
            .collect::<Vec<_>>(),
        vec!["agent-b", "agent-a"]
    );
    assert!(found.iter().all(
        |session| session.native.provider == "cursor" && session.metadata["resumable"] == true
    ));
    let requests = fixture.requests();
    let pages = requests
        .iter()
        .filter(|row| row["method"] == "ListAgents")
        .collect::<Vec<_>>();
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[1]["request"]["options"]["cursor"], "second");
    assert_eq!(
        std::fs::read(fixture.root.join("source/index.db")).unwrap(),
        b"native SDK fixture index"
    );
    fixture.readonly();
}
#[tokio::test]
async fn reading_checkpoint_turns_normalizes_text_tools_and_paginates_messages() {
    let fixture = Fixture::new();
    let mut data = fixture.data();
    let mut messages = vec![
        json!({"type":"user","uuid":"agent-a:0","agentId":"agent-a","message":{"turn":{"case":"agentConversationTurn","value":{"userMessage":{"text":"Inspect the file"},"steps":[{"type":"thinkingMessage","message":{"text":"Inspecting"}},{"type":"toolCall","message":{"id":"call-1","type":"read","args":{"path":"fixture.txt"},"result":{"status":"success","value":"Native output"}}},{"type":"assistantMessage","message":{"text":"Fixture answer"}}]}}}}),
    ];
    for index in 1..101 {
        messages.push(json!({"type":"user","uuid":format!("agent-a:{index}"),"agentId":"agent-a","message":{"type":"agentConversationTurn","turn":{"userMessage":{"text":format!("Prompt {index}")},"steps":[]}}}));
    }
    data["messages"]["agent-a"] = json!(messages);
    fixture.save(&data);
    let document = CursorHarness
        .read_native_history_with(&fixture.query, "agent-a", &fixture.overlay)
        .await
        .unwrap();
    assert_eq!(document.session.native.id, "agent-a");
    assert_eq!(document.session.metadata["recordCount"], 101);
    assert_eq!(
        document.options.execution.config_home,
        Some(fixture.root.join("source"))
    );
    assert!(document.capabilities.resume.is_available());
    assert!(!document.capabilities.fork.is_available());
    assert!(document.records.iter().any(
        |record| matches!(&record.kind,EventKind::TextDelta{text,..} if text=="Fixture answer")
    ));
    assert!(document.records.iter().any(|record|matches!(&record.kind,EventKind::UserMessage(prompt) if prompt.text().unwrap()=="Inspect the file")));
    assert!(document.records.iter().any(|record|matches!(&record.kind,EventKind::ToolCompleted(tool) if tool.name.as_deref()==Some("read") && tool.input["path"]=="fixture.txt" && tool.output["value"]=="Native output")));
    assert!(
        document
            .tree
            .iter()
            .all(|node| node.metadata["nativeAnchor"] == false)
    );
    let requests = fixture.requests();
    let pages = requests
        .iter()
        .filter(|row| row["method"] == "ListAgentMessages")
        .collect::<Vec<_>>();
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[1]["request"]["options"]["offset"], 100);
    assert!(
        !serde_json::to_string(&document)
            .unwrap()
            .contains("private-history-key")
    );
    fixture.readonly();
}
#[tokio::test]
async fn scope_recheck_rejects_moved_agent_before_reading_its_messages() {
    let fixture = Fixture::new();
    let mut data = fixture.data();
    data["agents"]["agent-a"]["local"]["cwd"] = json!(fixture.root.join("changed"));
    fixture.save(&data);
    assert!(
        CursorHarness
            .read_native_history_with(&fixture.query, "agent-a", &fixture.overlay)
            .await
            .is_err()
    );
    assert!(
        !fixture
            .requests()
            .iter()
            .any(|row| row["method"] == "ListAgentMessages")
    );
    fixture.readonly();
}
#[tokio::test]
async fn unknown_cli_identity_and_cloud_identity_never_create_or_resume_sdk_agents() {
    let fixture = Fixture::new();
    fixture.save(&fixture.data());
    assert!(
        CursorHarness
            .read_native_history_with(&fixture.query, "cli-existing-uuid", &fixture.overlay)
            .await
            .is_err()
    );
    assert!(
        CursorHarness
            .read_native_history_with(&fixture.query, "bc-cloud", &fixture.overlay)
            .await
            .is_err()
    );
    fixture.readonly();
}
#[tokio::test]
async fn pagination_cycles_fail_without_unbounded_reads() {
    let fixture = Fixture::new();
    let mut data = fixture.data();
    data["pages"]["second"]["nextCursor"] = json!("second");
    fixture.save(&data);
    assert!(
        tokio::time::timeout(
            Duration::from_secs(3),
            CursorHarness.native_history_with(&fixture.query, &fixture.overlay)
        )
        .await
        .unwrap()
        .is_err()
    );
    fixture.readonly();
}
#[tokio::test]
async fn missing_sdk_store_does_not_launch_or_create_native_history_storage() {
    let fixture = Fixture::new();
    let mut query = fixture.query.clone();
    query.config_home = Some(fixture.root.join("missing-store"));
    assert!(
        CursorHarness
            .native_history_with(&query, &fixture.overlay)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(!fixture.root.join("missing-store").exists());
    assert!(fixture.requests().is_empty());
}
#[tokio::test]
async fn native_protobuf_tool_oneofs_preserve_call_identity_inputs_outputs_and_errors() {
    let fixture = Fixture::new();
    let mut data = fixture.data();
    let call = json!({
        "toolCallId": "native-call",
        "tool": {
            "case": "readToolCall",
            "value": {
                "args": {"path": "native.txt"},
                "result": {"result": {"case": "error", "value": {"message": "Native read error"}}}
            }
        }
    });
    let steps = json!([
        {"message": {"case": "toolCall", "value": call}},
        {"message": {"case": "assistantMessage", "value": {"text": "The read failed"}}}
    ]);
    data["messages"]["agent-a"] = json!([{
        "type": "user", "uuid": "agent-a:0", "agentId": "agent-a",
        "message": {"agentConversationTurn": {"userMessage": {"text": "Read it"}, "steps": steps}}
    }]);
    fixture.save(&data);
    let document = CursorHarness
        .read_native_history_with(&fixture.query, "agent-a", &fixture.overlay)
        .await
        .unwrap();
    assert!(document.records.iter().any(|record|matches!(&record.kind,EventKind::ToolCompleted(tool) if tool.id=="native-call" && tool.name.as_deref()==Some("read") && tool.status=="failed" && tool.input["path"]=="native.txt" && tool.output["value"]["message"]=="Native read error")));
    assert!(document.records.iter().any(
        |record| matches!(&record.kind,EventKind::TextDelta{text,..} if text=="The read failed")
    ));
    fixture.readonly();
}
#[tokio::test]
async fn repeated_message_page_is_rejected_immediately() {
    let fixture = Fixture::new();
    let mut data = fixture.data();
    let messages=(0..100).map(|index|json!({"type":"user","uuid":format!("agent-a:{index}"),"agentId":"agent-a","message":{"text":"Observed text"}})).collect::<Vec<_>>();
    data["messages"]["agent-a"] = json!(messages);
    data["repeatMessages"] = json!(true);
    fixture.save(&data);
    assert!(
        tokio::time::timeout(
            Duration::from_secs(3),
            CursorHarness.read_native_history_with(&fixture.query, "agent-a", &fixture.overlay)
        )
        .await
        .unwrap()
        .is_err()
    );
    assert_eq!(
        fixture
            .requests()
            .iter()
            .filter(|row| row["method"] == "ListAgentMessages")
            .count(),
        2
    );
    fixture.readonly();
}
