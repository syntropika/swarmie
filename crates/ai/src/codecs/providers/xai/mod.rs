pub const GROK_3: &str = "grok-3";
pub const GROK_3_MINI: &str = "grok-3-mini";
pub const GROK_2_IMAGE_1212: &str = "grok-2-image-1212";
pub const GROK_4: &str = "grok-4-0709";

use crate::codecs::providers::openai::responses_api::SystemInstructionsPlacement;
use crate::codecs::providers::openai::wire::{
    Dialect, ImageBody, Quirks, ResponsesContract, ResponsesQuirks, Route, SpeechBody,
};

pub const PROVIDER_NAME: &str = "xai";

pub const DIALECT: Dialect = Dialect {
    request_id_header: Some("x-request-id"),
    quirks: Quirks {
        completion_route: Route::Responses,
        completion_path: "/v1/chat/completions",
        embeddings_path: "/v1/embeddings",
        models_path: "/v1/models",
        verify_path: "/v1/api-key",
        transcription_path: "/v1/audio/transcriptions",
        image_generation_path: "/v1/images/generations",
        audio_generation_path: "/v1/tts",
        image_body: ImageBody::Xai,
        speech_body: SpeechBody::Xai,
        responses: ResponsesQuirks {
            path: "/v1/responses",
            system_instructions: SystemInstructionsPlacement::InputSystemMessages,
            contract: ResponsesContract::Xai,
            ..ResponsesQuirks::openai()
        },
        ..Quirks::openai()
    },
    ..Dialect::gateway(PROVIDER_NAME, "https://api.x.ai", "XAI_API_KEY")
};

pub(crate) fn reads_images(model: &str) -> bool {
    let model = model.to_ascii_lowercase();
    !((model.starts_with("grok-2") && !model.contains("vision"))
        || model.starts_with("grok-3")
        || model.starts_with("grok-code"))
}
