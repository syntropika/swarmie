use serde::{Deserialize, Serialize};

use super::wire::AnthropicConfig;
use crate::codecs::error::EncodeError;
use crate::codecs::model::{ModelInfo, ModelList};
pub use crate::codecs::operation::VerifyDecoder;
use crate::codecs::operation::{ModelListing, ModelPage, Verify as VerifyOp};
use crate::codecs::wire::{Body, Descriptor, DocumentDecoder, Encoded, Framing, Mode, Wire};

impl AnthropicConfig {
    pub fn models(&self) -> Models {
        Models {
            provider: self.clone(),
        }
    }

    pub fn verify(&self) -> Verify {
        Verify {
            provider: self.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Models {
    pub provider: AnthropicConfig,
}

impl Wire for Models {
    type Op = ModelListing;
    type Payload = crate::codecs::wire::Encoded;
    type Frame = crate::codecs::wire::WireFrame;
    type Event = ModelsPage;
    type Decoder<'id> = DocumentDecoder<ModelListing, ModelsPage>;
    type Reassembler = crate::codecs::wire::document::Unreassembled;

    fn describe(&self) -> Descriptor<'_> {
        Descriptor::new(self.provider.dialect.name)
    }

    fn encode(&self, cursor: Option<String>, _mode: Mode) -> Result<Encoded, EncodeError> {
        Ok(Encoded::new(
            self.models_request(cursor.as_deref())?,
            Framing::Whole,
        ))
    }

    fn decoder<'id>(&self) -> Self::Decoder<'id> {
        DocumentDecoder::new(
            |body| {
                crate::codecs::providers::internal::wire::classify_marker_keyed_frame(
                    body,
                    &["data"],
                )
            },
            |page| {
                // Missing or empty cursors would repeatedly fetch page one, even with has_more.
                let next = page
                    .last_id
                    .filter(|cursor| page.has_more && !cursor.is_empty());
                Ok(ModelPage {
                    models: ModelList::new(page.data.into_iter().map(ModelInfo::from).collect()),
                    next,
                })
            },
        )
    }
}

impl Models {
    fn models_request(&self, cursor: Option<&str>) -> Result<http::Request<Body>, EncodeError> {
        let uri = match cursor {
            Some(cursor) => format!(
                "{}{}",
                self.provider.base_url,
                crate::codecs::providers::internal::with_query_pairs(
                    "/v1/models",
                    &[("after_id", cursor)],
                )
            ),
            None => format!("{}/v1/models", self.provider.base_url),
        };
        self.provider
            .headers(http::Request::get(uri))
            .body(Body::empty())
            .map_err(EncodeError::from)
    }
}

#[derive(Debug, Deserialize)]
#[doc(hidden)]
pub struct ModelsPage {
    data: Vec<ModelEntry>,
    #[serde(default)]
    has_more: bool,
    #[serde(default)]
    last_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModelEntry {
    id: String,
    display_name: String,
}

impl From<ModelEntry> for ModelInfo {
    fn from(entry: ModelEntry) -> Self {
        ModelInfo::new(entry.id, entry.display_name)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Verify {
    pub provider: AnthropicConfig,
}

impl Wire for Verify {
    type Op = VerifyOp;
    type Payload = crate::codecs::wire::Encoded;
    type Frame = crate::codecs::wire::WireFrame;
    type Event = ();
    type Decoder<'id> = VerifyDecoder;
    type Reassembler = crate::codecs::wire::document::Unreassembled;

    fn describe(&self) -> Descriptor<'_> {
        Descriptor::new(self.provider.dialect.name)
    }

    fn encode(&self, _request: (), _mode: Mode) -> Result<Encoded, EncodeError> {
        let request = self
            .provider
            .headers(http::Request::get(format!(
                "{}/v1/models",
                self.provider.base_url
            )))
            .body(Body::empty())?;
        Ok(Encoded::new(request, Framing::Whole))
    }

    fn decoder<'id>(&self) -> Self::Decoder<'id> {
        VerifyDecoder
    }
}
