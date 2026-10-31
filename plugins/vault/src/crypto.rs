use crate::{
    DeviceEnvelope, EncryptedRecord, EnrollmentStatement, Error, RecordHeader, RecordKind, Result,
    SecretValue, SignedEnrollment, VaultIdentity,
};
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use hpke::{
    Deserializable, Kem, OpModeR, OpModeS, Serializable, aead::ChaCha20Poly1305, kdf::HkdfSha256,
    kem::X25519HkdfSha256,
};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

type HpkeKem = X25519HkdfSha256;

pub fn canonical(value: &impl Serialize) -> Result<Vec<u8>> {
    serde_jcs::to_vec(value).map_err(|_| Error::Invalid)
}

pub fn digest(value: &[u8]) -> Vec<u8> {
    Sha256::digest(value).to_vec()
}

pub fn verify_signature(public: &[u8], value: &[u8], signature: &[u8]) -> Result<()> {
    let public: &[u8; 32] = public.try_into().map_err(|_| Error::Integrity)?;
    VerifyingKey::from_bytes(public)
        .map_err(|_| Error::Integrity)?
        .verify_strict(
            value,
            &Signature::from_slice(signature).map_err(|_| Error::Integrity)?,
        )
        .map_err(|_| Error::Integrity)
}

/// Usable vault material is memory-only. Device bundles never contain this key.
pub struct VaultKey(Zeroizing<[u8; 32]>);

impl VaultKey {
    pub fn generate() -> Self {
        let mut key = [0; 32];
        OsRng.fill_bytes(&mut key);
        Self(Zeroizing::new(key))
    }

    fn derive(&self, label: &str, identity: VaultIdentity) -> Result<Zeroizing<[u8; 32]>> {
        let mut output = Zeroizing::new([0; 32]);
        let info = canonical(&(label, identity))?;
        Hkdf::<Sha256>::new(None, self.0.as_ref())
            .expand(&info, output.as_mut())
            .map_err(|_| Error::Integrity)?;
        Ok(output)
    }

    fn derive_record(&self, label: &str, header: &RecordHeader) -> Result<Zeroizing<[u8; 32]>> {
        let mut output = Zeroizing::new([0; 32]);
        let info = canonical(&(label, header.identity, header.secret, header.revision))?;
        Hkdf::<Sha256>::new(None, self.0.as_ref())
            .expand(&info, output.as_mut())
            .map_err(|_| Error::Integrity)?;
        Ok(output)
    }

    pub fn commitment(&self, identity: VaultIdentity) -> Result<Vec<u8>> {
        Ok(self.derive("vault-key-check/v1", identity)?.to_vec())
    }

    pub fn seal(
        &self,
        header: RecordHeader,
        value: Option<&SecretValue>,
    ) -> Result<EncryptedRecord> {
        header.metadata.validate()?;
        if header.version != 1
            || header.successor == 0
            || (header.successor == 1) != header.previous.is_none()
            || (header.kind == RecordKind::Value) != value.is_some()
        {
            return Err(Error::Invalid);
        }
        self.sealer(header)?.seal(value)
    }

    pub(crate) fn sealer(&self, header: RecordHeader) -> Result<RecordSealer> {
        Ok(RecordSealer {
            encryption: self.derive_record("vault-record/v1", &header)?,
            metadata: self.derive_record("vault-metadata/v1", &header)?,
            header,
        })
    }

    pub fn verify(&self, record: &EncryptedRecord) -> Result<()> {
        record
            .header
            .metadata
            .validate()
            .map_err(|_| Error::Integrity)?;
        if record.header.version != 1
            || record.header.successor == 0
            || record.nonce.len() != 24
            || record.ciphertext.len() > 65_552
            || record.authentication.len() != 32
        {
            return Err(Error::Integrity);
        }
        let key = self.derive_record("vault-metadata/v1", &record.header)?;
        let mut mac =
            <Hmac<Sha256> as Mac>::new_from_slice(key.as_ref()).map_err(|_| Error::Integrity)?;
        mac.update(&canonical(&(
            &record.header,
            &record.nonce,
            digest(&record.ciphertext),
        ))?);
        mac.verify_slice(&record.authentication)
            .map_err(|_| Error::Integrity)
    }

