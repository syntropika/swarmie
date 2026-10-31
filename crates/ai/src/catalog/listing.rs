//! Pure discovery-response parsing. The caller owns HTTP, credentials and pagination.
use crate::catalog::{
    ModelLimits, ResolutionError,
    model_schema::{identifier, unique_json},
};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ListingProtocol {
    OpenAi,
    Anthropic,
    Gemini,
    Ollama,
}

/// A listed identity, not a capability or inference entitlement.
#[derive(Clone, Debug, PartialEq)]
pub struct ListedModel {
    pub id: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub limits: ModelLimits,
    /// Original row, including revision, owner and future provider fields.
    pub raw: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelListing {
    pub models: Vec<ListedModel>,
    pub has_more: Option<bool>,
    /// Gemini's nextPageToken. Other protocols use their own cursor contracts.
    pub next_page_token: Option<String>,
    pub first_id: Option<String>,
    pub last_id: Option<String>,
    /// Complete response, retaining pagination and unknown envelope fields.
    pub raw: Value,
}

impl ModelListing {
    /// Parse one bounded page. No ID normalization, source merge or network I/O.
    /// Duplicate identities and malformed reported limits fail the whole page.
    pub fn from_json(protocol: ListingProtocol, bytes: &[u8]) -> Result<Self, ResolutionError> {
        let raw = unique_json(bytes).map_err(|_| ResolutionError::InvalidSource)?;
        let object = raw.as_object().ok_or(ResolutionError::InvalidSource)?;
        let key = match protocol {
            ListingProtocol::OpenAi | ListingProtocol::Anthropic => "data",
            ListingProtocol::Gemini | ListingProtocol::Ollama => "models",
        };
        let rows = object
            .get(key)
            .and_then(Value::as_array)
            .ok_or(ResolutionError::InvalidSource)?;
        let mut seen = BTreeSet::new();
        let mut models = Vec::with_capacity(rows.len());
        for row in rows {
            let object = row.as_object().ok_or(ResolutionError::InvalidSource)?;
            let id_key = match protocol {
                ListingProtocol::Gemini => "name",
                ListingProtocol::Ollama => "model",
                _ => "id",
            };
            let id = object
                .get(id_key)
                .and_then(Value::as_str)
                .filter(|id| identifier(id))
                .ok_or(ResolutionError::InvalidSource)?;
            if !seen.insert(id) {
                return Err(ResolutionError::InvalidSource);
            }
            let name_key = match protocol {
                ListingProtocol::Gemini => "displayName",
                ListingProtocol::Ollama => "name",
                _ => "display_name",
            };
            let mut limits = ModelLimits::default();
            if protocol == ListingProtocol::Gemini {
                limits.input = optional_limit(row, "inputTokenLimit")?;
                limits.output = optional_limit(row, "outputTokenLimit")?;
            } else if protocol == ListingProtocol::Anthropic {
                limits.input = optional_limit(row, "max_input_tokens")?;
                limits.output = optional_limit(row, "max_tokens")?;
            }
            models.push(ListedModel {
                id: id.to_owned(),
                display_name: optional_string(row, name_key)?,
                description: optional_string(row, "description")?,
                limits,
                raw: row.clone(),
            });
        }
        let has_more = match raw.get("has_more") {
            None | Some(Value::Null) => None,
            Some(Value::Bool(value)) => Some(*value),
            _ => return Err(ResolutionError::InvalidSource),
        };
        Ok(Self {
            models,
            has_more,
            next_page_token: optional_string(&raw, "nextPageToken")?,
            first_id: optional_string(&raw, "first_id")?,
            last_id: optional_string(&raw, "last_id")?,
            raw,
        })
    }
}

fn optional_string(row: &Value, key: &str) -> Result<Option<String>, ResolutionError> {
    match row.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        _ => Err(ResolutionError::InvalidSource),
    }
}

fn optional_limit(row: &Value, key: &str) -> Result<Option<u64>, ResolutionError> {
    match row.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .map(|value| (value > 0).then_some(value))
            .ok_or(ResolutionError::InvalidSource),
    }
}
