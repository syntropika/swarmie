use futures_util::StreamExt;
use swarmie::{
    mock::{MockHarness, MockScript, MockStep},
    *,
};
use swarmie_permissions::{PermissionPolicyExtension, PolicyMode};
use serde_json::{Value, json};
use std::time::Duration;

fn option(id: &str, kind: &str) -> PermissionOption {
    PermissionOption {
        id: id.into(),
        label: id.into(),
        kind: kind.into(),
    }
}
fn request(kind: &str) -> PermissionRequest {
    PermissionRequest {
        questions: Vec::new(),
        id: "operation-1".into(),
        kind: kind.into(),
        title: "Requested operation".into(),
        description: None,
        command: None,
        paths: vec!["example.txt".into()],
        options: vec![
            option("always", "approve"),
            option("approve", "approve"),
            option("deny", "deny"),
        ],
        metadata: Value::Null,
    }
}
async fn start(
    mode: PolicyMode,
    request: PermissionRequest,
) -> (Runtime, Agent, MockHarness, EventStream) {
    let mock = MockHarness::new(MockScript::default().turn(vec![
        MockStep::Request {
            request: request.clone(),
        },
        MockStep::AwaitAnswer {
            id: request.id,
            option: None,
        },
        MockStep::text("completed"),
    ]));
    let runtime = Runtime::builder()
        .harness(mock.clone())
        .extension(PermissionPolicyExtension::new(mode))
        .build();
    let agent = runtime
        .spawn(SpawnOptions::new("mock", std::env::current_dir().unwrap()))
        .await
        .unwrap();
    let mut events = agent.events();
    agent.prompt("perform the operation").await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(event) = events.next().await {
            if matches!(event.unwrap().kind, EventKind::PermissionRequested(_)) {
                return;
            }
        }
        panic!("permission request was not emitted");
    })
    .await
    .unwrap();
    (runtime, agent, mock, events)
}
async fn ended(events: &mut EventStream) -> Vec<RuntimeEvent> {
    tokio::time::timeout(Duration::from_secs(2), async {
        let mut seen = vec![];
        while let Some(event) = events.next().await {
            let event = event.unwrap();
            let end = matches!(event.kind, EventKind::TurnEnded { .. });
            seen.push(event);
            if end {
                return seen;
            }
        }
        panic!("turn did not end");
    })
    .await
    .unwrap()
}
async fn pending(mode: PolicyMode, permission: PermissionRequest) {
    let (_runtime, agent, mock, mut events) = start(mode, permission.clone()).await;
    assert!(
        tokio::time::timeout(Duration::from_millis(30), events.next())
            .await
            .is_err(),
        "policy unexpectedly advanced the pending request"
    );
    assert_eq!(agent.snapshot().await.state, SessionState::NeedsInput);
    assert!(mock.answers().await.is_empty());
    // A pending request remains usable by the client, even when its offered IDs are opaque.
    agent
        .respond_permission(PermissionResponse {
            answers: Default::default(),
            id: permission.id,
            option: permission.options.last().unwrap().id.clone(),
            data: Value::Null,
        })
        .await
        .unwrap();
    ended(&mut events).await;
    assert_eq!(mock.answers().await.len(), 1);
    agent.close().await.unwrap();
}

