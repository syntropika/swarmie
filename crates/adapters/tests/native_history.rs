use adapters::{ClaudeHarness, CodexHarness, PiHarness};
use swarmie::*;
use serde_json::{Value, json};
use std::path::PathBuf;
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("swarmie-history-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn query(&self, harness: &str) -> NativeHistoryQuery {
        NativeHistoryQuery {
            harness: harness.into(),
            cwd: Some(self.0.clone()),
            config_home: Some(self.0.clone()),
            limit: 100,
        }
    }
    fn write(&self, relative: &str, records: Vec<Value>) -> PathBuf {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            records
                .iter()
                .map(|value| format!("{value}\n"))
                .collect::<String>(),
        )
        .unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn message(id: &str, role: &str, text: &str) -> Value {
    json!({"id":id,"type":"message","role":role,"content":[{"type":"text","text":text}]})
}
#[tokio::test]
async fn codex_history_preserves_native_ids_turn_groups_and_raw_without_launching() {
    let fixture = Fixture::new();
    let id = uuid::Uuid::new_v4().to_string();
    fixture.write(
        "sessions/2026/fixture.jsonl",
        vec![
            json!({"type":"session_meta","payload":{"id":id,"cwd":fixture.0}}),
            json!({"type":"event_msg","payload":{"type":"task_started","turn_id":"native-turn"}}),
            json!({"type":"response_item","payload":message("u1","user","task")}),
            json!({"type":"response_item","payload":message("u2","user","steer")}),
            json!({"type":"response_item","payload":message("a1","assistant","answer")}),
            json!({"type":"turn_context","payload":{"model":"selected-model"}}),
        ],
    );
    let query = fixture.query("codex");
    let found = CodexHarness.native_history(&query).await.unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].native.id, id);
    let document = CodexHarness.read_native_history(&query, &id).await.unwrap();
    assert_eq!(document.tree.len(), 3);
    assert!(
        document
            .tree
            .iter()
            .all(|node| node.active && node.metadata["nativeAnchor"] == true)
    );
    assert_eq!(
        document
            .records
            .iter()
            .filter(|record| matches!(record.kind, EventKind::TurnStarted))
            .count(),
        1
    );
    assert_eq!(document.options.model.as_deref(), Some("selected-model"));
    assert_eq!(
        document
            .records
            .iter()
            .filter(|record| matches!(record.kind, EventKind::Raw))
            .count(),
        6
    );
    assert!(matches!(
        CodexHarness
            .read_native_history(&query, "../../private.jsonl")
            .await,
        Err(Error::Invalid(_))
    ));
}
#[tokio::test]
async fn claude_and_pi_active_branches_exclude_abandoned_messages_but_preserve_the_tree() {
    for harness in ["claude", "pi"] {
        let fixture = Fixture::new();
        let session = uuid::Uuid::new_v4().to_string();
        let records = if harness == "claude" {
            vec![
                json!({"type":"user","uuid":"u1","parentUuid":null,"sessionId":session,"cwd":fixture.0,"message":{"role":"user","content":"first"}}),
                json!({"type":"assistant","uuid":"abandoned","parentUuid":"u1","sessionId":session,"cwd":fixture.0,"message":{"role":"assistant","content":"abandoned answer"}}),
                json!({"type":"assistant","uuid":"chosen","parentUuid":"u1","sessionId":session,"cwd":fixture.0,"message":{"role":"assistant","content":"chosen answer"}}),
            ]
        } else {
            vec![
                json!({"type":"session","version":3,"id":session,"cwd":fixture.0}),
                json!({"type":"message","id":"u1","parentId":null,"message":{"role":"user","content":"first"}}),
                json!({"type":"message","id":"abandoned","parentId":"u1","message":{"role":"assistant","content":"abandoned answer"}}),
                json!({"type":"message","id":"chosen","parentId":"u1","message":{"role":"assistant","content":"chosen answer"}}),
            ]
        };
        let path = fixture.write(
            if harness == "claude" {
                "projects/work/session.jsonl"
            } else {
                "sessions/work/session.jsonl"
            },
            records,
        );
        let query = fixture.query(harness);
        let document = if harness == "claude" {
            ClaudeHarness
                .read_native_history(&query, &session)
                .await
                .unwrap()
        } else {
            PiHarness
                .read_native_history(
                    &query,
                    &std::fs::canonicalize(path).unwrap().to_string_lossy(),
                )
                .await
                .unwrap()
        };
        assert_eq!(document.tree.len(), 3);
        assert!(
            !document
                .tree
                .iter()
                .find(|node| node.id == "abandoned")
                .unwrap()
                .active
        );
        assert!(
            document
                .tree
                .iter()
                .find(|node| node.id == "chosen")
                .unwrap()
                .active
        );
        assert!(!document.records.iter().any(|record|matches!(&record.kind,EventKind::TextDelta{text,..} if text.contains("abandoned"))));
        assert!(document.records.iter().any(
            |record| matches!(&record.kind,EventKind::TextDelta{text,..} if text=="chosen answer")
        ));
        if harness == "claude" {
            assert_eq!(
                document.capabilities.metadata["rollbackHistorySupported"],
                false
            );
        }
    }
}
#[tokio::test]
async fn runtime_import_is_durable_idempotent_and_does_not_spawn_native_work() {
    let fixture = Fixture::new();
    let id = uuid::Uuid::new_v4().to_string();
    fixture.write("projects/work/session.jsonl",vec![json!({"type":"user","uuid":"u1","parentUuid":null,"sessionId":id,"cwd":fixture.0,"message":{"role":"user","content":"imported task"}}),json!({"type":"assistant","uuid":"a1","parentUuid":"u1","sessionId":id,"cwd":fixture.0,"message":{"role":"assistant","content":"imported answer"}})]);
    let runtime = Runtime::builder().harness(ClaudeHarness).build();
    let imported = runtime
        .import_native_history(fixture.query("claude"), &id)
        .await
        .unwrap();
    assert_eq!(imported.state, SessionState::Closed);
    assert_eq!(imported.native.id, id);
    assert_eq!(imported.turns_completed, 1);
    assert_eq!(
        runtime
            .import_native_history(fixture.query("claude"), &id)
            .await
            .unwrap()
            .id,
        imported.id
    );
    assert!(runtime.agent(&imported.id).await.is_none());
    let history = runtime.transcript(&imported.id, 0, 4096).await.unwrap();
    assert!(history.iter().all(|event| event.session_id == imported.id));
    assert!(history.iter().any(
        |event| matches!(&event.kind,EventKind::TextDelta{text,..} if text=="imported answer")
    ));
    assert_eq!(runtime.session_tree(&imported.id).await.unwrap().len(), 2);
}

