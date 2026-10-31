use crate::codecs::completion::Usage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingResponse {
    pub embeddings: Vec<Embedding>,
    #[serde(default)]
    pub usage: Usage,
    pub provider: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub raw: serde_json::Value,
}

impl EmbeddingResponse {
    pub fn new(embeddings: Vec<Embedding>) -> Self {
        Self {
            embeddings,
            usage: Usage::default(),
            provider: String::new(),
            model: None,
            response_id: None,
            provider_request_id: None,
            raw: serde_json::Value::Null,
        }
    }

    pub(crate) fn from_vectors(vectors: impl IntoIterator<Item = Vec<f64>>) -> Self {
        Self::new(
            vectors
                .into_iter()
                .map(|vec| Embedding {
                    document: String::new(),
                    vec,
                })
                .collect(),
        )
    }
}

#[derive(Clone, Default, Deserialize, Serialize, Debug)]
pub struct Embedding {
    pub document: String,
    pub vec: Vec<f64>,
}

impl PartialEq for Embedding {
    fn eq(&self, other: &Self) -> bool {
        self.document == other.document
    }
}

impl Eq for Embedding {}

pub fn image_media_type(bytes: &[u8]) -> Option<&'static str> {
    use crate::codecs::message::{ImageMediaType, MimeType};
    ImageMediaType::sniff(bytes).map(|media_type| media_type.to_mime_type())
}

pub fn image_document(bytes: &[u8]) -> String {
    use base64::Engine as _;
    use sha2::Digest as _;
    let media_type = image_media_type(bytes).unwrap_or("application/octet-stream");
    let digest = sha2::Sha256::digest(bytes);
    format!(
        "{media_type};sha256={}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest)
    )
}