#[tokio::test]
async fn known_edits_approve_once_without_selecting_session_wide_option() {
    for kind in ["file", "edit", "file_change", "file_write", "patch"] {
        let (_runtime, agent, mock, mut events) =
            start(PolicyMode::AcceptEdits, request(kind)).await;
        let events = ended(&mut events).await;
        let answers = mock.answers().await;
        assert_eq!(answers.len(), 1);
        assert_eq!(answers[0].option, "approve");
        assert!(answers[0].data.is_null());
        assert_eq!(events.iter().filter(|event| matches!(&event.kind, EventKind::PermissionResolved { option, .. } if option == "approve")).count(), 1);
        assert_eq!(agent.snapshot().await.state, SessionState::Idle);
        agent.close().await.unwrap();
    }
}
#[tokio::test]
async fn accept_edits_leaves_commands_questions_plans_and_ambiguous_requests_pending() {
    for kind in [
        "command",
        "execute",
        "question",
        "plan",
        "tool",
        "permissions",
        "future-operation",
    ] {
        pending(PolicyMode::AcceptEdits, request(kind)).await;
    }
}
#[tokio::test]
async fn unknown_or_session_wide_options_remain_interactive() {
    for mode in [PolicyMode::AcceptEdits, PolicyMode::AutoApprove] {
        let mut permission = request("edit");
        permission.options = vec![
            option("opaque-id", "approve"),
            option("always", "approve_session"),
            option("approve", "future-option"),
            option("deny", "deny"),
        ];
        pending(mode, permission).await;
        let mut session_only = request("file");
        session_only.options = vec![option("always", "approve"), option("deny", "deny")];
        pending(mode, session_only).await;
    }
}
#[tokio::test]
async fn ask_questions_and_plans_are_never_automatically_answered() {
    pending(PolicyMode::Ask, request("file")).await;
    for mode in [PolicyMode::AutoApprove, PolicyMode::Deny] {
        for kind in ["question", "plan", "future-operation", "permissions"] {
            pending(mode, request(kind)).await;
        }
    }
}
#[tokio::test]
async fn automatic_modes_select_the_actual_offered_option() {
    let mut permission = request("command");
    permission.options = vec![
        option("remember", "approve_session"),
        option("native-once-choice", "approve_once"),
        option("reject-all", "deny_session"),
        option("native-reject-choice", "deny_once"),
    ];
    let (_runtime, agent, mock, mut events) =
        start(PolicyMode::AutoApprove, permission.clone()).await;
    ended(&mut events).await;
    assert_eq!(mock.answers().await[0].option, "native-once-choice");
    agent.close().await.unwrap();
    let (_runtime, agent, mock, mut events) = start(PolicyMode::Deny, permission).await;
    ended(&mut events).await;
    assert_eq!(mock.answers().await[0].option, "native-reject-choice");
    agent.close().await.unwrap();
    let mut no_denial = request("command");
    no_denial.options = vec![option("approve", "approve"), option("cancel", "cancel")];
    pending(PolicyMode::Deny, no_denial).await;
    let mut session_denial = request("command");
    session_denial.options = vec![option("reject-all", "deny_session")];
    pending(PolicyMode::Deny, session_denial).await;
}
#[tokio::test]
async fn public_commands_select_future_request_policy_without_changing_native_config() {
    let extension = PermissionPolicyExtension::default();
    assert_eq!(extension.id(), "permissions");
    assert_eq!(extension.mode(), PolicyMode::Ask);
    assert_eq!(
        extension.invoke("policy.get", json!({})).await.unwrap(),
        json!({"mode":"ask"})
    );
    assert_eq!(
        extension
            .invoke("policy.set", json!({"mode":"accept_edits"}))
            .await
            .unwrap(),
        json!({"mode":"accept_edits"})
    );
    assert!(
        extension
            .invoke("policy.set", json!({"mode":"full_access"}))
            .await
            .is_err()
    );
    assert!(
        extension
            .invoke("policy.set", json!({"mode":"deny","extra":true}))
            .await
            .is_err()
    );
    assert_eq!(extension.mode(), PolicyMode::AcceptEdits);
    assert!(extension.invoke("unknown", Value::Null).await.is_err());
    assert_eq!(
        "accept-edits".parse::<PolicyMode>().unwrap(),
        PolicyMode::AcceptEdits
    );
    assert!("unsafe-default".parse::<PolicyMode>().is_err());
}

#[tokio::test]
async fn changing_mode_does_not_answer_old_requests_and_applies_to_the_next_turn() {
    let permission = request("file");
    let steps = vec![
        MockStep::Request {
            request: permission.clone(),
        },
        MockStep::AwaitAnswer {
            id: permission.id.clone(),
            option: None,
        },
    ];
    let mock = MockHarness::new(MockScript::default().turn(steps.clone()).turn(steps));
    let runtime = Runtime::builder()
        .harness(mock.clone())
        .extension(PermissionPolicyExtension::default())
        .build();
    let agent = runtime
        .spawn(SpawnOptions::new("mock", std::env::current_dir().unwrap()))
        .await
        .unwrap();
    let mut events = agent.events();
    agent.prompt("first").await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(event) = events.next().await {
            if matches!(event.unwrap().kind, EventKind::PermissionRequested(_)) {
                break;
            }
        }
    })
    .await
    .unwrap();
    runtime
        .invoke_extension("permissions", "policy.set", json!({"mode":"accept_edits"}))
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(30), events.next())
            .await
            .is_err()
    );
    assert_eq!(agent.snapshot().await.state, SessionState::NeedsInput);
    assert!(mock.answers().await.is_empty());
    agent
        .respond_permission(PermissionResponse {
            answers: Default::default(),
            id: permission.id,
            option: "deny".into(),
            data: Value::Null,
        })
        .await
        .unwrap();
    ended(&mut events).await;
    agent.prompt("second").await.unwrap();
    ended(&mut events).await;
    assert_eq!(
        mock.answers()
            .await
            .iter()
            .map(|answer| answer.option.as_str())
            .collect::<Vec<_>>(),
        vec!["deny", "approve"]
    );
    agent.close().await.unwrap();
}
#[tokio::test]
async fn duplicate_option_ids_are_ambiguous_and_remain_interactive() {
    let mut permission = request("file");
    permission.options = vec![
        option("approve", "approve"),
        option("approve", "deny"),
        option("deny", "deny"),
    ];
    pending(PolicyMode::AcceptEdits, permission).await;
}
