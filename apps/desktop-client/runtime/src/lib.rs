//! Native host execution and I/O for the ordinary Tembra MCP services.
mod definitions;
mod ffi;
mod node;

pub use node::DesktopNode;

pub use ffi::{
    swarmie_desktop_runtime_close, swarmie_desktop_runtime_exchange, swarmie_desktop_runtime_start,
};
