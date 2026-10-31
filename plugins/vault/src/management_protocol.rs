use crate::{DeviceEnvelope, Role, SignedEnrollment, VaultIdentity};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TicketIntent {
    /// Immutable account/root/epoch binding.
    pub identity: VaultIdentity,
    /// Target decryption node.
    pub node: Uuid,
    /// Canonical management signing-key fingerprint, independently pinned by the node.
    pub client: Vec<u8>,
    /// Actual current shell process/login generation.
    pub shell: Uuid,
    /// Memory-only ephemeral Ed25519 resume public key.
    pub resume: Vec<u8>,
    /// Domain-separated TLS 1.3 exporter from the qualified mutual channel.
    pub exporter: Vec<u8>,
    /// Fresh challenge supplied by the actual node.
    pub challenge: Uuid,
    /// Fresh client ceremony nonce.
    pub nonce: Uuid,
}

/// One-use server-side ticket, useless without node proof and the same mutual channel.
#[derive(Clone, Serialize, Deserialize)]
pub struct Ticket {
    /// Opaque ticket identity; not a standalone authorization proof.
    pub id: Uuid,
    /// Exact endpoint/session/channel intent accepted from the client.
    pub intent: TicketIntent,
    /// Absolute authentication-ticket expiry; not a vault inactivity timer.
    pub expires: i64,
}

#[derive(Serialize, Deserialize)]
pub struct CloudRelease {
    /// Exact admitted device statement.
    pub certificate: SignedEnrollment,
    /// Device-encrypted inner HPKE envelope, never the server wrapping key.
    pub envelope: DeviceEnvelope,
    /// Exact client/node/channel authorization admitted by the server.
    pub admission: ClientAdmission,
}

/// Safe authorization facts returned only after actual ticket redemption.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientAdmission {
    /// Exact one-use ticket accepted by the server.
    pub ticket: Uuid,
    /// Actual mutual channel and memory-only resume identity.
    pub intent: TicketIntent,
    /// Native account family registered to the approved management key.
    pub family: Uuid,
    /// Hosted client/node authorization generation.
    pub generation: u64,
    /// Authority barrier required before new connected decisions.
    pub sequence: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityCheck {
    /// Immutable queried root.
    pub identity: VaultIdentity,
    /// Full authority high-water sequence.
    pub sequence: u64,
    /// Actual native family making this check.
    pub actor_family: Uuid,
    /// Account's current mutable root; other admitted roots remain readable archives.
    pub current: Option<Uuid>,
    /// Persistent exposure warning for the queried root.
    pub exposed: bool,
    /// Active native families supplied by the authentication owner at request admission.
    pub active_families: BTreeSet<Uuid>,
    /// Enrollment facts with separate session and enrollment denials.
    pub devices: Vec<DeviceAuthority>,
    /// Latest client/node unlock authorizations.
    pub generations: Vec<AuthorizationGeneration>,
}

/// Current admission metadata; never device private keys or inner envelopes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceAuthority {
    /// Routing device identity authenticated by its admitted certificate.
    pub device: Uuid,
    /// Canonical signing-key fingerprint.
    pub fingerprint: Vec<u8>,
    /// Admitted certificate roles.
    pub roles: BTreeSet<Role>,
    /// Current proof-bound native family association.
    pub family: Option<Uuid>,
    /// Enrollment retirement/compromise denial, independent of family expiration.
    pub removed: bool,
}

/// Hosted generation fence for one management identity and execution node.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizationGeneration {
    /// Management signing-key fingerprint.
    pub client: Vec<u8>,
    /// Decryption node.
    pub node: Uuid,
    /// Latest hosted authorization generation.
    pub generation: u64,
}

/// Discovery is not trust: only independently pinned bundles authorize an enrollment.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultCatalog {
    pub current: Option<Uuid>,
    pub roots: Vec<CatalogRoot>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogRoot {
    pub identity: VaultIdentity,
    pub exposed: bool,
    pub certificates: Vec<SignedEnrollment>,
    pub removed: BTreeSet<Uuid>,
}
