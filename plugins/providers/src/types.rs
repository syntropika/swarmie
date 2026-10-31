use crate::invalid;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tembra::Error;
use uuid::Uuid;

macro_rules! name {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> tembra::Result<Self> {
                let value = value.into();
                if value.is_empty() || value.len() > 255 || value.chars().any(char::is_control) {
                    return Err(invalid(
                        "identifier must contain 1 to 255 bytes without controls",
                    ));
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = Error;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

name!(ToolName);
name!(ToolCallId);

/// Stable identity of an external operation, preserved across assignment attempts.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OperationId(Uuid);

impl OperationId {
    pub fn as_uuid(self) -> Uuid {
        self.0
    }

    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl std::fmt::Display for OperationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Model-facing context, distinct from Tembra's authority and cancellation Context.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentContext {
    pub instructions: String,
    /// Captured model-only project input, separate from canonical history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sources: Option<crate::ContextSources>,
    pub messages: Vec<Message>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Message {
    User {
        content: String,
    },
    Assistant(AssistantMessage),
    Tool {
        call: ToolCallId,
        result: ToolResult,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssistantMessage {
    /// Private canonical provider records, reported usage and continuation metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<Value>,
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCall {
    pub id: ToolCallId,
    pub name: ToolName,
    pub arguments: Value,
}

/// A returned failure is a confirmed tool outcome. A provider error is uncertain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ToolResult {
    Success(Value),
    Failed(String),
    Blocked(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSpec {
    pub name: ToolName,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl Usage {
    /// Adds reported token counts without wrapping either counter.
    pub fn checked_add(self, other: Self) -> Option<Self> {
        Some(Self {
            input_tokens: self.input_tokens.checked_add(other.input_tokens)?,
            output_tokens: self.output_tokens.checked_add(other.output_tokens)?,
        })
    }
}

impl Default for OperationId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRequest {
    pub context: AgentContext,
    pub tools: Vec<ToolSpec>,
}

/// Safe per-attempt protection label. An absent or disabled inspector is never reported as clean.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionProtection {
    Pending,
    Unprotected,
    Clean,
    HumanOverride,
    CannotInspectOverride,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelResponse {
    pub protection: SubmissionProtection,
    pub message: AssistantMessage,
    pub usage: Usage,
}
