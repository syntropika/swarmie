//! Desktop-owned worker, byte channels and C ABI for reusable client node services.
mod ffi;
mod git;
mod node;

pub use node::DesktopNode;

pub use ffi::{
    swarmie_desktop_runtime_close, swarmie_desktop_runtime_exchange,
    swarmie_desktop_runtime_grant_directory, swarmie_desktop_runtime_start,
};