    pub fn open(&self, record: &EncryptedRecord) -> Result<SecretValue> {
        self.verify(record)?;
        if record.header.kind != RecordKind::Value {
            return Err(Error::NotFound);
        }
        let key = self.derive_record("vault-record/v1", &record.header)?;
        let data = XChaCha20Poly1305::new_from_slice(key.as_ref())
            .map_err(|_| Error::Integrity)?
            .decrypt(
                &XNonce::from(
                    <[u8; 24]>::try_from(record.nonce.as_slice()).map_err(|_| Error::Integrity)?,
                ),
                Payload {
                    msg: &record.ciphertext,
                    aad: &canonical(&record.header)?,
                },
            )
            .map_err(|_| Error::Integrity)?;
        SecretValue::new(data)
    }
}

pub(crate) struct RecordSealer {
    encryption: Zeroizing<[u8; 32]>,
    metadata: Zeroizing<[u8; 32]>,
    header: RecordHeader,
}

impl RecordSealer {
    pub(crate) fn seal(self, value: Option<&SecretValue>) -> Result<EncryptedRecord> {
        if (self.header.kind == RecordKind::Value) != value.is_some() {
            return Err(Error::Invalid);
        }
        let aad = canonical(&self.header)?;
        let mut nonce = vec![0; 24];
        OsRng.fill_bytes(&mut nonce);
        let ciphertext = XChaCha20Poly1305::new_from_slice(self.encryption.as_ref())
            .map_err(|_| Error::Integrity)?
            .encrypt(
                &XNonce::from(
                    <[u8; 24]>::try_from(nonce.as_slice()).map_err(|_| Error::Integrity)?,
                ),
                Payload {
                    msg: value.map_or(&[], SecretValue::expose),
                    aad: &aad,
                },
            )
            .map_err(|_| Error::Integrity)?;
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(self.metadata.as_ref())
            .map_err(|_| Error::Integrity)?;
        mac.update(&canonical(&(&self.header, &nonce, digest(&ciphertext)))?);
        Ok(EncryptedRecord {
            header: self.header,
            nonce,
            ciphertext,
            authentication: mac.finalize().into_bytes().to_vec(),
        })
    }
}

/// Serialized only into the OS keyring, atomically with its independent trust pins.
#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct DeviceKeys {
    signing: [u8; 32],
    encryption: [u8; 32],
}

impl DeviceKeys {
    pub fn generate() -> Self {
        let signing = SigningKey::generate(&mut OsRng).to_bytes();
        let (secret, _) = HpkeKem::gen_keypair(&mut OsRng);
        Self {
            signing,
            encryption: secret.to_bytes().into(),
        }
    }

    #[cfg(feature = "cloud-client")]
    pub fn transport_identity(&self) -> swarmie_permissions::transport::TransportIdentity {
        swarmie_permissions::transport::TransportIdentity::from_seed(Zeroizing::new(self.signing))
    }

    pub fn signing_public(&self) -> Vec<u8> {
        SigningKey::from_bytes(&self.signing)
            .verifying_key()
            .to_bytes()
            .to_vec()
    }

    pub fn encryption_public(&self) -> Result<Vec<u8>> {
        let sk = <HpkeKem as Kem>::PrivateKey::from_bytes(&self.encryption)
            .map_err(|_| Error::Integrity)?;
        Ok(HpkeKem::sk_to_pk(&sk).to_bytes().to_vec())
    }

    pub fn sign(&self, bytes: &[u8]) -> Vec<u8> {
        SigningKey::from_bytes(&self.signing)
            .sign(bytes)
            .to_bytes()
            .to_vec()
    }

    pub fn enroll(&self, statement: EnrollmentStatement) -> Result<SignedEnrollment> {
        statement.validate()?;
        if statement.approver != self.signing_public() {
            return Err(Error::Forbidden);
        }
        let signature = self.sign(&canonical(&statement)?);
        Ok(SignedEnrollment {
            statement,
            signature,
        })
    }

    pub fn envelope(
        key: &VaultKey,
        recipient: &[u8],
        identity: VaultIdentity,
        device: uuid::Uuid,
        signing: &[u8],
    ) -> Result<DeviceEnvelope> {
        let pk =
            <HpkeKem as Kem>::PublicKey::from_bytes(recipient).map_err(|_| Error::Integrity)?;
        let info = canonical(&(
            "swarmie/vault-envelope/v1",
            identity,
            device,
            signing,
            recipient,
        ))?;
        let (enc, mut sender) = hpke::setup_sender::<ChaCha20Poly1305, HkdfSha256, HpkeKem, _>(
            &OpModeS::Base,
            &pk,
            &info,
            &mut OsRng,
        )
        .map_err(|_| Error::Integrity)?;
        let ciphertext = sender
            .seal(key.0.as_ref(), &info)
            .map_err(|_| Error::Integrity)?;
        Ok(DeviceEnvelope {
            encapsulation: enc.to_bytes().to_vec(),
            ciphertext,
        })
    }

