//! Node-owned secrets. Plaintext and authority handles never implement serialization.
//! The embedding supplies authenticated human and execution identities; network labels are not proof.

mod authority;
mod capture;
mod cloud_protocol;
mod crypto;
mod entry;
mod error;
mod management_protocol;
mod model;
mod node;
mod output;
mod storage;

pub mod keyring;

pub use authority::*;
pub use capture::*;
pub use cloud_protocol::*;
pub use crypto::{
    DeviceKeys, TrustBundle, TrustPins, VaultKey, canonical, digest, verify_signature,
};
pub use entry::*;
pub use error::{Error, Result};
pub use management_protocol::*;
pub use model::*;
pub use node::*;
pub use output::*;
pub use storage::*;

#[cfg(feature = "executors")]
pub mod executors;

#[cfg(feature = "cloud-client")]
pub mod cloud_http;

#[cfg(feature = "tools")]
pub mod tools;

#[cfg(feature = "tools")]
mod tools_schema;

#[cfg(feature = "tools")]
pub use tools_schema::definitions as tool_definitions;

#[cfg(feature = "provider-credentials")]
mod provider_credentials;
#[cfg(feature = "provider-credentials")]
pub use provider_credentials::VaultCredentials;
