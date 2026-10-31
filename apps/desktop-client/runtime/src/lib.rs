//! Desktop C and origin-restricted WebView adapters for the shared Rust node.
//! Account credentials and vault values have no native getter or log projection.

mod bridge;
mod ffi;

pub use bridge::*;
pub use ffi::{
    swarmie_desktop_runtime_close, swarmie_desktop_runtime_exchange,
    swarmie_desktop_runtime_grant_directory, swarmie_desktop_runtime_start,
};
pub use swarmie_node::Node;
