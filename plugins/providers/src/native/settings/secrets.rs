use super::failure;
use std::sync::Arc;
use zeroize::Zeroizing;

pub(super) fn missing() -> tembra::Error {
    tembra::Error::new(
        tembra::Code::Unavailable,
        "providers.credential_missing",
        "the account credential is unavailable",
    )
}

pub(super) fn invalid() -> tembra::Error {
    tembra::Error::new(
        tembra::Code::Invalid,
        "providers.credential_invalid",
        "the account credential is invalid",
    )
}
pub(super) fn has_no_restore_point(error: &tembra::Error) -> bool {
    (error.code == tembra::Code::Unavailable && error.reason == "providers.credential_missing")
        || (error.code == tembra::Code::Invalid && error.reason == "providers.credential_invalid")
}

/// The host supplies credential custody. Blocking platform calls use the blocking pool.
pub(super) trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> tembra::Result<Zeroizing<String>>;
    fn set(&self, key: &str, value: &str) -> tembra::Result<()>;
    fn delete(&self, key: &str) -> tembra::Result<()>;
}

pub(super) struct KeyringStore {
    pub namespace: String,
}
impl KeyringStore {
    fn entry(&self, key: &str) -> tembra::Result<keyring::Entry> {
        keyring::Entry::new("swarmie.providers.v1", &format!("{}/{key}", self.namespace))
            .map_err(|_| failure("the system credential store is unavailable"))
    }
}
impl SecretStore for KeyringStore {
    fn get(&self, key: &str) -> tembra::Result<Zeroizing<String>> {
        self.entry(key)?
            .get_password()
            .map(Zeroizing::new)
            .map_err(|error| {
                if matches!(error, keyring::Error::NoEntry) {
                    missing()
                } else if matches!(error, keyring::Error::BadEncoding(_)) {
                    invalid()
                } else {
                    failure("the system credential store is unavailable")
                }
            })
    }
    fn set(&self, key: &str, value: &str) -> tembra::Result<()> {
        self.entry(key)?
            .set_password(value)
            .map_err(|_| failure("the account could not be saved in the system credential store"))
    }
    fn delete(&self, key: &str) -> tembra::Result<()> {
        self.entry(key)?
            .delete_credential()
            .map_err(|_| failure("credential cleanup failed"))
    }
}

pub(super) async fn get(
    store: Arc<dyn SecretStore>,
    key: String,
) -> tembra::Result<Zeroizing<String>> {
    tokio::task::spawn_blocking(move || store.get(&key))
        .await
        .map_err(|_| failure("the credential store operation did not finish"))?
}
pub(super) async fn set(
    store: Arc<dyn SecretStore>,
    key: String,
    value: Zeroizing<String>,
) -> tembra::Result<()> {
    tokio::task::spawn_blocking(move || store.set(&key, &value))
        .await
        .map_err(|_| failure("the credential store operation did not finish"))?
}
pub(super) async fn delete(store: Arc<dyn SecretStore>, key: String) -> tembra::Result<()> {
    tokio::task::spawn_blocking(move || store.delete(&key))
        .await
        .map_err(|_| failure("the credential store operation did not finish"))?
}

const PREFIX: &str = "swarmie.parts.v1:";

