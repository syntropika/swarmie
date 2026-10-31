use serde::Serialize;
use serde_json::Value;
use std::{any::Any, fmt};

use crate::ToolExecutionError;

/// Explicit tool presentation supported by the current model message contract.
/// Text is never parsed to guess whether it contains JSON.
#[derive(Clone, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ToolOutput {
    Text(String),
    Json(Value),
}

impl ToolOutput {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    pub fn json(value: Value) -> Self {
        Self::Json(value)
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            Self::Json(_) => None,
        }
    }

    pub fn as_json(&self) -> Option<&Value> {
        match self {
            Self::Json(value) => Some(value),
            Self::Text(_) => None,
        }
    }

    /// Convert at the provider message boundary without reparsing strings.
    pub fn into_value(self) -> Value {
        match self {
            Self::Text(text) => Value::String(text),
            Self::Json(value) => value,
        }
    }
}

impl fmt::Debug for ToolOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ToolOutput")
            .field(&match self {
                Self::Text(_) => "text",
                Self::Json(_) => "json",
            })
            .finish()
    }
}

/// Convert an owned serializable value into its explicit model presentation.
/// Return `ToolOutput` to override a serializable type's default presentation.
pub trait IntoToolOutput {
    fn into_tool_output(self) -> Result<ToolOutput, ToolExecutionError>;
}

impl<T: Serialize + 'static> IntoToolOutput for T {
    fn into_tool_output(self) -> Result<ToolOutput, ToolExecutionError> {
        let mut value = Some(self);
        let slot = &mut value as &mut dyn Any;
        if let Some(output) = slot.downcast_mut::<Option<ToolOutput>>() {
            return Ok(output.take().expect("output is present"));
        }
        let explicit_json = slot.is::<Option<Value>>();
        serde_json::to_value(value)
            .map(|value| match value {
                Value::String(text) if !explicit_json => ToolOutput::Text(text),
                value => ToolOutput::Json(value),
            })
            .map_err(ToolExecutionError::from_error)
    }
}
