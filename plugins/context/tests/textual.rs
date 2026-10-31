use swarmie_context::textual;
use swarmie_providers::{
    AgentContext, AssistantMessage, Message, ModelResponse, ToolCall, ToolCallId, ToolName,
    ToolResult, Usage,
};

#[test]
fn complete_source_survives_textual_preparation_without_executable_tools() {
    let call = ToolCall {
        id: ToolCallId::new("one").unwrap(),
        name: ToolName::new("files.write").unwrap(),
        arguments: serde_json::json!({"text":"\"\\🙂"}),
    };
    let source = AgentContext {
        sources: None,
        instructions: "Retain every user constraint".into(),
        messages: vec![
            Message::User {
                content: "🙂\"\\".repeat(349_525),
            },
            Message::Assistant(AssistantMessage {
                provider: None,
                content: "Call admitted".into(),
                tool_calls: vec![call.clone()],
            }),
            Message::Tool {
                call: call.id,
                result: ToolResult::Success(serde_json::json!({"value":"acknowledged"})),
            },
        ],
    };
    let prepared = textual::request(&source).unwrap();
    assert!(prepared.tools.is_empty());
    let [Message::User { content }] = prepared.context.messages.as_slice() else {
        panic!("one complete source record expected")
    };
    let restored: AgentContext = serde_json::from_str(content).unwrap();
    assert_eq!(restored, source);
}

#[test]
fn summary_preserves_usage_but_rejects_empty_or_executable_results() {
    let mut response = ModelResponse {
        protection: swarmie_providers::SubmissionProtection::Clean,
        message: AssistantMessage {
            provider: None,
            content: "Preserved decisions and pending work".into(),
            tool_calls: vec![],
        },
        usage: Usage {
            input_tokens: 100,
            output_tokens: 10,
        },
    };
    let summary = textual::result(response.clone()).unwrap();
    assert_eq!(summary.content, response.message.content);
    assert_eq!(summary.usage, response.usage);
    assert_eq!(summary.protection, response.protection);
    let mut pending = response.clone();
    pending.protection = swarmie_providers::SubmissionProtection::Pending;
    assert!(textual::result(pending).is_err());
    response.message.content = " ".into();
    assert!(textual::result(response.clone()).is_err());
    response.message.content = "Result".into();
    response.message.tool_calls.push(ToolCall {
        id: ToolCallId::new("one").unwrap(),
        name: ToolName::new("files.write").unwrap(),
        arguments: serde_json::json!({}),
    });
    assert!(textual::result(response).is_err());
}

#[test]
fn current_project_sources_are_not_promoted_into_a_historical_summary() {
    use swarmie_providers::{ContextSources, SourceKind, SourcePart, SourceScope};
    let source = AgentContext {
        instructions: "Host instructions".into(),
        sources: Some(
            ContextSources::new(
                SourceScope {
                    node: "node".into(),
                    principal: "user".into(),
                    directory: "project".into(),
                },
                vec![
                    SourcePart::new(
                        SourceKind::ProjectInstructions,
                        "id".into(),
                        "revision".into(),
                        "AGENTS.md".into(),
                        "current source marker".into(),
                    )
                    .unwrap(),
                ],
            )
            .unwrap(),
        ),
        messages: vec![Message::User {
            content: "Canonical source".into(),
        }],
    };
    let prepared = textual::request(&source).unwrap();
    assert!(prepared.context.sources.is_none());
    let encoded = serde_json::to_string(&prepared).unwrap();
    assert!(!encoded.contains("current source marker"));
    assert!(encoded.contains("Canonical source"));
    assert!(source.model_messages().unwrap().len() > source.messages.len());
}