    /// Must be called with independently pinned material, never a new Cloud-supplied root.
    pub fn unlock(
        &self,
        pins: &TrustPins,
        enrollment: &SignedEnrollment,
        envelope: &DeviceEnvelope,
    ) -> Result<VaultKey> {
        let statement = &enrollment.statement;
        statement.validate()?;
        if statement.identity != pins.identity
            || digest(&canonical(enrollment)?) != pins.statement_digest
            || statement.approver != pins.approver
            || statement.signing_key != self.signing_public()
            || statement.encryption_key.as_ref() != Some(&self.encryption_public()?)
            || digest(&canonical(envelope)?) != pins.envelope_digest
            || statement.envelope_digest.as_ref() != Some(&pins.envelope_digest)
            || statement.commitment != pins.commitment
        {
            return Err(Error::Integrity);
        }
        verify_enrollment_chain(pins, enrollment)?;
        verify_signature(
            &pins.approver,
            &canonical(statement)?,
            &enrollment.signature,
        )?;
        let sk = <HpkeKem as Kem>::PrivateKey::from_bytes(&self.encryption)
            .map_err(|_| Error::Integrity)?;
        let enc = <HpkeKem as Kem>::EncappedKey::from_bytes(&envelope.encapsulation)
            .map_err(|_| Error::Integrity)?;
        let info = canonical(&(
            "swarmie/vault-envelope/v1",
            pins.identity,
            statement.recipient,
            &statement.signing_key,
            statement.encryption_key.as_ref().ok_or(Error::Integrity)?,
        ))?;
        let mut receiver = hpke::setup_receiver::<ChaCha20Poly1305, HkdfSha256, HpkeKem>(
            &OpModeR::Base,
            &sk,
            &enc,
            &info,
        )
        .map_err(|_| Error::Integrity)?;
        let mut bytes = Zeroizing::new(
            receiver
                .open(&envelope.ciphertext, &info)
                .map_err(|_| Error::Integrity)?,
        );
        let array: [u8; 32] = bytes.as_slice().try_into().map_err(|_| Error::Integrity)?;
        let key = VaultKey(Zeroizing::new(array));
        bytes.zeroize();
        if !bool::from(key.commitment(pins.identity)?.ct_eq(&pins.commitment)) {
            return Err(Error::Integrity);
        }
        Ok(key)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustPins {
    pub identity: VaultIdentity,
    pub root: Vec<u8>,
    /// Immutable founding-to-approver certificates, independently pinned during enrollment.
    pub chain: Vec<SignedEnrollment>,
    pub approver: Vec<u8>,
    pub envelope_digest: Vec<u8>,
    pub statement_digest: Vec<u8>,
    pub commitment: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustBundle {
    pub keys: DeviceKeys,
    pub pins: TrustPins,
}

pub(crate) fn verify_enrollment_chain(pins: &TrustPins, own: &SignedEnrollment) -> Result<()> {
    use crate::Role;
    use std::collections::BTreeSet;
    if pins.chain.len() >= 8 {
        return Err(Error::Integrity);
    }
    let mut chain: Vec<_> = pins.chain.iter().collect();
    chain.push(own);
    let mut keys = BTreeSet::new();
    let mut devices = BTreeSet::new();
    let mut prior: Option<&EnrollmentStatement> = None;
    for certificate in chain {
        let current = &certificate.statement;
        current.validate()?;
        if current.identity != pins.identity
            || current.commitment != pins.commitment
            || !keys.insert(current.signing_key.clone())
            || !devices.insert(current.recipient)
        {
            return Err(Error::Integrity);
        }
        if let Some(parent) = prior {
            if current.approver != parent.signing_key
                || !parent.roles.contains(&Role::EnrollDevices)
                || !current.roles.is_subset(&parent.roles)
            {
                return Err(Error::Integrity);
            }
        } else if current.signing_key != pins.root
            || current.approver != pins.root
            || current.roles != BTreeSet::from([Role::Decrypt, Role::Manage, Role::EnrollDevices])
        {
            return Err(Error::Integrity);
        }
        verify_signature(
            &current.approver,
            &canonical(current)?,
            &certificate.signature,
        )?;
        prior = Some(current);
    }
    Ok(())
}
