use crate::device_linking::{Error, Result};
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// SHA-256 of the canonical 32-byte endpoint public key. Installation labels grant no authority.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct DeviceId([u8; 32]);

impl DeviceId {
    /// Derive the stable endpoint identity from its canonical public key.
    pub fn of(public: &[u8; 32]) -> Self {
        Self(Sha256::digest(public).into())
    }
}

impl From<DeviceId> for String {
    fn from(value: DeviceId) -> Self {
        value.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

impl TryFrom<String> for DeviceId {
    type Error = Error;

    fn try_from(value: String) -> Result<Self> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(Error::Invalid);
        }
        let mut bytes = [0; 32];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
                .map_err(|_| Error::Invalid)?;
        }
        Ok(Self(bytes))
    }
}

/// Encode public protocol values with the shared canonical JSON encoding.
pub fn canonical(value: &impl Serialize) -> Result<Vec<u8>> {
    serde_jcs::to_vec(value).map_err(|_| Error::Invalid)
}

/// Hash the exact canonical value used for immutable operation recognition.
pub fn digest(value: &impl Serialize) -> Result<[u8; 32]> {
    Ok(Sha256::digest(canonical(value)?).into())
}

/// Verify a strict Ed25519 signature for the given public key and purpose bytes.
pub fn verify(public: &[u8; 32], message: &[u8], signature: &[u8]) -> Result<()> {
    VerifyingKey::from_bytes(public)
        .map_err(|_| Error::Forbidden)?
        .verify_strict(
            message,
            &Signature::from_slice(signature).map_err(|_| Error::Forbidden)?,
        )
        .map_err(|_| Error::Forbidden)
}

/// Check the bounded account display label without treating it as authority.
pub fn valid_label(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}

/// Check a bounded opaque execution-node identifier.
pub fn valid_node(value: &str) -> bool {
    !value.is_empty() && value.len() <= 255 && !value.chars().any(char::is_control)
}

/// Fresh server challenge scoped to the authenticated native account family.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistrationChallenge {
    /// Stable identity assigned to this registration challenge or admitted grant.
    pub id: Uuid,
    /// Exact account authenticated by the native session owner.
    pub account: Uuid,
    /// Current native credential family, never an installation label.
    pub family: Uuid,
    /// Configured public API issuer for this challenge.
    pub issuer: String,
    /// Fresh purpose-bound nonce; reuse cannot create another mutation.
    pub nonce: [u8; 32],
    /// Setup challenge expiry in Unix seconds; it is not a vault lock timer.
    pub expires_at: u64,
}

/// Signed immutable answer to one registration challenge.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistrationProof {
    /// Exact fresh server challenge retained for proof verification.
    pub challenge: RegistrationChallenge,
    /// Canonical 32-byte Ed25519 endpoint signing public key.
    pub public_key: [u8; 32],
    /// Account-managed display label; it grants no cryptographic authority.
    pub name: String,
    /// Exact stable execution-node identity, when this endpoint hosts a node.
    pub node: Option<String>,
    /// Previous accepted generation required for compare-and-exchange rebind.
    pub expected_generation: Option<u64>,
    /// Ed25519 signature over the domain-separated canonical intent.
    pub signature: Vec<u8>,
}

impl RegistrationProof {
    /// Return the exact domain-separated bytes covered by this proof.
    pub fn signing_bytes(&self) -> Result<Vec<u8>> {
        if !valid_label(&self.name) || self.node.as_ref().is_some_and(|node| !valid_node(node)) {
            return Err(Error::Invalid);
        }
        canonical(&(
            "swarmie-device-registration/v1",
            &self.challenge,
            self.public_key,
            &self.name,
            &self.node,
            self.expected_generation,
        ))
    }

    /// Verify a strict Ed25519 signature for the given public key and purpose bytes.
    pub fn verify(&self) -> Result<()> {
        verify(&self.public_key, &self.signing_bytes()?, &self.signature)
    }
}

/// The sole current native-family association for an endpoint, shared by all its vault roots.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceRegistration {
    /// Exact account authenticated by the native session owner.
    pub account: Uuid,
    /// Digest of this endpoint public key.
    pub device: DeviceId,
    /// Canonical 32-byte Ed25519 endpoint signing public key.
    pub public_key: [u8; 32],
    /// Current native credential family, never an installation label.
    pub family: Uuid,
    /// Current accepted authority generation.
    pub generation: u64,
    /// Account-managed display label; it grants no cryptographic authority.
    pub name: String,
    /// Stable node identities admitted under this endpoint key.
    pub nodes: std::collections::BTreeSet<String>,
}

/// Exact vault/root binding digest included by the vault owner. No secret value is present.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultLink {
    /// Exact vault identity, or the bound optional vault intent.
    pub vault: Uuid,
    /// Vault data-key epoch selected by the vault owner.
    pub epoch: Uuid,
    /// Independently authenticated root signing key.
    pub root: [u8; 32],
    /// Exact recipient encryption public key approved by the linking intent.
    pub recipient_key: [u8; 32],
    /// Digest of the root-qualified endpoint capability binding.
    pub binding: [u8; 32],
}

