use crate::codecs::error::EncodeError;
use crate::codecs::model::{ModelInfo, ModelList};
use crate::codecs::operation::{ModelListing, ModelPage};
use crate::codecs::wire::{Body, Descriptor, DocumentDecoder, Encoded, Framing, Mode, Wire};
use serde::{Deserialize, Serialize};

use super::{OllamaConfig, PROVIDER_NAME};

#[derive(Debug, Deserialize)]
pub struct ListModelsResponse {
    pub models: Vec<ListModelEntry>,
}

#[derive(Debug, Deserialize)]
pub struct ListModelEntry {
    pub name: String,
    pub model: String,
}

impl From<ListModelEntry> for ModelInfo {
    fn from(value: ListModelEntry) -> Self {
        ModelInfo::new(value.model, value.name)
    }
}

impl OllamaConfig {
    pub fn models(&self) -> Models {
        Models {
            provider: self.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Models {
    pub provider: OllamaConfig,
}

impl Wire for Models {
    type Op = ModelListing;
    type Payload = crate::codecs::wire::Encoded;
    type Frame = crate::codecs::wire::WireFrame;
    type Event = ListModelsResponse;
    type Decoder<'id> = DocumentDecoder<ModelListing, Self::Event>;
    type Reassembler = crate::codecs::wire::document::Unreassembled;

    fn describe(&self) -> Descriptor<'_> {
        Descriptor::new(PROVIDER_NAME)
    }

    fn encode(&self, _cursor: Option<String>, _mode: Mode) -> Result<Encoded, EncodeError> {
        let request = self
            .provider
            .request(http::Method::GET, "/api/tags")
            .body(Body::empty())?;
        Ok(Encoded::new(request, Framing::Whole))
    }

    fn decoder<'id>(&self) -> Self::Decoder<'id> {
        DocumentDecoder::new(
            |body| {
                crate::codecs::providers::internal::wire::classify_marker_keyed_frame(
                    body,
                    &["models"],
                )
            },
            |reply| {
                Ok(ModelPage {
                    models: ModelList::new(reply.models.into_iter().map(ModelInfo::from).collect()),
                    next: None,
                })
            },
        )
    }
}
