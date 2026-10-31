use crate::codecs::error::ProviderError;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelInfo {
    pub id: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub r#type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<u64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub owned_by: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_length: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
}

impl ModelInfo {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: Some(name.into()),
            description: None,
            r#type: None,
            created_at: None,
            owned_by: None,
            context_length: None,
            max_output_tokens: None,
        }
    }

    pub fn from_id(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: None,
            description: None,
            r#type: None,
            created_at: None,
            owned_by: None,
            context_length: None,
            max_output_tokens: None,
        }
    }

    pub fn display_name(&self) -> &str {
        self.name.as_ref().unwrap_or(&self.id)
    }
}

impl fmt::Display for ModelInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelList {
    pub data: Vec<ModelInfo>,
}

impl ModelList {
    pub fn new(data: Vec<ModelInfo>) -> Self {
        Self { data }
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn iter(&self) -> std::slice::Iter<'_, ModelInfo> {
        self.data.iter()
    }
}

impl IntoIterator for ModelList {
    type Item = ModelInfo;
    type IntoIter = std::vec::IntoIter<ModelInfo>;

    fn into_iter(self) -> Self::IntoIter {
        self.data.into_iter()
    }
}

impl<'a> IntoIterator for &'a ModelList {
    type Item = &'a ModelInfo;
    type IntoIter = std::slice::Iter<'a, ModelInfo>;

    fn into_iter(self) -> Self::IntoIter {
        self.data.iter()
    }
}

const RESPONSE_BODY_PREVIEW_LIMIT: usize = 2048;

fn format_response_body_preview(body: &[u8]) -> String {
    let preview_len = body.len().min(RESPONSE_BODY_PREVIEW_LIMIT);
    let preview_bytes = body.get(..preview_len).unwrap_or(body);
    let mut preview = String::from_utf8_lossy(preview_bytes).into_owned();

    if body.len() > RESPONSE_BODY_PREVIEW_LIMIT {
        preview.push_str(&format!(
            "\n...<truncated {} bytes>",
            body.len() - RESPONSE_BODY_PREVIEW_LIMIT
        ));
    }

    preview
}

fn format_response_context(
    provider: &str,
    path: &str,
    details: impl fmt::Display,
    body: &[u8],
) -> String {
    format!(
        "provider={provider}\npath={path}\n{details}\nbody_bytes={}\nresponse_body_preview:\n{}",
        body.len(),
        format_response_body_preview(body)
    )
}

pub(crate) fn parse_error(
    provider: &str,
    path: &str,
    details: impl fmt::Display,
    body: &[u8],
) -> ProviderError {
    ProviderError::Response(format_response_context(provider, path, details, body))
}
