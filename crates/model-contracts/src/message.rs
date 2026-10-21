use tembra::prelude::*;

/// A versioned provider namespace. Preserve opaque values verbatim between turns; do not
/// inspect them for agent policy. They are model continuation data, never credentials.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct Extension {
    pub namespace: String,
    pub version: u32,
    pub value: serde_json::Value,
}

/// Ordered inference content. Tool results correlate by the provider-issued call ID.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "tembra::serde",
    tag = "type",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "tembra::schemars", transform = crate::tagged_union)]
pub enum ContentPart {
    Text {
        text: String,
    },
    Reasoning {
        text: String,
    },
    Image {
        media_type: String,
        source: MediaSource,
    },
    ToolCall {
        call: ToolCall,
    },
    ToolResult {
        call_id: String,
        value: serde_json::Value,
        is_error: bool,
    },
    Extension {
        extension: Extension,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "tembra::serde",
    tag = "type",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "tembra::schemars", transform = crate::tagged_union)]
pub enum MediaSource {
    Url { url: String },
    Base64 { data: String },
}

/// Final tool arguments are a JSON object. Streaming arguments are incomplete JSON text.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
    #[serde(default)]
    pub extensions: Vec<Extension>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", rename_all = "snake_case")]
#[schemars(crate = "tembra::schemars")]
pub enum Role {
    System,
    Developer,
    User,
    Assistant,
    Tool,
}

/// A projection for one inference turn, rather than a persistent session event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentPart>,
}

impl Message {
    pub fn text(role: Role, text: impl Into<String>) -> Self {
        Self {
            role,
            content: vec![ContentPart::Text { text: text.into() }],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}