struct InstalledPi(PathBuf);
#[async_trait::async_trait]
impl Harness for InstalledPi {
    fn id(&self) -> &str {
        "pi"
    }
    async fn detect(&self) -> Result<Detection> {
        PiHarness.detect().await
    }
    async fn native_history(
        &self,
        query: &NativeHistoryQuery,
    ) -> Result<Vec<NativeHistorySession>> {
        PiHarness.native_history(query).await
    }
    async fn read_native_history(
        &self,
        query: &NativeHistoryQuery,
        id: &str,
    ) -> Result<NativeHistoryDocument> {
        PiHarness.read_native_history(query, id).await
    }
    async fn connect(
        &self,
        options: &SpawnOptions,
        native: Option<&NativeSessionRef>,
        fork: bool,
    ) -> Result<BackendConnection> {
        let mut options = options.clone();
        options.config["executable"] = json!(self.0);
        PiHarness.connect(&options, native, fork).await
    }
    async fn fork_before_user(
        &self,
        options: &SpawnOptions,
        native: &NativeSessionRef,
        message: &str,
    ) -> Result<BackendConnection> {
        let mut options = options.clone();
        options.config["executable"] = json!(self.0);
        PiHarness.fork_before_user(&options, native, message).await
    }
}
#[tokio::test]
#[ignore = "requires the installed Pi fixture CLI; runs native session operations without model inference"]
async fn installed_pi_before_user_fork_uses_actual_child_history_and_preserves_source() {
    let fixture = Fixture::new();
    let session = uuid::Uuid::new_v4().to_string();
    let assistant = |text: &str| json!({"role":"assistant","content":[{"type":"text","text":text}],"api":"openai-responses","provider":"openai","model":"gpt-4.1","usage":{"input":1,"output":1,"cacheRead":0,"cacheWrite":0,"totalTokens":2,"cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"total":0}},"stopReason":"stop","timestamp":1});
    let path=fixture.write("sessions/work/native.jsonl",vec![json!({"type":"session","version":3,"id":session,"timestamp":"2026-10-03T00:00:00Z","cwd":fixture.0}),json!({"type":"message","id":"aa000001","parentId":null,"timestamp":"2026-10-03T00:00:01Z","message":{"role":"user","content":"first native task","timestamp":1}}),json!({"type":"message","id":"aa000002","parentId":"aa000001","timestamp":"2026-10-03T00:00:02Z","message":assistant("first native answer")}),json!({"type":"message","id":"aa000003","parentId":"aa000002","timestamp":"2026-10-03T00:00:03Z","message":{"role":"user","content":"second native task","timestamp":3}}),json!({"type":"message","id":"aa000004","parentId":"aa000003","timestamp":"2026-10-03T00:00:04Z","message":assistant("second native answer")})]);
    let executable = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/harnesses/node_modules/.bin/pi")
        .canonicalize()
        .unwrap();
    let runtime = Runtime::builder().harness(InstalledPi(executable)).build();
    let source = runtime
        .import_native_history(
            fixture.query("pi"),
            &std::fs::canonicalize(&path).unwrap().to_string_lossy(),
        )
        .await
        .unwrap();
    let source = runtime.resume(&source.id).await.unwrap();
    source.pause_queue(true).await.unwrap();
    let parent_id = source.snapshot().await.id;
    let before = std::fs::read(&path).unwrap();
    let branch = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        runtime.fork_before_user(&parent_id, "aa000003", None),
    )
    .await
    .unwrap()
    .unwrap();
    branch.pause_queue(true).await.unwrap();
    let snapshot = branch.snapshot().await;
    assert_ne!(snapshot.native.id, source.snapshot().await.native.id);
    assert_eq!(snapshot.turns_completed, 1);
    let history = runtime.transcript(&snapshot.id, 0, 4096).await.unwrap();
    assert!(history.iter().any(
        |event| matches!(&event.kind,EventKind::TextDelta{text,..} if text=="first native answer")
    ));
    assert!(!history.iter().any(
        |event| matches!(&event.kind,EventKind::TextDelta{text,..} if text=="second native answer")
    ));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    branch.close().await.unwrap();
    source.close().await.unwrap();
}

#[tokio::test]
async fn selected_history_preview_and_direct_lookup_do_not_lose_older_sessions_at_the_list_limit() {
    let fixture = Fixture::new();
    let old = fixture.write(
        "sessions/old.jsonl",
        vec![
            json!({"type":"session_meta","payload":{"id":"old-session","cwd":fixture.0}}),
            json!({"type":"response_item","payload":message("u-old","user","An older task")}),
        ],
    );
    std::fs::File::open(old)
        .unwrap()
        .set_times(
            std::fs::FileTimes::new()
                .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1)),
        )
        .unwrap();
    for index in 0..1000 {
        fixture.write(&format!("sessions/new-{index}.jsonl"),vec![json!({"type":"session_meta","payload":{"id":format!("new-{index}"),"cwd":fixture.0}}),json!({"type":"response_item","payload":message("u","user","A recent task")})]);
    }
    let mut query = fixture.query("codex");
    query.limit = 1;
    let listed = CodexHarness.native_history(&query).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].title, "A recent task");
    let old = CodexHarness
        .read_native_history(&query, "old-session")
        .await
        .unwrap();
    assert_eq!(old.session.native.id, "old-session");
    assert_eq!(old.session.title, "An older task");
}

