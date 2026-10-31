//! Read-only node identity supplied and persisted by the embedding.

use serde::{Deserialize, Serialize};
use tembra::{Code, Error, Orb, Service};

/// The public Swarmie node identity service.
pub const NODE_SERVICE: &str = "swarmie.node";

/// The only identity service key.
pub const NODE_KEY: &str = "main";

/// Version of the Swarmie node binding contract, independently of a route URL.
pub const NODE_PROTOCOL_VERSION: u32 = 1;

/// Stable identity selected by the embedding and reused across process restarts.
///
/// Constructing this value neither generates keys nor authenticates a route. The
/// embedding owns persistence and the channel's authentication policy.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct NodeId(String);

impl NodeId {
    /// Validate a nonempty, bounded identifier without whitespace.
    ///
    /// # Errors
    /// Empty identifiers, whitespace and values larger than 128 bytes are refused.
    pub fn new(value: impl Into<String>) -> tembra::Result<Self> {
        let value = value.into();
        if value.is_empty() || value.len() > 128 || value.chars().any(char::is_whitespace) {
            return Err(Error::invalid(
                "node_id",
                "node identity must contain 1..128 bytes without whitespace",
            ));
        }
        Ok(Self(value))
    }

    /// Borrow the persistent identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for NodeId {
    type Error = Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<NodeId> for String {
    fn from(value: NodeId) -> Self {
        value.0
    }
}

/// Identity expectations can be checked only after the embedding authenticates a channel.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeIdentity {
    /// Persistent identity, never inferred from the selected route.
    pub node_id: NodeId,
    /// Principal supplied by the authenticated invocation context.
    pub principal: String,
    /// Swarmie node binding contract version.
    pub protocol_version: u32,
}

/// Methods exposed by the read-only identity service.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodeMethod {
    /// Report this node and the caller's authenticated principal.
    Identify,
}

impl NodeMethod {
    /// Portable method spelling for Rust and JavaScript consumers.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Identify => "identify",
        }
    }
}

/// Compose the optional node binding service using ordinary managed Tembra lifetime.
///
/// The embedding registers this Orb through its normal composition before exposing
/// a channel. The bare runtime still bootstraps only Plugin Manager. Persistence,
/// TLS, credentials and transport serving are not introduced by this service.
pub fn node_orb(node_id: NodeId) -> Orb {
    Orb::new(NODE_SERVICE, NODE_PROTOCOL_VERSION).service(
        Service::new(NODE_SERVICE, move |declaration| {
            if declaration.address()?.key != [NODE_KEY] {
                return Err(Error::new(
                    Code::Forbidden,
                    "node_key",
                    "node identity uses the main key",
                ));
            }
            Ok(node_id.clone())
        })
        .method(
            NodeMethod::Identify.as_str(),
            |node_id, context, (): ()| async move {
                context.check()?;
                Ok(NodeIdentity {
                    node_id: (*node_id).clone(),
                    principal: context.principal()?.to_owned(),
                    protocol_version: NODE_PROTOCOL_VERSION,
                })
            },
        ),
    )
}

/// Register node identity through the runtime's existing plugin manager.
/// The embedding supplies the stable identity; this factory performs no I/O.
pub fn node_plugin(node_id: NodeId) -> swarmie_plugin_manager::CorePlugin {
    swarmie_plugin_manager::CorePlugin::new(
        swarmie_plugin_manager::PluginId::new(NODE_SERVICE).expect("fixed node plugin identity"),
        std::num::NonZeroU32::new(NODE_PROTOCOL_VERSION).expect("nonzero node protocol"),
        move |_| Ok(vec![node_orb(node_id.clone())]),
    )
}
