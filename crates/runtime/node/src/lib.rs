//! Reusable native node owner with protected client capability composition.
//! Hosts own process lifetime. Client observation does not own admitted work.
//! Platform adapters authenticate private input before opening a private attachment.

mod cards;
mod configuration;
mod control;
mod git;
mod node;

pub use control::{NativeAgent, NativeExecution, NativeNode};
pub use node::{HostConfiguration, MAX_FRAME_BYTES, Node, PrivatePort};

/// Private control service. Ordinary byte channels cannot acquire this service.
pub const PRIVATE_CONTROL_SERVICE: &str = control::SERVICE;
