//! Pure immutable-generation key planning. This module neither accesses custody
//! nor supplies profile ownership, initialization or physical settlement proof.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Serialize;
use swarmie_providers::accounts::recovery::{CredentialReference, CredentialSlot, ProviderOwner};

const DOMAIN: &[u8] = b"swarmie.provider-generation.v2\0";
const USERNAME_PREFIX: &str = "swarmie.immutable.v2:";
const PARTS: usize = 64;
const DIGEST_TEMPLATE: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn invalid(message: &str) -> tembra::Error {
    tembra::Error::invalid("providers.credential_reference", message)
}

/// Injected owner/namespace and explicit UTF-8 storage bounds. Construction and
/// planning perform no I/O. Bounds must come from a separately qualified native
/// backend; accepting a plan does not establish custody availability or authority.
pub struct NativeCredentialKeyspace {
    owner: ProviderOwner,
    namespace: String,
    maximum_reference_bytes: usize,
    maximum_username_bytes: usize,
    maximum_descriptor_bytes: usize,
}

impl NativeCredentialKeyspace {
    pub fn new(
        owner: ProviderOwner,
        namespace: String,
        maximum_reference_bytes: usize,
        maximum_username_bytes: usize,
        maximum_descriptor_bytes: usize,
    ) -> tembra::Result<Self> {
        if namespace.is_empty()
            || u32::try_from(namespace.len()).is_err()
            || namespace.len() > maximum_username_bytes
            || maximum_reference_bytes == 0
            || maximum_username_bytes == 0
            || maximum_descriptor_bytes == 0
        {
            return Err(invalid("invalid native credential namespace or bounds"));
        }
        Ok(Self {
            owner,
            namespace,
            maximum_reference_bytes,
            maximum_username_bytes,
            maximum_descriptor_bytes,
        })
    }

    /// Check the exact reference before remote exchange or renewal. The result
    /// is a mechanical layout, not a reservation, sealed value or access grant.
    pub fn plan(&self, reference: &CredentialReference) -> tembra::Result<NativeCredentialKeys> {
        if reference.owner() != &self.owner {
            return Err(invalid("the credential reference belongs to another owner"));
        }
        if self.owner.principal().len() > self.maximum_reference_bytes
            || matches!(reference.slot(), CredentialSlot::Account { account_id } if account_id.len() > self.maximum_reference_bytes)
        {
            return Err(invalid(
                "the credential reference exceeds the configured bound",
            ));
        }
        reference.check_size(self.maximum_reference_bytes)?;
        let mut tuple = DOMAIN.to_vec();
        append_string(&mut tuple, &self.namespace)?;
        append_string(&mut tuple, self.owner.principal())?;
        tuple.extend_from_slice(self.owner.profile().as_bytes());
        match reference.slot() {
            CredentialSlot::Account { account_id } => {
                tuple.push(0);
                append_string(&mut tuple, account_id)?;
            }
            CredentialSlot::Operation { operation } => {
                tuple.push(1);
                tuple.extend_from_slice(operation.as_bytes());
            }
        }
        tuple.extend_from_slice(reference.generation().as_bytes());
        let username = format!("{USERNAME_PREFIX}{}", URL_SAFE_NO_PAD.encode(tuple));
        // /descriptor is longer than every /part/NN suffix. The inventory is
        // fixed even when a sealed descriptor is absent or invalid.
        if username
            .len()
            .checked_add("/descriptor".len())
            .is_none_or(|length| length > self.maximum_username_bytes)
        {
            return Err(invalid(
                "native credential keys exceed the configured bound",
            ));
        }
        let legacy_prefix = format!("{}/", self.namespace);
        if username.starts_with(&legacy_prefix)
            || format!("{username}/descriptor").starts_with(&legacy_prefix)
            || (0..PARTS)
                .any(|index| format!("{username}/part/{index:02}").starts_with(&legacy_prefix))
        {
            return Err(invalid(
                "native credential keys overlap the legacy namespace",
            ));
        }
        let maximum_descriptor_bytes = serde_json::to_vec(&Descriptor {
            version: 2,
            reference,
            count: PARTS,
            digest: DIGEST_TEMPLATE,
        })
        .map_err(|_| invalid("the protected descriptor could not be encoded"))?
        .len();
        if maximum_descriptor_bytes > self.maximum_descriptor_bytes {
            return Err(invalid(
                "the protected descriptor exceeds the configured bound",
            ));
        }
        Ok(NativeCredentialKeys {
            reference: reference.clone(),
            username,
            maximum_descriptor_bytes,
        })
    }
}

/// Exact native usernames for one immutable generation, with no secret, Debug
/// or RPC representation. The service is the existing provider keyring service;
/// usernames already contain their new domain and must not be legacy-prefixed.
pub struct NativeCredentialKeys {
    reference: CredentialReference,
    username: String,
    maximum_descriptor_bytes: usize,
}

impl NativeCredentialKeys {
    pub fn service(&self) -> &'static str {
        "swarmie.providers.v1"
    }

    pub fn reference(&self) -> &CredentialReference {
        &self.reference
    }

    pub fn descriptor_key(&self) -> String {
        format!("{}/descriptor", self.username)
    }

    pub fn part_key(&self, index: usize) -> tembra::Result<String> {
        if index >= PARTS {
            return Err(invalid("invalid protected credential part index"));
        }
        Ok(format!("{}/part/{index:02}", self.username))
    }

    /// Encoded bytes for the version-2 descriptor with 64 parts and a 64-character
    /// SHA-256 digest. Actual sealing must use this same checked descriptor shape.
    pub fn maximum_descriptor_bytes(&self) -> usize {
        self.maximum_descriptor_bytes
    }
}