#[tokio::test]
async fn imported_active_tools_and_images_preserve_observed_lifecycle_without_synthetic_completions()
 {
    for harness in ["codex", "claude", "pi"] {
        let fixture = Fixture::new();
        let session = uuid::Uuid::new_v4().to_string();
        let content = if harness == "codex" {
            json!([{"type":"input_text","text":"inspect image"},{"type":"input_image","image_url":"data:image/png;base64,aW1hZ2U="}])
        } else if harness == "claude" {
            json!([{"type":"text","text":"inspect image"},{"type":"image","source":{"type":"base64","media_type":"image/png","data":"aW1hZ2U="}}])
        } else {
            json!([{"type":"text","text":"inspect image"},{"type":"image","mimeType":"image/png","data":"aW1hZ2U="}])
        };
        let mut records = match harness {
            "codex" => vec![
                json!({"type":"session_meta","payload":{"id":session,"cwd":fixture.0}}),
                json!({"type":"response_item","payload":{"type":"message","id":"u","role":"user","content":content}}),
                json!({"type":"response_item","payload":{"type":"function_call","call_id":"tool1","name":"read_file","arguments":"{\"path\":\"hello.txt\"}"}}),
                json!({"type":"response_item","payload":{"type":"function_call_output","call_id":"tool1","output":"file contents"}}),
                json!({"type":"response_item","payload":message("a","assistant","done")}),
            ],
            "claude" => vec![
                json!({"uuid":"u","parentUuid":null,"sessionId":session,"cwd":fixture.0,"message":{"role":"user","content":content}}),
                json!({"uuid":"a-tool","parentUuid":"u","sessionId":session,"cwd":fixture.0,"message":{"role":"assistant","content":[{"type":"tool_use","id":"tool1","name":"read_file","input":{"path":"hello.txt"}}]}}),
                json!({"uuid":"result","parentUuid":"a-tool","sessionId":session,"cwd":fixture.0,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tool1","content":"file contents"}]}}),
                json!({"uuid":"a","parentUuid":"result","sessionId":session,"cwd":fixture.0,"message":{"role":"assistant","content":"done"}}),
            ],
            _ => vec![
                json!({"type":"session","id":session,"cwd":fixture.0}),
                json!({"type":"message","id":"u","parentId":null,"message":{"role":"user","content":content}}),
                json!({"type":"message","id":"a-tool","parentId":"u","message":{"role":"assistant","content":[{"type":"toolCall","id":"tool1","name":"read_file","arguments":{"path":"hello.txt"}}]}}),
                json!({"type":"message","id":"result","parentId":"a-tool","message":{"role":"toolResult","toolCallId":"tool1","toolName":"read_file","content":[{"type":"text","text":"file contents"}]}}),
                json!({"type":"message","id":"a","parentId":"result","message":{"role":"assistant","content":"done"}}),
            ],
        };
        let pending = match harness {
            "codex" => {
                json!({"type":"response_item","payload":{"type":"function_call","call_id":"unfinished","name":"read_file","arguments":"{}"}})
            }
            "claude" => {
                records.last_mut().unwrap()["parentUuid"] = json!("pending");
                json!({"uuid":"pending","parentUuid":"result","sessionId":session,"cwd":fixture.0,"message":{"role":"assistant","content":[{"type":"tool_use","id":"unfinished","name":"read_file","input":{}}]}})
            }
            _ => {
                records.last_mut().unwrap()["parentId"] = json!("pending");
                json!({"type":"message","id":"pending","parentId":"result","message":{"role":"assistant","content":[{"type":"toolCall","id":"unfinished","name":"read_file","arguments":{}}]}})
            }
        };
        records.insert(records.len() - 1, pending);
        if harness == "claude" {
            for entry in &mut records {
                entry["type"] = entry["message"]["role"].clone();
            }
        }
        let relative = match harness {
            "codex" => "sessions/import.jsonl",
            "claude" => "projects/work/import.jsonl",
            _ => "sessions/work/import.jsonl",
        };
        let path = fixture.write(relative, records);
        let query = fixture.query(harness);
        let document = match harness {
            "codex" => CodexHarness
                .read_native_history(&query, &session)
                .await
                .unwrap(),
            "claude" => ClaudeHarness
                .read_native_history(&query, &session)
                .await
                .unwrap(),
            _ => PiHarness
                .read_native_history(
                    &query,
                    &std::fs::canonicalize(path).unwrap().to_string_lossy(),
                )
                .await
                .unwrap(),
        };
        let started = document
            .records
            .iter()
            .find_map(|record| {
                if let EventKind::ToolStarted(tool) = &record.kind {
                    Some((record.turn_id.clone(), tool))
                } else {
                    None
                }
            })
            .unwrap();
        let completed = document
            .records
            .iter()
            .find_map(|record| {
                if let EventKind::ToolCompleted(tool) = &record.kind {
                    Some((record.turn_id.clone(), tool))
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(started.0, completed.0);
        assert_eq!(started.1.id, "tool1");
        assert_eq!(completed.1.name.as_deref(), Some("read_file"));
        assert_eq!(completed.1.input["path"], "hello.txt");
        assert_eq!(completed.1.status, "completed");
        assert_eq!(
            document
                .records
                .iter()
                .filter(|record| matches!(record.kind, EventKind::ToolStarted(_)))
                .count(),
            2
        );
        assert_eq!(
            document
                .records
                .iter()
                .filter(|record| matches!(record.kind, EventKind::ToolCompleted(_)))
                .count(),
            1
        );
        assert_eq!(
            document
                .records
                .iter()
                .filter(|record| matches!(record.kind, EventKind::TurnStarted))
                .count(),
            1
        );
        assert!(document.records.iter().any(|record|matches!(&record.kind,EventKind::UserMessage(prompt) if prompt.content.iter().any(|content|matches!(content,PromptContent::Image{mime_type,data} if mime_type=="image/png" && data=="aW1hZ2U=")))));
        assert!(document.tree.iter().any(|node| {
            node.role == "user"
                && node.metadata["prompt"]["content"]
                    .as_array()
                    .is_some_and(|parts| {
                        parts.iter().any(|part| {
                            part["type"] == "image"
                                && part["data"]["mime_type"] == "image/png"
                                && part["data"]["data"] == "aW1hZ2U="
                        })
                    })
        }));
    }
}
