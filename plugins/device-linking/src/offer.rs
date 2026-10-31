use crate::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};
use swarmie_permissions::transport::{BootstrapBinding, TransportIdentity};
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

/// Protected-storage payload. No diagnostic formatter or model-facing getter exposes the seed.
#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
pub struct EndpointKeys {
    seed: [u8; 32],
}

impl EndpointKeys {
    pub fn generate() -> Self {
        Self {
            seed: SigningKey::generate(&mut OsRng).to_bytes(),
        }
    }

    pub fn public(&self) -> [u8; 32] {
        SigningKey::from_bytes(&self.seed)
            .verifying_key()
            .to_bytes()
    }

    pub fn id(&self) -> DeviceId {
        DeviceId::of(&self.public())
    }

    pub fn transport(&self) -> TransportIdentity {
        TransportIdentity::from_seed(Zeroizing::new(self.seed))
    }

    pub(crate) fn sign(&self, message: &[u8]) -> Vec<u8> {
        SigningKey::from_bytes(&self.seed)
            .sign(message)
            .to_bytes()
            .to_vec()
    }

    /// Sign one typed, domain-separated stable link intent.
    pub fn sign_link(&self, intent: &LinkIntent) -> Result<Vec<u8>> {
        Ok(self.sign(&intent.signing_bytes()?))
    }

    pub fn registration(
        &self,
        challenge: RegistrationChallenge,
        name: String,
        node: Option<String>,
        expected_generation: Option<u64>,
    ) -> Result<RegistrationProof> {
        let mut proof = RegistrationProof {
            challenge,
            public_key: self.public(),
            name,
            node,
            expected_generation,
            signature: vec![],
        };
        proof.signature = self.sign(&proof.signing_bytes()?);
        Ok(proof)
    }
}

/// Private full-code contents. Keep outside State, history, logs and Cloud rendezvous metadata.
#[derive(Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
pub struct LinkOffer {
    version: u8,
    issuer: String,
    rendezvous: [u8; 16],
    capability: [u8; 32],
    target_key: [u8; 32],
}

impl LinkOffer {
    pub fn decode(code: &str, expected_issuer: &str) -> Result<Self> {
        let code = code.strip_prefix("swarmie:link:").ok_or(Error::Invalid)?;
        if code.len() > 4096 {
            return Err(Error::Invalid);
        }
        let bytes = Zeroizing::new(URL_SAFE_NO_PAD.decode(code).map_err(|_| Error::Invalid)?);
        let offer: Self = serde_json::from_slice(&bytes).map_err(|_| Error::Invalid)?;
        if offer.version != 1
            || offer.issuer != expected_issuer
            || offer.capability == [0; 32]
            || offer.rendezvous == [0; 16]
            || VerifyingKey::from_bytes(&offer.target_key).is_err()
        {
            return Err(Error::Forbidden);
        }
        Ok(offer)
    }

    pub fn code(&self) -> Result<Zeroizing<String>> {
        let bytes = Zeroizing::new(canonical(self)?);
        Ok(Zeroizing::new(format!(
            "swarmie:link:{}",
            URL_SAFE_NO_PAD.encode(bytes.as_slice())
        )))
    }

    pub fn rendezvous(&self) -> [u8; 16] {
        self.rendezvous
    }

    pub fn target(&self) -> [u8; 32] {
        self.target_key
    }

    fn public_digest(&self) -> Result<[u8; 32]> {
        digest(&(self.version, &self.issuer, self.rendezvous, self.target_key))
    }

    fn proof_bytes(&self, binding: &BootstrapBinding, claimant: &[u8; 32]) -> Result<Vec<u8>> {
        if !binding.is_live() || binding.offer() != &self.rendezvous {
            return Err(Error::Forbidden);
        }
        canonical(&(
            "swarmie-link-claim/v1",
            binding.exporter(),
            self.public_digest()?,
            self.target_key,
            {
                // Canonical RFC 8410 Ed25519 SubjectPublicKeyInfo, without NULL parameters.
                let mut spki = [0; 44];
                spki[..12].copy_from_slice(&[
                    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
                ]);
                spki[12..].copy_from_slice(claimant);
                <[u8; 32]>::from(Sha256::digest(spki))
            },
        ))
    }

    fn mac(&self) -> Result<Hmac<Sha256>> {
        let mut key = Zeroizing::new([0; 32]);
        Hkdf::<Sha256>::new(None, &self.capability)
            .expand(b"swarmie-link-claim-hmac/v1", key.as_mut())
            .map_err(|_| Error::Invalid)?;
        Hmac::<Sha256>::new_from_slice(key.as_ref()).map_err(|_| Error::Invalid)
    }

    /// Sent only over the target-pinned completed TLS connection.
    pub fn claim_proof(&self, binding: &BootstrapBinding) -> Result<[u8; 32]> {
        if binding.peer() != &self.target_key {
            return Err(Error::Forbidden);
        }
        let mut mac = self.mac()?;
        mac.update(&self.proof_bytes(binding, binding.local())?);
        Ok(mac.finalize().into_bytes().into())
    }
}

