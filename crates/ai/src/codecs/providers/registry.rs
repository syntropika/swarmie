//! Protocol identity and dialect lookup without account selection.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    OpenAi,
    Anthropic,
    Gemini,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderId {
    vendor: String,
    format: Option<Format>,
}

impl ProviderId {
    pub fn catalog(vendor: &str) -> Option<Self> {
        Some(Self {
            vendor: vendor.to_owned(),
            format: match vendor {
                "openai" | "deepseek" | "moonshotai" | "ollama" => Some(Format::OpenAi),
                "anthropic" => Some(Format::Anthropic),
                "google" => Some(Format::Gemini),
                _ => None,
            },
        })
    }
    pub fn vendor(&self) -> &str {
        &self.vendor
    }
    pub fn format(&self) -> Option<Format> {
        self.format
    }
    pub(crate) fn catalog_priced(&self) -> bool {
        false
    }
}

pub(crate) const OPENAI_DIALECTS: &[&super::openai::wire::Dialect] = &[
    &super::openai::wire::OPENAI,
    &super::openai::wire::DEEPSEEK,
    &super::openai::wire::MOONSHOT,
    &super::openai::wire::OLLAMA,
];
