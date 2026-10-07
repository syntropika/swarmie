use crate::{Error, PermissionRequest, PermissionResponse, Result, Usage};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QuestionOption {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Question {
    pub id: String,
    pub prompt: String,
    #[serde(default)]
    pub options: Vec<QuestionOption>,
    #[serde(default)]
    pub multiple: bool,
    #[serde(default = "yes")]
    pub allow_text: bool,
    #[serde(default)]
    pub secret: bool,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
fn yes() -> bool {
    true
}

/// Single answers are strings; multiple selections retain their ordered string array.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum QuestionAnswer {
    Text(String),
    Choices(Vec<String>),
}

impl PermissionRequest {
    pub fn canonical_questions(&self) -> Result<Vec<Question>> {
        let legacy = self
            .metadata
            .get("questions")
            .filter(|v| !v.is_null())
            .map(|v| serde_json::from_value::<Vec<Question>>(v.clone()))
            .transpose()
            .map_err(|e| Error::Invalid(format!("invalid questions: {e}")))?;
        if !self.questions.is_empty() {
            if legacy.as_ref().is_some_and(|q| q != &self.questions) {
                return Err(Error::Invalid(
                    "conflicting canonical and metadata questions".into(),
                ));
            }
            Ok(self.questions.clone())
        } else {
            Ok(legacy.unwrap_or_default())
        }
    }
    /// Fill the typed surface and the legacy metadata used by existing clients.
    pub fn normalize_questions(&mut self) -> Result<()> {
        self.questions = self.canonical_questions()?;
        if !self.questions.is_empty() {
            if self.kind != "question" {
                return Err(Error::Invalid(
                    "questions require a question request".into(),
                ));
            }
            if self.metadata.is_null() {
                self.metadata = json!({});
            }
            let metadata = self
                .metadata
                .as_object_mut()
                .ok_or_else(|| Error::Invalid("question metadata must be an object".into()))?;
            metadata.insert("questions".into(), serde_json::to_value(&self.questions)?);
        }
        Ok(())
    }
}
impl PermissionResponse {
    /// Preserve legacy `data.answers`, rejecting ambiguous duplicate answer sources.
    pub fn normalize_answers(&mut self, question: bool) -> Result<()> {
        if !question && (!self.answers.is_empty() || self.data.get("answers").is_some()) {
            return Err(Error::Invalid(
                "answers require a question answer operation".into(),
            ));
        }
        if self.answers.is_empty() {
            return Ok(());
        }
        if self.data.is_null() {
            self.data = json!({});
        }
        let data = self
            .data
            .as_object_mut()
            .ok_or_else(|| Error::Invalid("question response data must be an object".into()))?;
        let answers = serde_json::to_value(&self.answers)?;
        if data.get("answers").is_some_and(|old| old != &answers) {
            return Err(Error::Invalid(
                "conflicting canonical and data answers".into(),
            ));
        }
        data.insert("answers".into(), answers);
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, Default, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanUpdate {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<PlanEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markdown: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SubagentInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_tool_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_delta: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<bool>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileLocation {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_column: Option<u64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unified: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolProgress {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_seconds: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percent: Option<f64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolDetails {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locations: Vec<FileLocation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diffs: Vec<FileDiff>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<ToolProgress>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subagent: Option<SubagentInfo>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

fn object(value: Value) -> BTreeMap<String, Value> {
    match value {
        Value::Object(map) => map.into_iter().collect(),
        Value::Null => BTreeMap::new(),
        legacy => BTreeMap::from([("native".into(), legacy)]),
    }
}
fn text(map: &mut BTreeMap<String, Value>, key: &str) -> Option<String> {
    let value = map.remove(key)?;
    if value.is_null() {
        return None;
    }
    if let Some(result) = value.as_str() {
        return Some(result.to_owned());
    }
    unparsed(map, key, value);
    None
}
fn unparsed(map: &mut BTreeMap<String, Value>, key: &str, value: Value) {
    let fields = map
        .entry("nativeFields".into())
        .or_insert_with(|| json!({}));
    if !fields.is_object() {
        *fields = json!({"previous":fields.take()});
    }
    fields[key] = value;
}
impl From<Value> for PlanEntry {
    fn from(value: Value) -> Self {
        let legacy_text = value.as_str().map(str::to_owned);
        let mut extra = object(value);
        let content = text(&mut extra, "text")
            .or_else(|| {
                extra
                    .get("content")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .or_else(|| extra.get("step").and_then(Value::as_str).map(str::to_owned))
            .or(legacy_text);
        Self {
            text: content,
            status: text(&mut extra, "status"),
            id: text(&mut extra, "id"),
            priority: text(&mut extra, "priority"),
            extra,
        }
    }
}
impl From<Value> for PlanUpdate {
    fn from(value: Value) -> Self {
        let legacy_markdown = value.as_str().map(str::to_owned);
        let legacy_entries = value.as_array().cloned();
        let mut extra = object(value);
        let source = extra
            .get("entries")
            .filter(|v| v.is_array())
            .or_else(|| extra.get("plan").filter(|v| v.is_array()))
            .or_else(|| extra.get("steps").filter(|v| v.is_array()))
            .cloned();
        let entries = source
            .and_then(|v| v.as_array().cloned())
            .or(legacy_entries)
            .unwrap_or_default()
            .into_iter()
            .map(PlanEntry::from)
            .collect();
        if let Some(original) = extra.remove("entries")
            && !original.is_array()
            && !original.is_null()
        {
            unparsed(&mut extra, "entries", original);
        }
        Self {
            entries,
            markdown: text(&mut extra, "markdown").or(legacy_markdown),
            text: text(&mut extra, "text"),
            title: text(&mut extra, "title"),
            status: text(&mut extra, "status"),
            request_id: text(&mut extra, "requestId"),
            extra,
        }
    }
}
impl From<Value> for SubagentInfo {
    fn from(value: Value) -> Self {
        let mut extra = object(value);
        let tokens = extra.get("tokens").and_then(Value::as_u64);
        if let Some(original) = extra.remove("tokens")
            && tokens.is_none()
            && !original.is_null()
        {
            unparsed(&mut extra, "tokens", original);
        }
        let background = extra.get("background").and_then(Value::as_bool);
        if let Some(original) = extra.remove("background")
            && background.is_none()
            && !original.is_null()
        {
            unparsed(&mut extra, "background", original);
        }
        let usage = extra
            .get("usage")
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok());
        if usage.is_some() || extra.get("usage").is_some_and(Value::is_null) {
            extra.remove("usage");
        } else if let Some(native) = extra.remove("usage") {
            extra.insert("nativeUsage".into(), native);
        }
        Self {
            id: text(&mut extra, "id"),
            parent_tool_id: text(&mut extra, "parentToolId"),
            parent_session_id: text(&mut extra, "parentSessionId"),
            role: text(&mut extra, "role"),
            model: text(&mut extra, "model"),
            status: text(&mut extra, "status"),
            summary: text(&mut extra, "summary"),
            title: text(&mut extra, "title"),
            text_delta: text(&mut extra, "textDelta"),
            tokens,
            usage,
            background,
            extra,
        }
    }
}
macro_rules! legacy_deserialize {
    ($name:ty) => {
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(
                deserializer: D,
            ) -> std::result::Result<Self, D::Error> {
                Value::deserialize(deserializer).map(Self::from)
            }
        }
    };
}
legacy_deserialize!(PlanEntry);
legacy_deserialize!(PlanUpdate);
legacy_deserialize!(SubagentInfo);
