//! Keyring entries contain device keys plus trust pins, never a returned envelope or V.
#[cfg(feature = "native-keyring")]
use crate::Error;
use crate::{Result, TrustBundle, VaultIdentity};
use uuid::Uuid;
#[cfg(feature = "native-keyring")]
use zeroize::Zeroizing;

pub trait ProtectedStorage {
    fn load_bundle(&self, identity: VaultIdentity, device: Uuid) -> Result<TrustBundle>;
    fn save_bundle(
        &self,
        identity: VaultIdentity,
        device: Uuid,
        bundle: &TrustBundle,
    ) -> Result<()>;
    fn forget_bundle(&self, identity: VaultIdentity, device: Uuid) -> Result<()>;
}

#[cfg(feature = "native-keyring")]
pub struct OsKeyring;

#[cfg(feature = "native-keyring")]
impl OsKeyring {
    fn entry(identity: VaultIdentity, device: Uuid) -> Result<keyring::Entry> {
        keyring::Entry::new(
            "swarmie.vault.v1",
            &format!(
                "{}/{}/{}/{}",
                identity.account, identity.vault, identity.epoch, device
            ),
        )
        .map_err(|_| Error::Unavailable)
    }
}

#[cfg(feature = "native-keyring")]
impl OsKeyring {
    fn pending_entry(identity: VaultIdentity, device: Uuid) -> Result<keyring::Entry> {
        keyring::Entry::new(
            "swarmie.vault.pending-device.v1",
            &format!(
                "{}/{}/{}/{}",
                identity.account, identity.vault, identity.epoch, device
            ),
        )
        .map_err(|_| Error::Unavailable)
    }
    /// Provisional recipient keys have no trust pins and cannot unlock any vault.
    pub fn save_pending_device(
        &self,
        identity: VaultIdentity,
        device: Uuid,
        keys: &crate::DeviceKeys,
    ) -> Result<()> {
        let bytes = Zeroizing::new(serde_json::to_vec(keys).map_err(|_| Error::Invalid)?);
        Self::pending_entry(identity, device)?
            .set_secret(&bytes)
            .map_err(|_| Error::Unavailable)
    }
    pub fn load_pending_device(
        &self,
        identity: VaultIdentity,
        device: Uuid,
    ) -> Result<crate::DeviceKeys> {
        let bytes = Zeroizing::new(
            Self::pending_entry(identity, device)?
                .get_secret()
                .map_err(|_| Error::Unavailable)?,
        );
        serde_json::from_slice(&bytes).map_err(|_| Error::Integrity)
    }
    pub fn forget_pending_device(&self, identity: VaultIdentity, device: Uuid) -> Result<()> {
        Self::pending_entry(identity, device)?
            .delete_credential()
            .map_err(|_| Error::Unavailable)
    }
}

#[cfg(feature = "native-keyring")]
impl ProtectedStorage for OsKeyring {
    fn load_bundle(&self, identity: VaultIdentity, device: Uuid) -> Result<TrustBundle> {
        let bytes = Zeroizing::new(
            Self::entry(identity, device)?
                .get_secret()
                .map_err(|_| Error::Unavailable)?,
        );
        let bundle: TrustBundle = serde_json::from_slice(&bytes).map_err(|_| Error::Integrity)?;
        if bundle.pins.identity != identity {
            return Err(Error::Integrity);
        }
        Ok(bundle)
    }

    fn save_bundle(
        &self,
        identity: VaultIdentity,
        device: Uuid,
        bundle: &TrustBundle,
    ) -> Result<()> {
        if bundle.pins.identity != identity {
            return Err(Error::Integrity);
        }
        let bytes = Zeroizing::new(serde_json::to_vec(bundle).map_err(|_| Error::Invalid)?);
        Self::entry(identity, device)?
            .set_secret(&bytes)
            .map_err(|_| Error::Unavailable)
    }

    fn forget_bundle(&self, identity: VaultIdentity, device: Uuid) -> Result<()> {
        Self::entry(identity, device)?
            .delete_credential()
            .map_err(|_| Error::Unavailable)
    }
}
