//! Provider protocol preparation without account orchestration or Tembra composition.
//!
//! [`Model`] captures an immutable Catalog snapshot and explicit credentials.
//! [`Prepared`] owns the final destination, headers and body. A host inspects and
//! admits those values before dispatch, supplies the Tokio runtime and retains
//! [`Execution`] until settlement. Presentation receives bounded previews only.
//! Dropping previews does not cancel decoding; [`Execution::stop`] requests local
//! transport termination. Neither disconnect nor Stop proves remote cancellation
//! or permits replay. Dropping the result owner loses result delivery.
//!
//! [`Model::prepare`] accepts typed completions or explicit protocol JSON through
//! one frozen preparation and execution path. [`codecs`] also exposes the pure
//! encoding and decoding interfaces without transport authority. Account selection,
//! input measurement, custody, refresh and durable receipts belong to the caller.
mod codec;
pub mod codecs;
mod credentials;
mod document;
mod error;
mod execution;
mod framing;
mod preparation;
mod protocol;
mod stream;

pub use credentials::{CredentialFuture, CredentialSource, Credentials, StaticCredentials};
pub use error::{Effect, Error, Failure};
pub use execution::{Execution, Outcome, StopHandle};
pub use preparation::{
    Admitted, AuthProfile, Description, Model, Prepared, Request, Timeouts, WireMode,
};

/// SDK types are available without its HTTP client, retries or environment auth.
pub use async_openai::types::{chat as chat_types, responses as response_types};
pub use protocol::{ChatProfile, Protocol};

/// Shared model facts and the existing operational snapshot cache.
pub mod catalog;

/// Shared canonical completion records and reported usage values.
pub use codecs::completion::{CompletionRequest, CompletionResponse, Usage};