/// Each part fits the smallest supported platform blob limit. A manifest selects
/// one complete generation. Credential rotation publishes that manifest last.
pub(super) struct ChunkedStore {
    raw: Arc<dyn SecretStore>,
}
impl ChunkedStore {
    pub fn new(raw: Arc<dyn SecretStore>) -> Self {
        Self { raw }
    }
    fn manifest(value: &str) -> tembra::Result<Option<(&str, usize, &str)>> {
        let Some(value) = value.strip_prefix(PREFIX) else {
            return Ok(None);
        };
        let mut fields = value.split(':');
        let (Some(generation), Some(count), Some(digest), None) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            return Err(invalid());
        };
        let count = count
            .parse::<usize>()
            .ok()
            .filter(|count| *count > 0 && *count <= 64)
            .ok_or_else(invalid)?;
        if uuid::Uuid::parse_str(generation).is_err()
            || digest.len() != 64
            || !digest.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err(invalid());
        }
        Ok(Some((generation, count, digest)))
    }
    fn cleanup(&self, key: &str, manifest: &str) {
        if let Ok(Some((generation, count, _))) = Self::manifest(manifest) {
            for index in 0..count {
                let _ = self.raw.delete(&format!("{key}/{generation}/{index}"));
            }
        }
    }
    fn publish(&self, key: &str, value: &str) -> tembra::Result<()> {
        use sha2::{Digest, Sha256};
        if value.is_empty() || value.len() > 32768 {
            return Err(failure("the credential exceeds the credential-store limit"));
        }
        let previous = match self.raw.get(key) {
            Ok(value) => Some(value),
            Err(error) if has_no_restore_point(&error) => None,
            Err(error) => return Err(error),
        };
        let generation = uuid::Uuid::new_v4().to_string();
        let mut remaining = value;
        let mut count = 0;
        while !remaining.is_empty() {
            let mut end = remaining.len().min(1024);
            while !remaining.is_char_boundary(end) {
                end -= 1;
            }
            if let Err(error) = self
                .raw
                .set(&format!("{key}/{generation}/{count}"), &remaining[..end])
            {
                for index in 0..count {
                    let _ = self.raw.delete(&format!("{key}/{generation}/{index}"));
                }
                return Err(error);
            }
            remaining = &remaining[end..];
            count += 1;
        }
        let manifest = format!(
            "{PREFIX}{generation}:{count}:{:x}",
            Sha256::digest(value.as_bytes())
        );
        if let Err(error) = self.raw.set(key, &manifest) {
            self.cleanup(key, &manifest);
            return Err(error);
        }
        if let Some(previous) = previous {
            self.cleanup(key, &previous);
        }
        Ok(())
    }
}
impl SecretStore for ChunkedStore {
    fn get(&self, key: &str) -> tembra::Result<Zeroizing<String>> {
        use sha2::{Digest, Sha256};
        let manifest = self.raw.get(key)?;
        let (generation, count, digest) = Self::manifest(&manifest)?.ok_or_else(invalid)?;
        let mut secret = Zeroizing::new(String::new());
        for index in 0..count {
            let part = self.raw.get(&format!("{key}/{generation}/{index}"))?;
            if part.len() > 1024 || secret.len() + part.len() > 32768 {
                return Err(invalid());
            }
            secret.push_str(&part);
        }
        if format!("{:x}", Sha256::digest(secret.as_bytes())) != digest {
            return Err(invalid());
        }
        Ok(secret)
    }
    fn set(&self, key: &str, value: &str) -> tembra::Result<()> {
        self.publish(key, value)
    }
    fn delete(&self, key: &str) -> tembra::Result<()> {
        let previous = self.raw.get(key)?;
        self.raw.delete(key)?;
        self.cleanup(key, &previous);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::BTreeMap,
        sync::{
            Mutex,
            atomic::{AtomicBool, Ordering},
        },
    };
    #[derive(Default)]
    struct Raw {
        values: Mutex<BTreeMap<String, Zeroizing<String>>>,
        refuse_manifest: AtomicBool,
        refuse_read: AtomicBool,
    }
    impl SecretStore for Raw {
        fn get(&self, key: &str) -> tembra::Result<Zeroizing<String>> {
            if self.refuse_read.load(Ordering::SeqCst) {
                return Err(failure("fixture read refused"));
            }
            self.values
                .lock()
                .unwrap()
                .get(key)
                .cloned()
                .ok_or_else(missing)
        }
        fn set(&self, key: &str, value: &str) -> tembra::Result<()> {
            assert!(
                value.encode_utf16().count() * 2 <= 2560,
                "each platform blob must fit"
            );
            if key == "account" && self.refuse_manifest.load(Ordering::SeqCst) {
                return Err(failure("fixture publication refused"));
            }
            self.values
                .lock()
                .unwrap()
                .insert(key.into(), Zeroizing::new(value.into()));
            Ok(())
        }
        fn delete(&self, key: &str) -> tembra::Result<()> {
            self.values.lock().unwrap().remove(key);
            Ok(())
        }
    }
    #[test]
    fn credentials_above_platform_blob_limits_rotate_with_one_manifest_commit() {
        let raw = Arc::new(Raw::default());
        let store = ChunkedStore::new(raw.clone());
        let first = Zeroizing::new("αβ🙂".repeat(3000));
        store.set("account", &first).unwrap();
        assert_eq!(store.get("account").unwrap().as_str(), first.as_str());
        let previous_keys: Vec<_> = raw.values.lock().unwrap().keys().cloned().collect();
        raw.refuse_manifest.store(true, Ordering::SeqCst);
        assert!(store.set("account", &"replacement".repeat(2000)).is_err());
        assert_eq!(store.get("account").unwrap().as_str(), first.as_str());
        assert_eq!(
            raw.values
                .lock()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            previous_keys,
            "failed publication must remove new parts and preserve the old generation"
        );
        raw.refuse_manifest.store(false, Ordering::SeqCst);
        store.set("account", "new credential").unwrap();
        assert_eq!(store.get("account").unwrap().as_str(), "new credential");
        assert_eq!(
            raw.values.lock().unwrap().len(),
            2,
            "rotation removes old parts"
        );
        store.delete("account").unwrap();
        assert!(raw.values.lock().unwrap().is_empty());
    }
    #[test]
    fn incomplete_or_modified_credential_parts_are_never_returned() {
        let raw = Arc::new(Raw::default());
        let store = ChunkedStore::new(raw.clone());
        store.set("account", &"a".repeat(4000)).unwrap();
        let key = raw
            .values
            .lock()
            .unwrap()
            .keys()
            .find(|key| key.as_str() != "account")
            .unwrap()
            .clone();
        raw.values
            .lock()
            .unwrap()
            .insert(key, Zeroizing::new("modified".into()));
        assert!(store.get("account").is_err());
    }
    #[test]
    fn store_access_errors_stop_publication_before_any_part_is_written() {
        let raw = Arc::new(Raw::default());
        let store = ChunkedStore::new(raw.clone());
        store.set("account", "previous synthetic value").unwrap();
        let previous = raw.values.lock().unwrap()["account"].clone();
        let keys = raw
            .values
            .lock()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        raw.refuse_read.store(true, Ordering::SeqCst);
        let error = store.set("account", "new synthetic value").unwrap_err();
        assert!(
            !has_no_restore_point(&error),
            "store access errors are not corruption"
        );
        assert!(raw.values.lock().unwrap()["account"].as_str() == previous.as_str());
        assert_eq!(
            raw.values
                .lock()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            keys
        );
        raw.refuse_read.store(false, Ordering::SeqCst);
        store.set("account", "new synthetic value").unwrap();
        assert_eq!(
            store.get("account").unwrap().as_str(),
            "new synthetic value"
        );
    }
}

