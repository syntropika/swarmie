use serde_json::{Map, Value, json};

use crate::codecs::embeddings;
use crate::codecs::error::EncodeError;
use crate::codecs::providers::internal::wire::classify_marker_keyed_frame;
use crate::codecs::wire::{
    Body, Capabilities, Descriptor, DocumentDecoder, Encoded, Framing, Mode, Wire,
};

pub const EMBEDDING_001: &str = "gemini-embedding-001";
pub const EMBEDDING_004: &str = "text-embedding-004";

pub fn default_ndims(model: &str) -> usize {
    match model {
        EMBEDDING_001 => 3072,
        _ => 768,
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Embeddings {
    pub provider: super::GeminiConfig,
    pub model: String,
    pub ndims: usize,
}

impl Embeddings {
    pub fn new(
        provider: super::GeminiConfig,
        model: impl Into<String>,
        ndims: Option<usize>,
    ) -> Self {
        let model = model.into();
        let ndims = ndims.unwrap_or_else(|| default_ndims(&model));
        Self {
            provider,
            model,
            ndims,
        }
    }
}

impl Wire for Embeddings {
    type Op = crate::codecs::operation::Embedding;
    type Payload = crate::codecs::wire::Encoded;
    type Frame = crate::codecs::wire::WireFrame;
    type Event = gemini_api_types::EmbeddingResponse;
    type Decoder<'id> = DocumentDecoder<Self::Op, Self::Event>;
    type Reassembler = crate::codecs::wire::document::Unreassembled;

    fn describe(&self) -> Descriptor<'_> {
        Descriptor::new(super::PROVIDER_NAME)
            .model(self.model.as_str())
            .capabilities(Capabilities::embedding(1024, self.ndims))
    }

    fn encode(&self, request: Vec<String>, _mode: Mode) -> Result<Encoded, EncodeError> {
        let model = format!("models/{}", self.model);
        let requests = request.iter().map(|doc| {
            json!({
                "model": model,
                "content": { "parts": [{ "text": doc }] },
                "output_dimensionality": self.ndims,
            })
        });
        let body = Value::Object(Map::from_iter([(
            "requests".to_owned(),
            Value::Array(requests.collect()),
        )]));

        let request = http::Request::post(format!(
            "{}/v1beta/models/{}:batchEmbedContents?key={}",
            self.provider.base_url,
            self.model,
            self.provider.api_key.expose()
        ))
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(Body::Bytes(serde_json::to_vec(&body)?))?;
        // `batchEmbedContents` has no streaming variant, so a streamed call
        // sends the same bytes and reads the same whole reply.
        Ok(Encoded::new(request, Framing::Whole))
    }

    fn decoder<'id>(&self) -> Self::Decoder<'id> {
        DocumentDecoder::new(
            |body| classify_marker_keyed_frame(body, &["embeddings"]),
            |reply| {
                let vectors = reply
                    .embeddings
                    .into_iter()
                    .map(|embedding| embedding.values);
                // Gemini supplies no usage or response id; the driver attaches the raw body.
                Ok(embeddings::EmbeddingResponse::from_vectors(vectors))
            },
        )
    }
}

pub mod gemini_api_types {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct EmbeddingResponse {
        pub embeddings: Vec<EmbeddingValues>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct EmbeddingValues {
        #[serde(default)]
        pub values: Vec<f64>,
    }
}

impl super::GeminiConfig {
    pub fn embedding(&self, model: impl Into<String>, ndims: Option<usize>) -> Embeddings {
        Embeddings::new(self.clone(), model, ndims)
    }
}
