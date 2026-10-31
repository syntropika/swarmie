use crate::account::{Endpoints, Error, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

/// Stored only by the explicitly supplied protected account-session adapter.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredSession {
    pub(crate) version: u8,
    pub(crate) account: Uuid,
    pub(crate) installation: Uuid,
    pub(crate) family: Uuid,
    pub(crate) endpoints: Endpoints,
    pub(crate) access_token: String,
    pub(crate) refresh_token: String,
    pub(crate) expires_at: i64,
    pub(crate) renewal_unknown: bool,
}

impl Drop for StoredSession {
    fn drop(&mut self) {
        self.access_token.zeroize();
        self.refresh_token.zeroize();
    }
}
impl StoredSession {
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        let session: Self = serde_json::from_slice(bytes).map_err(|_| Error::Integrity)?;
        session.endpoints.validate()?;
        if session.version != 1
            || session.access_token.is_empty()
            || session.access_token.len() > 8192
            || session.refresh_token.is_empty()
            || session.refresh_token.len() > 8192
        {
            return Err(Error::Integrity);
        }
        Ok(session)
    }
    pub fn write_private<T>(&self, sink: impl FnOnce(&[u8]) -> Result<T>) -> Result<T> {
        let bytes = Zeroizing::new(serde_json::to_vec(self).map_err(|_| Error::Invalid)?);
        sink(&bytes)
    }
}

/// No plaintext-file or environment fallback. Errors preserve existing enrolled device keys.
pub trait SessionStorage {
    fn load(&self, account: Uuid, installation: Uuid) -> Result<StoredSession>;
    fn save(&self, session: &StoredSession) -> Result<()>;
    fn forget(&self, account: Uuid, installation: Uuid) -> Result<()>;
}

#[cfg(feature = "native-keyring")]
pub struct KeyringSessions;
#[cfg(feature = "native-keyring")]
impl KeyringSessions {
    fn entry(account: Uuid, installation: Uuid) -> Result<keyring::Entry> {
        keyring::Entry::new(
            "swarmie.account.session.v1",
            &format!("{account}/{installation}"),
        )
        .map_err(|_| Error::ProtectedStorageUnavailable)
    }
}
#[cfg(feature = "native-keyring")]
impl SessionStorage for KeyringSessions {
    fn load(&self, account: Uuid, installation: Uuid) -> Result<StoredSession> {
        let bytes = Zeroizing::new(Self::entry(account, installation)?.get_secret().map_err(
            |error| {
                if matches!(error, keyring::Error::NoEntry) {
                    Error::SignInRequired
                } else {
                    Error::ProtectedStorageUnavailable
                }
            },
        )?);
        let session = StoredSession::decode(&bytes)?;
        if session.account != account || session.installation != installation {
            return Err(Error::Integrity);
        }
        Ok(session)
    }
    fn save(&self, session: &StoredSession) -> Result<()> {
        session.write_private(|bytes| {
            Self::entry(session.account, session.installation)?
                .set_secret(bytes)
                .map_err(|_| Error::ProtectedStorageUnavailable)
        })
    }
    fn forget(&self, account: Uuid, installation: Uuid) -> Result<()> {
        match Self::entry(account, installation)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(Error::ProtectedStorageUnavailable),
        }
    }
}