#[cfg(test)]
mod native_qualification {
    use super::*;

    #[test]
    #[ignore = "requires an unlocked native credential store"]
    fn native_credential_store_round_trip_uses_only_owned_synthetic_entries() {
        let store = ChunkedStore::new(Arc::new(KeyringStore {
            namespace: format!("native-fixture/{}", uuid::Uuid::new_v4()),
        }));
        struct Cleanup<'a>(&'a ChunkedStore);
        impl Drop for Cleanup<'_> {
            fn drop(&mut self) {
                let _ = self.0.delete("fixture");
            }
        }
        let cleanup = Cleanup(&store);
        let value = Zeroizing::new("αβ🙂".repeat(3000));
        store
            .set("fixture", &value)
            .expect("synthetic credential write");
        assert_eq!(
            store
                .get("fixture")
                .expect("synthetic credential read")
                .as_str(),
            value.as_str()
        );
        store
            .set("fixture", "rotated synthetic value")
            .expect("synthetic credential rotation");
        assert_eq!(
            store
                .get("fixture")
                .expect("synthetic rotated credential read")
                .as_str(),
            "rotated synthetic value"
        );
        store
            .delete("fixture")
            .expect("synthetic credential deletion");
        assert!(store.get("fixture").is_err());
        drop(cleanup);
    }
}
