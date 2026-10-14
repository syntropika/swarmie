// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
//! Secure storage adapters. Session-only use is explicit and never writes credentials to disk.
use crate::{Error, Result};
use async_trait::async_trait;

#[async_trait]
pub trait Vault: Send + Sync {
    async fn check(&self) -> Result<()> {
        self.read().await.map(|_| ())
    }
    async fn read(&self) -> Result<Option<String>>;
    async fn write(&self, secret: String) -> Result<()>;
    async fn delete(&self) -> Result<()>;
}
/// The service name is installation-scoped, so unrelated owners cannot rotate the same grant.
pub struct PlatformVault {
    name: String,
}
impl PlatformVault {
    pub fn new(installation: uuid::Uuid) -> Self {
        Self {
            name: format!("swarmie.account.{installation}"),
        }
    }
    async fn operation<T: Send + 'static>(
        &self,
        operation: impl FnOnce(keyring_core::Entry) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let name = self.name.clone();
        tokio::task::spawn_blocking(move || {
            let entry = platform_entry(&name)?;
            operation(entry)
        })
        .await
        .map_err(|_| Error::Vault)?
    }
}
#[async_trait]
impl Vault for PlatformVault {
    async fn read(&self) -> Result<Option<String>> {
        self.operation(|entry| match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(_) => Err(Error::Vault),
        })
        .await
    }
    async fn write(&self, secret: String) -> Result<()> {
        self.operation(move |entry| entry.set_password(&secret).map_err(|_| Error::Vault))
            .await
    }
    async fn delete(&self) -> Result<()> {
        self.operation(|entry| match entry.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(_) => Err(Error::Vault),
        })
        .await
    }
}

fn platform_entry(name: &str) -> Result<keyring_core::Entry> {
    use keyring_core::api::CredentialStoreApi;
    #[cfg(target_os = "linux")]
    let store = zbus_secret_service_keyring_store::Store::new().map_err(|_| Error::Vault)?;
    #[cfg(target_os = "macos")]
    let store = apple_native_keyring_store::keychain::Store::new().map_err(|_| Error::Vault)?;
    #[cfg(windows)]
    let store = windows_native_keyring_store::Store::new().map_err(|_| Error::Vault)?;
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    {
        store.build(name, "account", None).map_err(|_| Error::Vault)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        Err(Error::Vault)
    }
}
