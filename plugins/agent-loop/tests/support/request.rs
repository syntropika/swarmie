use serde_json::json;
use std::num::NonZeroU32;
use swarmie_agent_loop::*;

pub fn request() -> Request {
    Request {
        context: AgentContext {
            sources: None,
            instructions: "Use the supplied tools.".into(),
            messages: vec![Message::User {
                content: "Add one.".into(),
            }],
        },
        tools: vec![ToolSpec {
            name: ToolName::new("counter.add").unwrap(),
            description: "Add one".into(),
            input_schema: json!({"type":"object"}),
        }],
        max_turns: NonZeroU32::new(3).unwrap(),
    }
}
