use std::path::Path;

use base64::{Engine, prelude::BASE64_STANDARD};
use serde_json::{Map, Value, json};

use super::completion::usage_of;
use crate::codecs::error::{EncodeError, ProviderError};
use crate::codecs::json_utils::Lenient;
use crate::codecs::operation::Transcription;
use crate::codecs::transcription;
use crate::codecs::wire::{Body, Descriptor, DocumentDecoder, Encoded, Framing, Mode, Wire};

const TRANSCRIPTION_PREAMBLE: &str =
    "Translate the provided audio exactly. Do not add additional information.";

fn transcription_body(
    request: transcription::TranscriptionRequest,
) -> Result<Vec<u8>, EncodeError> {
    let mut generation_config = match request.additional_params {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(config)) => config,
        Some(other) => {
            return Err(EncodeError::request(format!(
                "Gemini transcription `additional_params` should be an object, got {other}"
            )));
        }
    };
    // A temperature named on the request outranks one carried inside
    // `additional_params`.
    if let Some(temp) = request.temperature {
        generation_config.insert("temperature".to_owned(), Value::from(temp));
    }
    // The request supplies no explicit MIME type, so infer it from the filename.
    let mime_type = mime_guess::from_path(Path::new(&request.filename))
        .first()
        .map_or_else(|| "audio/mpeg".to_string(), |mime| mime.to_string());
    let mut body = json!({
        "contents": [{
            "parts": [{ "inlineData": { "mimeType": mime_type, "data": null }, "thought": false }],
            "role": "user",
        }],
        "generationConfig": generation_config,
        "safetySettings": null,
        "toolConfig": null,
        "systemInstruction": {
            "parts": [{ "text": TRANSCRIPTION_PREAMBLE, "thought": false }],
            "role": "model",
        },
    });
    // The audio moves in: `json!` would copy it.
    let data = body.pointer_mut("/contents/0/parts/0/inlineData/data");
    *data.ok_or_else(|| EncodeError::request("the body has no slot for the audio"))? =
        Value::String(BASE64_STANDARD.encode(request.data));
    Ok(serde_json::to_vec(&body)?)
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Transcriptions {
    pub provider: super::GeminiConfig,
    pub model: String,
}

impl Transcriptions {
    pub fn new(provider: super::GeminiConfig, model: impl Into<String>) -> Self {
        Self {
            provider,
            model: model.into(),
        }
    }
}

impl Wire for Transcriptions {
    type Op = Transcription;
    type Payload = crate::codecs::wire::Encoded;
    type Frame = crate::codecs::wire::WireFrame;
    type Event = Value;
    type Decoder<'id> = DocumentDecoder<Transcription, Value>;
    type Reassembler = crate::codecs::wire::document::Unreassembled;

    fn describe(&self) -> Descriptor<'_> {
        Descriptor::new(super::PROVIDER_NAME).model(self.model.as_str())
    }

    fn encode(
        &self,
        request: transcription::TranscriptionRequest,
        _mode: Mode,
    ) -> Result<Encoded, EncodeError> {
        let body = transcription_body(request)?;
        let request = http::Request::post(format!(
            "{}/v1beta/models/{}:generateContent?key={}",
            self.provider.base_url,
            self.model,
            self.provider.api_key.expose()
        ))
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(Body::Bytes(body))?;
        // Gemini reports no transport request-id header.
        Ok(Encoded::new(request, Framing::Whole))
    }

    fn decoder<'id>(&self) -> Self::Decoder<'id> {
        DocumentDecoder::new(super::completion::classify_reply, |reply| {
            transcript_of(&reply)
        })
    }
}

pub fn transcript_of(reply: &Value) -> Result<transcription::TranscriptionResponse, ProviderError> {
    let candidate = reply
        .arr("candidates")
        .first()
        .ok_or_else(|| ProviderError::Response("No response candidates in response".into()))?;
    let parts: Vec<&str> = candidate
        .get("content")
        .map(|content| content.arr("parts"))
        .unwrap_or_default()
        .iter()
        .filter(|part| part.bool("thought") != Some(true))
        .filter_map(|part| part.str("text"))
        .collect();
    if parts.is_empty() {
        return Err(ProviderError::Response(
            "Response content contains no text".to_string(),
        ));
    }
    Ok(transcription::TranscriptionResponse {
        model: reply.str("modelVersion").map(str::to_owned),
        response_id: Some(reply.str("responseId").unwrap_or_default().to_owned()),
        usage: reply.get("usageMetadata").map(usage_of).unwrap_or_default(),
        ..transcription::TranscriptionResponse::new(parts.concat())
    })
}

impl super::GeminiConfig {
    pub fn transcription(&self, model: impl Into<String>) -> Transcriptions {
        Transcriptions::new(self.clone(), model)
    }
}
