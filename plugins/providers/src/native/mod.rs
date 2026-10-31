//! Native provider settings, credential references and physically owned file I/O.
mod account_file;
mod credential_keys;
mod protection;
mod provider_io;
mod settings;

pub use account_file::NativeAccountFile;
pub use credential_keys::{NativeCredentialKeys, NativeCredentialKeyspace};
pub use protection::{MODEL_PROTECTION_SERVICE, NativeModelProtection};
pub use provider_io::{NativeProviderIo, NativeProviderLease};
pub use settings::NativeProviders;

/// Principal-scoped native authorization service.
pub const AUTHORIZATION_SERVICE: &str = settings::oauth::SERVICE;
