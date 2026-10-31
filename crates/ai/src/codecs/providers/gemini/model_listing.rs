use crate::codecs::error::EncodeError;
use crate::codecs::error::ProviderError;
use crate::codecs::json_utils::Lenient;
use crate::codecs::wire::Flow;
use crate::codecs::wire::Wire;
use crate::codecs::{
    model::{ModelInfo, ModelList, listing},
    operation::{ModelListing, ModelPage, Verify},
    providers::internal::{wire::classify_marker_keyed_frame, with_query_pairs},
    wire::{
        Body, Decoder, Descriptor, DocumentDecoder, Encoded, Framing, Mode, Out, WireEvent,
        WireFrame,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_PAGE_SIZE: usize = 1000;

fn model_of(entry: &Value) -> Option<ModelInfo> {
    let id = entry
        .str("baseModelId")
        .map(str::trim)
        .filter(|id| !id.is_empty());
    let name = entry.str("name").map(str::trim).unwrap_or_default();
    let name = name.strip_prefix("models/").unwrap_or(name);
    let id = id.or((!name.is_empty()).then_some(name))?;
    let limit = |key: &str| entry.u64(key).and_then(|limit| u32::try_from(limit).ok());
    let mut model = ModelInfo::from_id(id.to_owned());
    model.name = entry.str("displayName").map(str::to_owned);
    model.description = entry.str("description").map(str::to_owned);
    model.context_length = limit("inputTokenLimit");
    model.max_output_tokens = limit("outputTokenLimit");
    Some(model)
}

fn list_models_path(page_token: Option<&str>) -> String {
    let page_size = MAX_PAGE_SIZE.to_string();
    let mut pairs = vec![("pageSize", page_size.as_str())];
    if let Some(page_token) = page_token {
        pairs.push(("pageToken", page_token));
    }
    with_query_pairs("/v1beta/models", &pairs)
}

#[derive(Debug)]
struct ListingPage {
    models: Vec<ModelInfo>,
    next_cursor: Option<String>,
}

fn parse_models_page(body: &[u8], path: &str) -> Result<ListingPage, ProviderError> {
    let error =
        |details: &dyn std::fmt::Display| listing::parse_error("Gemini", path, details, body);
    let page: Value = serde_json::from_slice(body)
        .map_err(|parse| error(&format_args!("parse_error={parse}")))?;
    if page.get("models").is_some_and(|models| !models.is_array()) {
        return Err(error(&"parse_error=`models` is not a list"));
    }
    let models = page
        .arr("models")
        .iter()
        .map(|entry| {
            model_of(entry).ok_or_else(|| {
                error(&"parse_error=model entry missing usable `baseModelId` and `name` values")
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    // Sending an empty cursor would repeatedly fetch the same page.
    Ok(ListingPage {
        models,
        next_cursor: page
            .str("nextPageToken")
            .filter(|token| !token.is_empty())
            .map(str::to_owned),
    })
}

impl super::GeminiConfig {
    pub fn models(&self) -> Models {
        Models::new(self.clone())
    }

    pub fn verify(&self) -> VerifyKey {
        VerifyKey::new(self.clone())
    }
}

const MODEL_PAGE_MARKERS: &[&str] = &["models", "nextPageToken"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Auth {
    Query,
    Header,
}

fn list_models_request(
    provider: &super::GeminiConfig,
    auth: Auth,
    page_token: Option<&str>,
) -> Result<http::Request<Body>, EncodeError> {
    let path = list_models_path(page_token);
    let trimmed = path.trim_start_matches('/');
    let base_url = &provider.base_url;
    // The page-size parameter guarantees an existing query string.
    let request = match auth {
        Auth::Query => http::Request::get(format!(
            "{base_url}/{trimmed}&key={}",
            provider.api_key.expose()
        )),
        Auth::Header => http::Request::get(format!("{base_url}/{trimmed}"))
            .header("x-goog-api-key", provider.api_key.expose()),
    };
    request.body(Body::empty()).map_err(EncodeError::from)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Models {
    pub provider: super::GeminiConfig,
}

impl Models {
    pub fn new(provider: super::GeminiConfig) -> Self {
        Self { provider }
    }
}

impl Wire for Models {
    type Op = ModelListing;
    type Payload = crate::codecs::wire::Encoded;
    type Frame = crate::codecs::wire::WireFrame;
    type Event = String;
    type Decoder<'id> = DocumentDecoder<ModelListing, String>;
    type Reassembler = crate::codecs::wire::document::Unreassembled;

    fn describe(&self) -> Descriptor<'_> {
        Descriptor::new(super::PROVIDER_NAME)
    }

    fn encode(&self, cursor: Option<String>, _mode: Mode) -> Result<Encoded, EncodeError> {
        Ok(Encoded::new(
            list_models_request(&self.provider, Auth::Query, cursor.as_deref())?,
            Framing::Whole,
        ))
    }

    fn decoder<'id>(&self) -> Self::Decoder<'id> {
        models_decoder()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InteractionsModels {
    pub provider: super::GeminiConfig,
}

impl InteractionsModels {
    pub fn new(provider: super::GeminiConfig) -> Self {
        Self { provider }
    }
}

impl Wire for InteractionsModels {
    type Op = ModelListing;
    type Payload = crate::codecs::wire::Encoded;
    type Frame = crate::codecs::wire::WireFrame;
    type Event = String;
    type Decoder<'id> = DocumentDecoder<ModelListing, String>;
    type Reassembler = crate::codecs::wire::document::Unreassembled;

    fn describe(&self) -> Descriptor<'_> {
        Descriptor::new(super::PROVIDER_NAME)
    }

    fn encode(&self, cursor: Option<String>, _mode: Mode) -> Result<Encoded, EncodeError> {
        Ok(Encoded::new(
            list_models_request(&self.provider, Auth::Header, cursor.as_deref())?,
            Framing::Whole,
        ))
    }

    fn decoder<'id>(&self) -> Self::Decoder<'id> {
        models_decoder()
    }
}

fn models_decoder() -> DocumentDecoder<ModelListing, String> {
    DocumentDecoder::new(
        |body| {
            classify_marker_keyed_frame::<Value>(body, MODEL_PAGE_MARKERS).map(|_| body.to_owned())
        },
        |payload| {
            // Diagnostics identify the endpoint without retaining the previous page's cursor.
            let page = parse_models_page(payload.as_bytes(), &list_models_path(None))?;
            Ok(ModelPage {
                models: ModelList::new(page.models),
                next: page.next_cursor,
            })
        },
    )
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VerifyKey {
    pub provider: super::GeminiConfig,
}

impl VerifyKey {
    pub fn new(provider: super::GeminiConfig) -> Self {
        Self { provider }
    }
}

impl Wire for VerifyKey {
    type Op = Verify;
    type Payload = crate::codecs::wire::Encoded;
    type Frame = crate::codecs::wire::WireFrame;
    type Event = ();
    type Decoder<'id> = VerifyKeyDecoder;
    type Reassembler = crate::codecs::wire::document::Unreassembled;

    fn describe(&self) -> Descriptor<'_> {
        Descriptor::new(super::PROVIDER_NAME)
    }

    fn encode(&self, _request: (), _mode: Mode) -> Result<Encoded, EncodeError> {
        let request = http::Request::get(format!(
            "{}/v1beta/models?key={}",
            self.provider.base_url,
            self.provider.api_key.expose()
        ))
        .body(Body::empty())?;
        Ok(Encoded::new(request, Framing::Whole))
    }

    fn decoder<'id>(&self) -> Self::Decoder<'id> {
        VerifyKeyDecoder
    }
}

#[derive(Default)]
pub struct VerifyKeyDecoder;

impl<'id> Decoder<'id, Verify> for VerifyKeyDecoder {
    type Event = ();

    fn classify(&self, frame: WireFrame) -> WireEvent<Self::Event> {
        classify_marker_keyed_frame::<Value>(&frame.as_str(), MODEL_PAGE_MARKERS).map(|_| ())
    }

    fn decode(
        &mut self,
        _event: Self::Event,
        out: Out<'id, Verify>,
    ) -> Result<Flow, ProviderError> {
        Ok(out.end(()))
    }

    fn eof(&mut self, out: Out<'id, Verify>) -> Result<Flow, ProviderError> {
        Ok(out.end(()))
    }
}
