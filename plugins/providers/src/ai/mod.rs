//! Configured swarmie-ai inference through the neutral model contract and native Tembra.
//! The embedding host supplies Tokio's I/O runtime; this library creates no executor.
//!
//! ```
//! use std::rc::Rc;
//! use swarmie_providers::ai::{
//!     Configuration, Connection, ConnectionId, EnvironmentCredentials,
//!     ModelId, ModelKey, Provider, ModelProtection, orb,
//! };
//!
//! fn configured_provider(metadata: impl Fn(&Connection,&ModelId)->tembra::Result<std::sync::Arc<swarmie_providers::ai::catalog::Snapshot>> + 'static) -> tembra::Result<(tembra::Orb, ModelKey)> {
//!     let key = ModelKey {
//!         connection: ConnectionId::new("primary")?,
//!         model: ModelId::new("chosen-model")?,
//!     };
//!     let mut connection = Connection::new(
//!         key.connection.clone(), Provider::OpenAi, [key.model.clone()],
//!     );
//!     connection.credential_key = Some("MODEL_API_KEY".into());
//!     Ok((orb(Configuration { connections: vec![connection] },
//!         Rc::new(EnvironmentCredentials), ModelProtection::Unprotected,metadata)?, key))
//! }
//! ```
pub use swarmie_ai::catalog;
mod accounts;
mod configuration;
mod conversion;
mod error;
mod model;
mod orb;

pub use accounts::{AiAccountDriver, AiModelFactory};
pub use configuration::{
    Configuration, Connection, ConnectionId, Credentials, EnvironmentCredentials, ModelId,
    ModelKey, Provider,
};
pub use model::{AdmittedAttempt, AiModel, ModelCredential, ModelDecisions, ModelProtection};
pub use orb::{SERVICE, orb};

/// Scripted providers are opt-in and share the production model contract.
#[cfg(feature = "mock")]
pub use crate::mock;
