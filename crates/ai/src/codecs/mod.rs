//! Provider request encoding and response decoding without transport authority.
//! These values do not admit an operation or grant account access.
//! Native records retain their provenance and edit fingerprints. Encoder facts
//! are captured data, distinct from the operational Catalog's qualifications.
//! History projection helpers require an explicit caller policy; they must not
//! silently repair the history behind an admitted physical request.

pub mod catalog;
pub mod completion;
mod driver;
pub mod error;
pub mod http_client;
pub mod id;
pub mod json_utils;
pub mod model;
pub mod observe;
pub mod operation;
mod provider_response;
pub mod providers;
pub mod redact;
pub mod rerank;
pub mod streaming;
pub mod telemetry;
pub mod transcription;
pub mod wasm_compat;
pub mod wire;

pub mod embeddings {
    mod embedding;
    pub use embedding::*;
}

pub mod conversation {
    pub mod pairing;
}

pub use completion::message;
pub use error::{ErrorReport, ProviderError};
