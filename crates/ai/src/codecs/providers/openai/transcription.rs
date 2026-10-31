use crate::codecs::completion::Usage;
use crate::codecs::error::ProviderError;
use crate::codecs::json_utils;
use crate::codecs::transcription::{self, TranscriptSegment, TranscriptWord};
use serde::{Deserialize, Serialize};

pub const WHISPER_1: &str = "whisper-1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResponse {
    pub text: String,
    #[serde(default)]
    pub usage: Option<TranscriptionUsage>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default, deserialize_with = "json_utils::null_or_default")]
    pub segments: Vec<TranscriptionSegment>,
    #[serde(default, deserialize_with = "json_utils::null_or_default")]
    pub words: Vec<TranscriptionWord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TranscriptionSegment {
    #[serde(default)]
    pub start: Option<f64>,
    #[serde(default)]
    pub end: Option<f64>,
    pub text: String,
    #[serde(default, alias = "speaker_id")]
    pub speaker: Option<Speaker>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TranscriptionWord {
    pub word: String,
    pub start: f64,
    pub end: f64,
    #[serde(default)]
    pub speaker: Option<Speaker>,
    #[serde(default)]
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum Speaker {
    Index(u64),
    Label(String),
}

impl From<Speaker> for String {
    fn from(speaker: Speaker) -> Self {
        match speaker {
            Speaker::Index(index) => index.to_string(),
            Speaker::Label(label) => label,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum TranscriptionUsage {
    Duration {
        r#type: DurationTag,
        seconds: f64,
    },
    Tokens {
        r#type: TokensTag,
        input_tokens: u64,
        #[serde(default)]
        input_token_details: Option<TranscriptionInputTokenDetails>,
        output_tokens: u64,
        total_tokens: u64,
    },
    Other(serde_json::Value),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DurationTag {
    Duration,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TokensTag {
    Tokens,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct TranscriptionInputTokenDetails {
    #[serde(default)]
    pub audio_tokens: u64,
    #[serde(default)]
    pub text_tokens: u64,
}

impl TranscriptionResponse {
    pub fn normalize(self) -> Result<transcription::TranscriptionResponse, ProviderError> {
        let usage = match &self.usage {
            Some(TranscriptionUsage::Tokens {
                input_tokens,
                output_tokens,
                total_tokens,
                ..
            }) => Usage {
                input_tokens: Some(*input_tokens),
                output_tokens: Some(*output_tokens),
                total_tokens: Some(*total_tokens),
                ..Default::default()
            },
            // Duration billing reports no token counts; the seconds stay
            // reachable on the raw payload.
            Some(TranscriptionUsage::Duration { .. })
            | Some(TranscriptionUsage::Other(_))
            | None => Usage::default(),
        };
        // A segment without both times is not a timed segment; its text is
        // still in `text`.
        let segments = self
            .segments
            .into_iter()
            .filter_map(|segment| {
                Some(TranscriptSegment {
                    start: segment.start?,
                    end: segment.end?,
                    text: segment.text,
                    speaker: segment.speaker.map(String::from),
                })
            })
            .collect();
        let words = self
            .words
            .into_iter()
            .map(|word| TranscriptWord {
                word: word.word,
                start: word.start,
                end: word.end,
                speaker: word.speaker.map(String::from),
                confidence: word.confidence,
            })
            .collect();
        Ok(transcription::TranscriptionResponse {
            usage,
            language: self.language,
            duration: self.duration,
            confidence: self.confidence,
            segments,
            words,
            ..transcription::TranscriptionResponse::new(self.text)
        })
    }
}