#[derive(Serialize)]
struct Descriptor<'a> {
    version: u8,
    reference: &'a CredentialReference,
    count: usize,
    digest: &'a str,
}

fn append_string(bytes: &mut Vec<u8>, value: &str) -> tembra::Result<()> {
    let length = u32::try_from(value.len())
        .map_err(|_| invalid("a native credential key component exceeds its encoding bound"))?;
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(value.as_bytes());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use uuid::Uuid;

    fn owner() -> ProviderOwner {
        ProviderOwner::new("principal".into(), Uuid::from_u128(1)).unwrap()
    }

    fn reference(id: &str) -> CredentialReference {
        CredentialReference::new(
            owner(),
            CredentialSlot::Account {
                account_id: id.into(),
            },
            Uuid::from_u128(2),
        )
        .unwrap()
    }

    fn keyspace() -> NativeCredentialKeyspace {
        NativeCredentialKeyspace::new(owner(), "fixture/principal".into(), 4096, 4096, 1024)
            .unwrap()
    }

    #[test]
    fn canonical_tuple_has_a_fixed_fixture_and_bounded_exact_inventory() {
        let reference = reference("a/b");
        let keys = keyspace().plan(&reference).unwrap();
        let base = "swarmie.immutable.v2:c3dhcm1pZS5wcm92aWRlci1nZW5lcmF0aW9uLnYyAAAAABFmaXh0dXJlL3ByaW5jaXBhbAAAAAlwcmluY2lwYWwAAAAAAAAAAAAAAAAAAAABAAAAAANhL2IAAAAAAAAAAAAAAAAAAAAC";
        assert_eq!(keys.service(), "swarmie.providers.v1");
        assert!(keys.reference() == &reference);
        assert_eq!(keys.descriptor_key(), format!("{base}/descriptor"));
        assert_eq!(keys.part_key(0).unwrap(), format!("{base}/part/00"));
        assert_eq!(keys.part_key(63).unwrap(), format!("{base}/part/63"));
        assert!(keys.part_key(64).is_err());
        let mut inventory = BTreeSet::from([keys.descriptor_key()]);
        for index in 0..64 {
            assert!(inventory.insert(keys.part_key(index).unwrap()));
        }
        assert_eq!(inventory.len(), 65);
        assert!(
            inventory
                .iter()
                .all(|key| !key.starts_with("fixture/principal/"))
        );
    }

    #[test]
    fn slots_raw_identifiers_namespaces_and_generations_remain_distinct() {
        let space = keyspace();
        let mut keys = BTreeSet::new();
        for id in ["a/b", "a", "b", "a\0b", "é", "e\u{301}"] {
            assert!(keys.insert(space.plan(&reference(id)).unwrap().descriptor_key()));
        }
        let operation = CredentialReference::new(
            owner(),
            CredentialSlot::Operation {
                operation: Uuid::from_u128(3),
            },
            Uuid::from_u128(2),
        )
        .unwrap();
        assert!(keys.insert(space.plan(&operation).unwrap().descriptor_key()));
        let later =
            CredentialReference::new(owner(), reference("a/b").slot().clone(), Uuid::from_u128(3))
                .unwrap();
        assert!(keys.insert(space.plan(&later).unwrap().descriptor_key()));
        let other =
            NativeCredentialKeyspace::new(owner(), "fixture/principal/".into(), 4096, 4096, 1024)
                .unwrap();
        assert!(keys.insert(other.plan(&reference("a/b")).unwrap().descriptor_key()));
    }

    #[test]
    fn owner_and_each_native_capacity_are_checked_before_any_effect() {
        let reference = reference("a/b");
        let keys = keyspace().plan(&reference).unwrap();
        let key_bytes = keys.descriptor_key().len();
        let descriptor_bytes = keys.maximum_descriptor_bytes();
        let reference_bytes = serde_json::to_vec(&reference).unwrap().len();
        let exact = NativeCredentialKeyspace::new(
            owner(),
            "fixture/principal".into(),
            reference_bytes,
            key_bytes,
            descriptor_bytes,
        )
        .unwrap();
        assert!(exact.plan(&reference).is_ok());
        for (refs, names, descriptor) in [
            (reference_bytes - 1, key_bytes, descriptor_bytes),
            (reference_bytes, key_bytes - 1, descriptor_bytes),
            (reference_bytes, key_bytes, descriptor_bytes - 1),
        ] {
            let space = NativeCredentialKeyspace::new(
                owner(),
                "fixture/principal".into(),
                refs,
                names,
                descriptor,
            )
            .unwrap();
            assert!(space.plan(&reference).is_err());
        }
        let foreign = CredentialReference::new(
            ProviderOwner::new("principal".into(), Uuid::from_u128(99)).unwrap(),
            reference.slot().clone(),
            reference.generation(),
        )
        .unwrap();
        assert!(exact.plan(&foreign).is_err());
        let foreign = CredentialReference::new(
            ProviderOwner::new("other".into(), owner().profile()).unwrap(),
            reference.slot().clone(),
            reference.generation(),
        )
        .unwrap();
        assert!(exact.plan(&foreign).is_err());
    }
}
