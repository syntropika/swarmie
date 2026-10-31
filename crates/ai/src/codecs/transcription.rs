use crate::codecs::completion::Usage;
use crate::codecs::json_utils;
use serde::{Deserialize, Serialize};
use std::io;
use std::{fs, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResponse {
    pub text: String,
    #[serde(default)]
    pub usage: Usage,
    pub provider: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub segments: Vec<TranscriptSegment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub words: Vec<TranscriptWord>,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub raw: serde_json::Value,
}

impl TranscriptionResponse {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            usage: Usage::default(),
            provider: String::new(),
            model: None,
            response_id: None,
            provider_request_id: None,
            language: None,
            duration: None,
            confidence: None,
            segments: Vec::new(),
            words: Vec::new(),
            raw: serde_json::Value::Null,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptSegment {
    pub start: f64,
    pub end: f64,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptWord {
    pub word: String,
    pub start: f64,
    pub end: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
}

pub struct TranscriptionRequest {
    pub data: Vec<u8>,
    pub filename: String,
    pub language: Option<String>,
    pub prompt: Option<String>,
    pub temperature: Option<f64>,
    pub additional_params: Option<serde_json::Value>,
}

const DEFAULT_FILENAME: &str = "file";

pub struct TranscriptionRequestBuilder {
    request: TranscriptionRequest,
}

impl TranscriptionRequestBuilder {
    pub fn new(data: Vec<u8>) -> Self {
        Self {
            request: TranscriptionRequest {
                data,
                filename: DEFAULT_FILENAME.to_owned(),
                language: None,
                prompt: None,
                temperature: None,
                additional_params: None,
            },
        }
    }

    pub fn from_file(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        let filename = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        Ok(Self::new(fs::read(path)?).filename(filename))
    }

    pub fn filename(mut self, filename: impl Into<Option<String>>) -> Self {
        self.request.filename = filename
            .into()
            .unwrap_or_else(|| DEFAULT_FILENAME.to_owned());
        self
    }

    pub fn language(mut self, language: String) -> Self {
        self.request.language = Some(language);
        self
    }

    pub fn prompt(mut self, prompt: String) -> Self {
        self.request.prompt = Some(prompt);
        self
    }

    pub fn temperature(mut self, temperature: f64) -> Self {
        self.request.temperature = Some(temperature);
        self
    }

    pub fn additional_params(mut self, params: impl Into<Option<serde_json::Value>>) -> Self {
        self.request.additional_params =
            json_utils::merge_params(self.request.additional_params.take(), params.into());
        self
    }

    pub fn build(self) -> TranscriptionRequest {
        self.request
    }
}
