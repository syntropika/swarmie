//! Public purpose-limited ciphertext carrier contract.
use crate::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Signed setup carrier request. All payload bytes are inside end-to-end bootstrap TLS.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarrierRequest {
    /// Exact account authenticated by this native session.
    pub account: Uuid,
    /// Configured account issuer, bound independently of HTTP routing.
    pub issuer: String,
    /// Endpoint whose retained private key signs this operation.
    pub device: DeviceId,
    /// Exact current device registration generation.
    pub generation: u64,
    /// Exact bounded setup action.
    pub action: CarrierAction,
    /// Domain-separated endpoint signature.
    pub signature: Vec<u8>,
}
impl CarrierRequest {
    /// Exact domain-separated immutable operation, excluding HTTP/native authentication.
    pub fn signing_bytes(&self) -> Result<Vec<u8>> {
        canonical(&(
            "swarmie-link-carrier/v1",
            self.account,
            &self.issuer,
            self.device,
            self.generation,
            &self.action,
        ))
    }
}

/// Purpose-limited setup operations, with no generic RPC or credential fields.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CarrierAction {
    /// Publish a bounded target offer. No capability or private code is uploaded.
    Open {
        /// Random private-transfer locator.
        offer: [u8; 16],
        /// Requested setup lifetime in seconds; Cloud assigns the actual expiry.
        lifetime_seconds: u16,
    },
    /// Open one candidate stream; TLS and the private capability must still authenticate it.
    Connect {
        /// Target offer locator.
        offer: [u8; 16],
        /// Immutable candidate identifier, chosen before dispatch.
        connection: Uuid,
        /// Target endpoint pinned by the private offer.
        target: DeviceId,
    },
    /// Extend only the verified target's claimed candidate for its bounded human review.
    Claim {
        /// Exact setup offer locator.
        offer: [u8; 16],
        /// Candidate whose private capability the target has verified.
        connection: Uuid,
        /// Separate claimed-review lifetime, at most ten minutes.
        lifetime_seconds: u16,
    },
    /// Retire a failed or declined candidate while keeping terminal bytes available to its peer.
    Retire {
        /// Exact setup offer locator.
        offer: [u8; 16],
        /// Exact candidate that will accept no more writes.
        connection: Uuid,
    },
    /// Read candidate streams as the actual target.
    Pending {
        /// Exact local target offer.
        offer: [u8; 16],
    },
    /// Append or recognize one exact ciphertext packet. Reusing an index with other bytes fails.
    Send {
        /// Target offer locator.
        offer: [u8; 16],
        /// Exact candidate stream.
        connection: Uuid,
        /// Zero-based sender packet index.
        index: u32,
        /// Opaque end-to-end TLS record bytes, never plaintext setup metadata.
        bytes: Vec<u8>,
    },
    /// Read the peer's exact packet without consuming it or changing storage.
    Read {
        /// Target offer locator.
        offer: [u8; 16],
        /// Exact candidate stream.
        connection: Uuid,
        /// Next zero-based peer packet index.
        index: u32,
    },
}

/// Safe carrier response; no response can prove a linked or unlocked node.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum CarrierReply {
    /// Cloud-assigned discovery expiry; no caller wall clock is trusted.
    Opened {
        /// Unix seconds assigned at the first recognized publication.
        expires_at: u64,
    },
    /// The original exact setup mutation is recognized.
    Accepted,
    /// Candidate IDs only. Their endpoint keys still require the completed TLS handshake.
    Pending {
        /// Bounded pending candidates for this target offer.
        connections: Vec<Uuid>,
    },
    /// Next exact ciphertext chunk, or none while the peer has no additional bytes.
    Packet {
        /// Packet index requested by the recipient.
        index: u32,
        /// Immutable ciphertext bytes, when available.
        bytes: Option<Vec<u8>>,
    },
}
