//! Native PKCE account session and origin-bound HTTPS, independent of the node vault.
//! The embedding owns browser presentation, actual user actions and its I/O runtime.
mod error;
mod login;
mod session;
mod storage;
mod transport;

pub use error::{Error, Result};
pub use login::*;
pub use session::*;
pub use storage::*;
pub use transport::*;

#[cfg(test)]
#[path = "../../../tests/fixtures/https-server.rs"]
mod https;
