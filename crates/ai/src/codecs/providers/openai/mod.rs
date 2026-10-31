pub const PROVIDER_NAME: &str = "openai";

pub mod completion;
pub mod embedding;
pub mod extension;
pub(crate) mod options;
pub mod responses_api;

pub mod wire;

pub use wire::{OpenAIConfig, Route};

pub mod transcription;

pub use completion::*;
pub use embedding::*;

pub(crate) fn sanitize_schema(schema: &mut serde_json::Value) {
    crate::codecs::providers::internal::schema::sanitize_schema(
        schema,
        crate::codecs::providers::internal::schema::SanitizeOptions {
            strip_ref_siblings: true,
            inject_empty_properties: true,
            strip_numeric_constraints: false,
        },
    );
}

pub(crate) fn sanitize_tool_schema(
    schema: &mut serde_json::Value,
    quirks: &wire::Quirks,
) -> Result<(), &'static str> {
    match quirks.strict_schema {
        wire::StrictSchema::OpenAi => {
            sanitize_schema(schema);
            Ok(())
        }
        wire::StrictSchema::DeepSeek => crate::codecs::providers::deepseek::strict_schema(schema),
    }
}

pub(crate) fn structured_output_schema(schema: schemars::Schema) -> (String, serde_json::Value) {
    let name = schema
        .as_object()
        .and_then(|object| object.get("title"))
        .and_then(|title| title.as_str())
        .unwrap_or("response_schema")
        .to_string();
    let mut value = schema.to_value();
    sanitize_schema(&mut value);
    (name, value)
}

pub use transcription::*;

pub(crate) fn reads_images(model: &str) -> bool {
    const TEXT_ONLY: [&str; 9] = [
        "gpt-3.5",
        "gpt-4-32k",
        "gpt-4-0125-preview",
        "gpt-4-1106-preview",
        "gpt-4-turbo-preview",
        "o1-mini",
        "o1-preview",
        "o3-mini",
        "gpt-5.3-codex-spark",
    ];
    let model = model.to_ascii_lowercase();
    !(matches!(model.as_str(), "gpt-4" | "gpt-4-0613" | "gpt-4-0314")
        || TEXT_ONLY.iter().any(|prefix| model.starts_with(prefix)))
}
