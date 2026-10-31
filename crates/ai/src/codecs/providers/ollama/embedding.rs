use crate::codecs::error::EncodeError;
use crate::codecs::operation::Embedding;
use crate::codecs::wire::{
    Body, Capabilities, Descriptor, DocumentDecoder, Encoded, Framing, Mode, Wire,
};
use serde::{Deserialize, Serialize};

use super::{OllamaConfig, PROVIDER_NAME};

pub const ALL_MINILM: &str = "all-minilm";
pub const NOMIC_EMBED_TEXT: &str = "nomic-embed-text";
pub const MXBAI_EMBED_LARGE: &str = "mxbai-embed-large";
pub const BGE_M3: &str = "bge-m3";
pub const EMBEDDINGGEMMA: &str = "embeddinggemma";
pub const QWEN3_EMBEDDING: &str = "qwen3-embedding";

fn model_dimensions_from_identifier(identifier: &str) -> Option<usize> {
    match identifier {
        ALL_MINILM => Some(384),
        NOMIC_EMBED_TEXT => Some(768),
        MXBAI_EMBED_LARGE => Some(1024),
        BGE_M3 => Some(1024),
        EMBEDDINGGEMMA => Some(768),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingResponse {
    pub model: String,
    pub embeddings: Vec<Vec<f64>>,
    #[serde(default)]
    pub total_duration: Option<u64>,
    #[serde(default)]
    pub load_duration: Option<u64>,
    #[serde(default)]
    pub prompt_eval_count: Option<u64>,
}

const MAX_DOCUMENTS: usize = 1024;

impl OllamaConfig {
    pub fn embedding(&self, model: impl Into<String>, ndims: Option<usize>) -> Embeddings {
        let model = model.into();
        let ndims = ndims
            .or_else(|| model_dimensions_from_identifier(&model))
            .unwrap_or_default();
        Embeddings {
            provider: self.clone(),
            model,
            ndims,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Embeddings {
    pub provider: OllamaConfig,
    pub model: String,
    pub ndims: usize,
}

impl Wire for Embeddings {
    type Op = Embedding;
    type Payload = crate::codecs::wire::Encoded;
    type Frame = crate::codecs::wire::WireFrame;
    type Event = EmbeddingResponse;
    type Decoder<'id> = DocumentDecoder<Embedding, Self::Event>;
    type Reassembler = crate::codecs::wire::document::Unreassembled;

    fn describe(&self) -> Descriptor<'_> {
        Descriptor::new(PROVIDER_NAME)
            .model(self.model.as_str())
            .capabilities(Capabilities::embedding(MAX_DOCUMENTS, self.ndims))
    }

    fn encode(&self, texts: Vec<String>, _mode: Mode) -> Result<Encoded, EncodeError> {
        let body = serde_json::json!({ "model": self.model, "input": texts });
        let request = self
            .provider
            .request(http::Method::POST, "/api/embed")
            .body(Body::Bytes(serde_json::to_vec(&body)?))?;
        Ok(Encoded::new(request, Framing::Whole))
    }

    fn decoder<'id>(&self) -> Self::Decoder<'id> {
        DocumentDecoder::new(
            |body| {
                crate::codecs::providers::internal::wire::classify_marker_keyed_frame(
                    body,
                    &["embeddings"],
                )
            },
            |reply| {
                // Ollama counts the prompt it embedded and nothing else: every token
                // of an embedding is input.
                let usage = crate::codecs::completion::Usage {
                    input_tokens: reply.prompt_eval_count,
                    total_tokens: reply.prompt_eval_count,
                    ..Default::default()
                };
                Ok(crate::codecs::embeddings::EmbeddingResponse {
                    model: Some(reply.model),
                    usage,
                    ..crate::codecs::embeddings::EmbeddingResponse::from_vectors(reply.embeddings)
                })
            },
        )
    }
}
