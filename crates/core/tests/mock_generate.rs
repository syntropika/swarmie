use futures_util::StreamExt;
use mpai::mock::*;
use mpai::*;
use serde_json::{Value, json};
use std::time::Duration;
fn options() -> SpawnOptions {
    SpawnOptions::new("mock", std::env::current_dir().unwrap())
}
fn tool() -> ToolEvent {
    ToolEvent {
        details: None,
        id: "tool".into(),
        name: Some("Read".into()),
        status: "running".into(),
        input: json!({"path":"example.txt"}),
        output: Value::Null,
        metadata: Value::Null,
    }
}
fn request() -> PermissionRequest {
    PermissionRequest {
        questions: Vec::new(),
        id: "question".into(),
        kind: "question".into(),
        title: "Choose a value".into(),
        description: None,
        command: None,
        paths: vec![],
        options: vec![
            PermissionOption {
                id: "answer".into(),
                label: "Submit".into(),
                kind: "answer".into(),
            },
            PermissionOption {
                id: "cancel".into(),
                label: "Cancel".into(),
                kind: "cancel".into(),
            },
        ],
        metadata: json!({"questions":[{"id":"color","prompt":"Color?","options":[{"id":"blue","label":"Blue"}],"multiple":false,"allowText":false}]}),
    }
}
async fn ended(events: &mut EventStream) -> Vec<RuntimeEvent> {
    tokio::time::timeout(Duration::from_secs(2), async {
        let mut result = vec![];
        loop {
            let event = events.next().await.unwrap().unwrap();
            let end = matches!(event.kind, EventKind::TurnEnded { .. });
            result.push(event);
            if end {
                return result;
            }
        }
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn mock_stream_tools_questions_multiturn_and_cancellation_use_the_real_engine() {
    let script = MockScript::default()
        .expect(
            "first",
            vec![
                MockStep::text("before "),
                MockStep::Reasoning {
                    text: "reasoning".into(),
                },
                MockStep::ToolStarted { tool: tool() },
                MockStep::ToolCompleted {
                    tool: ToolEvent {
                        status: "completed".into(),
                        output: json!("file contents"),
                        ..tool()
                    },
                },
                MockStep::Request { request: request() },
                MockStep::AwaitAnswer {
                    id: "question".into(),
                    option: Some("answer".into()),
                },
                MockStep::text("after"),
            ],
        )
        .expect(
            "second",
            vec![
                MockStep::text("cancel me"),
                MockStep::Sleep {
                    milliseconds: 10_000,
                },
            ],
        )
        .expect("third", vec![MockStep::text("final")]);
    let mock = MockHarness::new(script);
    let runtime = Runtime::builder().harness(mock.clone()).build();
    let agent = runtime.spawn(options()).await.unwrap();
    let mut events = agent.events();
    agent.prompt("first").await.unwrap();
    let mut saw_tool = false;
    loop {
        let event = events.next().await.unwrap().unwrap();
        if matches!(event.kind, EventKind::ToolCompleted(_)) {
            saw_tool = true;
        }
        if matches!(event.kind, EventKind::PermissionRequested(_)) {
            break;
        }
    }
    assert!(saw_tool);
    assert_eq!(agent.snapshot().await.state, SessionState::NeedsInput);
    agent
        .respond_permission(PermissionResponse {
            answers: Default::default(),
            id: "question".into(),
            option: "answer".into(),
            data: json!({"answers":{"color":"blue"}}),
        })
        .await
        .unwrap();
    assert!(
        ended(&mut events).await.iter().any(
            |event| matches!(&event.kind, EventKind::TextDelta { text, .. } if text == "after")
        )
    );
    assert_eq!(mock.answers().await[0].data["answers"]["color"], "blue");
    agent.prompt("second").await.unwrap();
    let turn = agent.snapshot().await.turn.unwrap();
    agent.cancel_turn(turn).await.unwrap();
    assert!(ended(&mut events).await.iter().any(
        |event| matches!(&event.kind, EventKind::TurnEnded { outcome, .. } if outcome.as_str() == "cancelled")
    ));
    agent.prompt("third").await.unwrap();
    assert!(
        ended(&mut events).await.iter().any(
            |event| matches!(&event.kind, EventKind::TextDelta { text, .. } if text == "final")
        )
    );
    assert_eq!(mock.prompts().await.len(), 3);
    assert_eq!(agent.snapshot().await.turns_completed, 3);
    assert!(matches!(
        agent.prompt("exhausted").await,
        Err(Error::Rejected(_))
    ));
    agent.close().await.unwrap();
}
#[tokio::test]
async fn generate_disables_tools_is_isolated_and_collects_final_without_duplication() {
    let mock = MockHarness::new(MockScript::default().expect(
        "generate",
        vec![
            MockStep::text("hello"),
            MockStep::Emit {
                kind: Box::new(EventKind::AssistantMessage(
                    json!({"content":[{"type":"text","text":"hello"}]}),
                )),
                raw: Some(json!({"provider":true})),
            },
        ],
    ));
    let runtime = Runtime::builder().harness(mock.clone()).build();
    let mut production_events = runtime.events();
    assert_eq!(
        runtime.generate(options(), "generate").await.unwrap(),
        "hello"
    );
    assert!(runtime.sessions().await.unwrap().is_empty());
    assert!(production_events.try_recv().is_err());
    let launched = mock.launches().await;
    assert_eq!(launched[0].execution.tools, ToolPolicy::Disabled);
    assert!(launched[0].execution.ephemeral);
}
#[tokio::test]
async fn generate_declines_requests_refuses_tool_events_and_enforces_deadline() {
    let runtime = Runtime::builder()
        .harness(MockHarness::new(MockScript::default().turn(vec![
            MockStep::Request { request: request() },
            MockStep::AwaitAnswer {
                id: "question".into(),
                option: Some("cancel".into()),
            },
            MockStep::text("declined"),
        ])))
        .build();
    assert_eq!(
        runtime.generate(options(), "prompt").await.unwrap(),
        "declined"
    );
    let runtime = Runtime::builder()
        .harness(MockHarness::new(
            MockScript::default().turn(vec![MockStep::ToolStarted { tool: tool() }]),
        ))
        .build();
    assert!(
        matches!(runtime.generate(options(), "prompt").await, Err(Error::Provider(message)) if message.contains("attempted a tool"))
    );
    assert!(runtime.sessions().await.unwrap().is_empty());
    let runtime = Runtime::builder()
        .harness(MockHarness::new(MockScript::default().turn(vec![
            MockStep::Sleep {
                milliseconds: 10_000,
            },
        ])))
        .build();
    assert!(matches!(
        runtime
            .generate_with_timeout(options(), "prompt".into(), Duration::from_millis(30))
            .await,
        Err(Error::Timeout(_))
    ));
}
#[tokio::test]
async fn generate_waits_for_native_terminal_after_recoverable_errors() {
    let error = || MockStep::Emit {
        kind: Box::new(EventKind::Error {
            message: "temporary provider overload".into(),
        }),
        raw: None,
    };
    let mock =
        MockHarness::new(MockScript::default().turn(vec![error(), MockStep::text("recovered")]));
    let runtime = Runtime::builder().harness(mock).build();
    assert_eq!(
        runtime.generate(options(), "prompt").await.unwrap(),
        "recovered"
    );
    assert!(runtime.sessions().await.unwrap().is_empty());
    for outcome in [
        TurnOutcome::Failed,
        TurnOutcome::Cancelled,
        TurnOutcome::Interrupted,
        TurnOutcome::Limited,
        TurnOutcome::Refused,
        TurnOutcome::Handled,
        TurnOutcome::Unknown,
    ] {
        let runtime = Runtime::builder()
            .harness(MockHarness::new(MockScript::default().turn(vec![
                error(),
                MockStep::text("partial"),
                MockStep::End { outcome },
            ])))
            .build();
        assert!(
            matches!(runtime.generate(options(),"prompt").await,Err(Error::Provider(message)) if message.contains(outcome.as_str()) && message.contains("temporary provider overload"))
        );
    }
    let runtime = Runtime::builder()
        .harness(MockHarness::new(MockScript::default().turn(vec![
            error(),
            MockStep::Sleep {
                milliseconds: 10_000,
            },
        ])))
        .build();
    assert!(matches!(
        runtime
            .generate_with_timeout(options(), "prompt".into(), Duration::from_millis(30))
            .await,
        Err(Error::Timeout(_))
    ));
}

#[tokio::test]
async fn schema_output_and_credential_redaction_are_real_public_contracts() {
    let mock = MockHarness::new(MockScript::default().turn(vec![MockStep::Emit {
        kind: Box::new(EventKind::StructuredOutput(json!({"answer":42}))),
        raw: Some(json!({"nativeStructured":true})),
    }]));
    let runtime = Runtime::builder().harness(mock.clone()).build();
    let mut spawn = options();
    spawn.execution.output_schema = Some(
        json!({"type":"object","properties":{"answer":{"type":"integer"}},"required":["answer"]}),
    );
    spawn
        .execution
        .env
        .insert("API_TOKEN".into(), "secret-value".into());
    assert_eq!(
        runtime.generate(spawn.clone(), "schema").await.unwrap(),
        "{\"answer\":42}"
    );
    assert_eq!(
        mock.launches().await[0].execution.env["API_TOKEN"],
        "secret-value"
    );
    spawn.execution.output_schema = None;
    let agent = runtime.spawn(spawn).await.unwrap();
    let snapshot = agent.snapshot().await;
    assert!(snapshot.options.execution.env.is_empty());
    assert!(
        !serde_json::to_string(&snapshot)
            .unwrap()
            .contains("secret-value")
    );
    agent.close().await.unwrap();
}
