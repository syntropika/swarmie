#![cfg(unix)]
use adapters::AcpHarness;
use swarmie::*;
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("swarmie-acp-ext-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn options(&self, provider: &str) -> SpawnOptions {
        let mut o = SpawnOptions::new(provider, &self.0);
        o.config = json!({"executable":"python3","args":[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/acp_extensions.py"),provider]});
        o
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
async fn request(connection: &mut BackendConnection) -> (PermissionRequest, Vec<EventKind>) {
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut events = vec![];
        loop {
            match connection.events.recv().await.unwrap() {
                BackendEvent::Content { kind, .. } => match *kind {
                    EventKind::PermissionRequested(p) => return (p, events),
                    kind => events.push(kind),
                },
                event => panic!("unexpected event before permission: {event:?}"),
            }
        }
    })
    .await
    .unwrap()
}
async fn finish(connection: &mut BackendConnection) -> (Value, Vec<EventKind>) {
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut text = String::new();
        let mut events = vec![];
        loop {
            match connection.events.recv().await.unwrap() {
                BackendEvent::Content { kind, .. } => {
                    if let EventKind::TextDelta { text: delta, .. } = &*kind {
                        text.push_str(delta);
                    }
                    events.push(*kind);
                }
                BackendEvent::TurnFinished { .. } => {
                    return (serde_json::from_str(&text).unwrap(), events);
                }
                event => panic!("unexpected event: {event:?}"),
            }
        }
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn cursor_questions_use_native_ids_and_plan_approval_waits_for_client_decision() {
    let fixture = Fixture::new();
    let options = fixture.options("cursor");
    let harness = AcpHarness::new("cursor", "cursor-agent", vec!["acp".into()]);
    let mut c = harness.connect(&options, None, false).await.unwrap();
    assert!(matches!(
        c.backend
            .prompt(&Prompt {
                content: vec![PromptContent::Image {
                    data: "AA==".into(),
                    mime_type: "image/png".into()
                }]
            })
            .await,
        Err(Error::Unsupported(_))
    ));
    c.backend.prompt(&Prompt::from("review")).await.unwrap();
    let (question, _) = request(&mut c).await;
    assert_eq!(question.kind, "question");
    assert_eq!(question.metadata["questions"][0]["id"], "question-a");
    assert_eq!(
        question.metadata["questions"][0]["options"][0]["id"],
        "option-a"
    );
    assert_eq!(question.metadata["questions"][0]["allowText"], false);
    assert!(matches!(
        c.backend
            .respond_permission(&PermissionResponse {
                answers: Default::default(),
                id: question.id.clone(),
                option: "answer".into(),
                data: json!({"answers":{"question-a":"not-a-native-option"}})
            })
            .await,
        Err(Error::Invalid(_))
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(25), c.events.recv())
            .await
            .is_err()
    );
    c.backend
        .respond_permission(&PermissionResponse {
            answers: Default::default(),
            id: question.id,
            option: "answer".into(),
            data: json!({"answers":{"question-a":"option-a"}}),
        })
        .await
        .unwrap();
    let (plan, events) = request(&mut c).await;
    assert_eq!(plan.kind, "plan");
    assert!(plan.description.as_deref().unwrap().contains("# Plan"));
    assert!(events.iter().any(|event| matches!(event,EventKind::PlanUpdated(p) if p.markdown.as_deref()==Some("# Plan\nInspect the project."))));
    assert!(
        tokio::time::timeout(Duration::from_millis(25), c.events.recv())
            .await
            .is_err()
    );
    c.backend
        .respond_permission(&PermissionResponse {
            answers: Default::default(),
            id: plan.id,
            option: "deny".into(),
            data: json!({"reason":"Review first"}),
        })
        .await
        .unwrap();
    let (receipt, events) = finish(&mut c).await;
    assert_eq!(
        receipt["replies"][0]["result"],
        json!({"outcome":{"outcome":"answered","answers":[{"questionId":"question-a","selectedOptionIds":["option-a"]}]}})
    );
    assert_eq!(
        receipt["replies"][1]["result"]["outcome"]["outcome"],
        "rejected"
    );
    assert!(receipt["replies"][2]["error"].is_object());
    assert!(events.iter().any(|e| matches!(e,EventKind::PlanUpdated(p) if p.entries.len()==2 && p.entries[0].status.as_deref()==Some("completed"))));
    assert!(
        events
            .iter()
            .any(|e| matches!(e,EventKind::SubagentUpdated(p) if p.id.as_deref()==Some("child-a")))
    );
    c.backend.close().await.unwrap();
}
#[tokio::test]
async fn grok_question_and_plan_use_the_published_native_response_dialects() {
    let fixture = Fixture::new();
    let options = fixture.options("grok");
    let harness = AcpHarness::new("grok", "grok", vec![]);
    let mut c = harness.connect(&options, None, false).await.unwrap();
    c.backend.prompt(&Prompt::from("review")).await.unwrap();
    let (question, _) = request(&mut c).await;
    c.backend
        .respond_permission(&PermissionResponse {
            answers: Default::default(),
            id: question.id,
            option: "answer".into(),
            data: json!({"answers":{"0":"First"}}),
        })
        .await
        .unwrap();
    let (plan, _) = request(&mut c).await;
    c.backend
        .respond_permission(&PermissionResponse {
            answers: Default::default(),
            id: plan.id,
            option: "deny".into(),
            data: json!({"feedback":"Revise the plan"}),
        })
        .await
        .unwrap();
    let (receipt, _) = finish(&mut c).await;
    assert_eq!(receipt["replies"][0]["result"]["outcome"], "accepted");
    assert_eq!(
        receipt["replies"][0]["result"]["answers"],
        json!({"Which option?":["First"]})
    );
    assert_eq!(
        receipt["replies"][0]["result"]["annotations"]["Which option?"]["preview"],
        "First preview"
    );
    assert_eq!(
        receipt["replies"][1]["result"],
        json!({"outcome":"cancelled","feedback":"Revise the plan"})
    );
    c.backend.close().await.unwrap();
}
#[tokio::test]
async fn kiro_native_model_config_options_and_announced_commands_are_preserved() {
    let fixture = Fixture::new();
    let mut options = fixture.options("kiro");
    options.model = Some("model-b".into());
    let harness = AcpHarness::new("kiro", "kiro-cli", vec!["acp".into()]);
    let mut c = harness.connect(&options, None, false).await.unwrap();
    assert!(c.capabilities.models.is_available());
    assert_eq!(json!(c.backend.models().await.unwrap())[0]["id"], "model-a");
    assert_eq!(
        json!(c.backend.configuration().await.unwrap())[0]["value"],
        "model-b"
    );
    c.backend
        .configure(json!({"model":"model-a"}))
        .await
        .unwrap();
    c.backend.prompt(&Prompt::from("receipt")).await.unwrap();
    let (receipt, _) = finish(&mut c).await;
    assert_eq!(receipt["model"], "model-a");
    assert_eq!(receipt["requests"].as_array().unwrap().len(), 2);
    assert!(
        receipt["requests"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["method"] == "session/set_config_option")
    );
    assert_eq!(
        c.backend.commands().await.unwrap()[0]["name"],
        "/context add"
    );
    assert_eq!(c.backend.commands().await.unwrap()[0]["input"], "<path>");
    c.backend.close().await.unwrap();
}
