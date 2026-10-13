//! Execution-owner lifetime, runtime transports, and service integration.
pub(crate) mod mcp_bridge;
pub(crate) mod runtime_host;
pub(crate) mod service;
pub(crate) mod transport;
#[cfg(windows)]
pub(crate) mod windows_pipe;

#[cfg(windows)]
pub(crate) mod windows_spawn;
