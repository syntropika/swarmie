//! Minimal Swarmie bootstrap over the real Tembra host.

mod node;
mod runtime;

pub use node::{
    NODE_KEY, NODE_PROTOCOL_VERSION, NODE_SERVICE, NodeId, NodeIdentity, NodeMethod, node_orb,
};
pub use runtime::Runtime;

#[cfg(feature = "rpc")]
pub use tembra::rpc;