/// Target-local, serial one-use claim owner. No invalid proof or foreign account consumes it.
pub struct OfferOwner {
    offer: LinkOffer,
    account: uuid::Uuid,
    node: String,
    deadline: Instant,
    claimed: bool,
}

impl OfferOwner {
    pub fn new(
        keys: &EndpointKeys,
        account: uuid::Uuid,
        node: String,
        issuer: String,
        lifetime: Duration,
    ) -> Result<Self> {
        if account.is_nil()
            || !valid_node(&node)
            || issuer.is_empty()
            || issuer.len() > 2048
            || issuer.chars().any(char::is_control)
            || lifetime.is_zero()
            || lifetime > Duration::from_secs(300)
        {
            return Err(Error::Invalid);
        }
        let mut rendezvous = [0; 16];
        let mut capability = [0; 32];
        OsRng.fill_bytes(&mut rendezvous);
        OsRng.fill_bytes(&mut capability);
        Ok(Self {
            offer: LinkOffer {
                version: 1,
                issuer,
                rendezvous,
                capability,
                target_key: keys.public(),
            },
            account,
            node,
            deadline: Instant::now() + lifetime,
            claimed: false,
        })
    }

    pub fn offer(&self) -> Result<LinkOffer> {
        if self.claimed {
            return Err(Error::Claimed);
        }
        if Instant::now() >= self.deadline {
            return Err(Error::Expired);
        }
        Ok(self.offer.clone())
    }

    /// Atomically consume only a valid capability proven on the actual completed TLS channel.
    /// Account lookup follows this step: a leaked valid code must be visible even if its holder
    /// is not admitted to this account. Incorrect proofs never consume the offer.
    pub fn claim_key(&mut self, binding: &BootstrapBinding, proof: &[u8]) -> Result<ClaimedKey> {
        if Instant::now() >= self.deadline {
            return Err(Error::Expired);
        }
        if binding.local() != &self.offer.target_key {
            return Err(Error::Forbidden);
        }
        let mut mac = self.offer.mac()?;
        mac.update(&self.offer.proof_bytes(binding, binding.peer())?);
        mac.verify_slice(proof).map_err(|_| Error::Forbidden)?;
        if self.claimed {
            return Err(Error::Claimed);
        }
        self.claimed = true;
        Ok(ClaimedKey {
            account: self.account,
            node: self.node.clone(),
            target: DeviceId::of(&self.offer.target_key),
            offer: self.offer.rendezvous,
            binding: binding.clone(),
        })
    }

    /// Convenience for embeddings that already hold independently authenticated directory data.
    /// A valid capability is consumed even when the following account check refuses its holder.
    pub fn claim(
        &mut self,
        binding: &BootstrapBinding,
        proof: &[u8],
        registration: &DeviceRegistration,
    ) -> Result<ClaimedOffer> {
        self.claim_key(binding, proof)?.authorize(registration)
    }
}

/// Native proof of the consumed capability, before account membership is resolved.
pub struct ClaimedKey {
    account: Uuid,
    node: String,
    target: DeviceId,
    offer: [u8; 16],
    binding: BootstrapBinding,
}
impl ClaimedKey {
    /// Actual claimant key digest for independently authenticated directory lookup.
    pub fn device(&self) -> DeviceId {
        DeviceId::of(self.binding.peer())
    }

    /// Bind account membership only after capability verification, never from peer metadata.
    pub fn authorize(self, registration: &DeviceRegistration) -> Result<ClaimedOffer> {
        if !self.binding.is_live()
            || registration.account != self.account
            || registration.public_key != *self.binding.peer()
            || registration.device != self.device()
            || registration.generation == 0
        {
            return Err(Error::Forbidden);
        }
        Ok(ClaimedOffer {
            account: self.account,
            node: self.node,
            target: self.target,
            claimant: registration.clone(),
            offer: self.offer,
            binding: self.binding,
        })
    }
}

/// Nonserializable actual claim. Public IDs or a Cloud assertion cannot construct it.
pub struct ClaimedOffer {
    account: uuid::Uuid,
    node: String,
    target: DeviceId,
    claimant: DeviceRegistration,
    offer: [u8; 16],
    binding: BootstrapBinding,
}

impl ClaimedOffer {
    pub fn claimant(&self) -> &DeviceRegistration {
        &self.claimant
    }

    pub fn check(&self, intent: &LinkIntent) -> Result<()> {
        if !self.binding.is_live()
            || intent.account != self.account
            || intent.target != self.target
            || intent.node != self.node
            || intent.controller != self.claimant.device
            || intent.controller_generation != self.claimant.generation
            || intent.offer != self.offer
        {
            return Err(Error::Forbidden);
        }
        intent.signing_bytes()?;
        Ok(())
    }
}