/// Stable intent reviewed once. Renewed native session proofs do not change its mutation identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkIntent {
    /// Immutable operation identity used to recognize its original result.
    pub operation: Uuid,
    /// Fresh purpose-bound nonce; reuse cannot create another mutation.
    pub nonce: Uuid,
    /// Exact account authenticated by the native session owner.
    pub account: Uuid,
    /// Authenticated endpoint receiving directional control authority.
    pub controller: DeviceId,
    /// Controller registration generation checked at admission.
    pub controller_generation: u64,
    /// Exact endpoint hosting the linked execution node.
    pub target: DeviceId,
    /// Target registration generation checked at admission.
    pub target_generation: u64,
    /// Exact stable execution-node identity, when this endpoint hosts a node.
    pub node: String,
    /// Private-transfer rendezvous identity; contains no pairing capability.
    pub offer: [u8; 16],
    /// Absent only when the embedding does not install a vault capability.
    pub vault: Option<VaultLink>,
}

impl LinkIntent {
    /// Return the exact domain-separated bytes covered by this proof.
    pub fn signing_bytes(&self) -> Result<Vec<u8>> {
        if self.operation.is_nil()
            || self.nonce.is_nil()
            || self.account.is_nil()
            || self.controller_generation == 0
            || self.target_generation == 0
            || self.offer == [0; 16]
            || self.vault.as_ref().is_some_and(|vault| {
                vault.vault.is_nil()
                    || vault.epoch.is_nil()
                    || vault.root == [0; 32]
                    || vault.recipient_key == [0; 32]
                    || vault.binding == [0; 32]
            })
            || self.controller == self.target
            || !valid_node(&self.node)
        {
            return Err(Error::Invalid);
        }
        canonical(&("swarmie-device-link/v1", self))
    }
}

/// Endpoint signatures over one immutable link intent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedLink {
    /// Exact stable both-peer linking intent.
    pub intent: LinkIntent,
    /// Controller signature over the exact canonical linking intent.
    pub controller_signature: Vec<u8>,
    /// Target signature over the same canonical linking intent.
    pub target_signature: Vec<u8>,
}

impl SignedLink {
    /// Verify a strict Ed25519 signature for the given public key and purpose bytes.
    pub fn verify(
        &self,
        controller: &DeviceRegistration,
        target: &DeviceRegistration,
    ) -> Result<()> {
        let value = &self.intent;
        if controller.device != value.controller
            || target.device != value.target
            || DeviceId::of(&controller.public_key) != controller.device
            || DeviceId::of(&target.public_key) != target.device
            || controller.account != value.account
            || target.account != value.account
            || controller.generation != value.controller_generation
            || target.generation != value.target_generation
            || !target.nodes.contains(&value.node)
        {
            return Err(Error::Forbidden);
        }
        let bytes = value.signing_bytes()?;
        verify(&controller.public_key, &bytes, &self.controller_signature)?;
        verify(&target.public_key, &bytes, &self.target_signature)
    }
}

/// Recognized acceptance or cancellation of the original link operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LinkOutcome {
    /// The original operation admitted or retired this exact grant generation.
    Accepted {
        /// Exact admitted directional link.
        link: Uuid,
        /// Current recognized grant generation.
        generation: u64,
    },
    /// The exact pending intent was prevented before control admission.
    Cancelled,
}

/// Recognizable immutable result of one link operation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkReceipt {
    /// Immutable operation identity used to recognize its original result.
    pub operation: Uuid,
    /// Digest of the original immutable operation intent.
    pub intent_digest: [u8; 32],
    /// Recognized operation result; never inferred from an outgoing request.
    pub outcome: LinkOutcome,
}

/// Admitted directional account link and revocation generation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkGrant {
    /// Stable identity assigned to this registration challenge or admitted grant.
    pub id: Uuid,
    /// Current accepted authority generation.
    pub generation: u64,
    /// Exact stable both-peer linking intent.
    pub intent: LinkIntent,
    /// Whether this exact directional grant has been retired.
    pub revoked: bool,
}

/// Participant-signed cancellation of one exact pending intent.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkFence {
    /// Exact stable both-peer linking intent.
    pub intent: LinkIntent,
    /// Endpoint authorized to fence or revoke its exact link.
    pub participant: DeviceId,
    /// Ed25519 signature over the domain-separated canonical intent.
    pub signature: Vec<u8>,
}

impl LinkFence {
    /// Return the exact domain-separated bytes covered by this proof.
    pub fn signing_bytes(&self) -> Result<Vec<u8>> {
        self.intent.signing_bytes()?;
        if self.participant != self.intent.controller && self.participant != self.intent.target {
            return Err(Error::Forbidden);
        }
        canonical(&(
            "swarmie-device-link-fence/v1",
            &self.intent,
            self.participant,
        ))
    }
}

/// Participant-signed revocation of an admitted link.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkRevocation {
    /// Exact native account that owns the admitted link.
    pub account: Uuid,
    /// Configured issuer whose directory admitted the endpoint keys.
    pub issuer: String,
    /// Immutable operation identity used to recognize its original result.
    pub operation: Uuid,
    /// Exact admitted directional control grant.
    pub link: Uuid,
    /// Previous accepted generation required for compare-and-exchange rebind.
    pub expected_generation: u64,
    /// Endpoint authorized to fence or revoke its exact link.
    pub participant: DeviceId,
    /// Ed25519 signature over the domain-separated canonical intent.
    pub signature: Vec<u8>,
}

impl LinkRevocation {
    /// Canonical unsigned request digest; signature encoding is not mutation identity.
    pub fn intent_digest(&self) -> Result<[u8; 32]> {
        Ok(Sha256::digest(self.signing_bytes()?).into())
    }

    /// Return the exact domain-separated bytes covered by this proof.
    pub fn signing_bytes(&self) -> Result<Vec<u8>> {
        canonical(&(
            "swarmie-device-link-revoke/v1",
            self.account,
            &self.issuer,
            self.operation,
            self.link,
            self.expected_generation,
            self.participant,
        ))
    }
}
