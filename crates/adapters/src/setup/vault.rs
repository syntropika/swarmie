use async_trait::async_trait;
use swarmie::{Error, Result};

#[async_trait]
pub trait SecretVault: Send + Sync {
    async fn read(&self, tool: &str) -> Result<Option<String>>;
    async fn write(&self, tool: &str, secret: String) -> Result<()>;
    async fn delete(&self, tool: &str) -> Result<()>;
    async fn check(&self, tool: &str) -> Result<()> {
        self.read(tool).await.map(|_| ())
    }
}

/// Provider credentials have an owner namespace separate from application accounts.
pub struct PlatformVault {
    owner: uuid::Uuid,
}
impl PlatformVault {
    pub fn new(owner: uuid::Uuid) -> Self {
        Self { owner }
    }
    async fn operation<T: Send + 'static>(
        &self,
        tool: &str,
        operation: impl FnOnce(keyring_core::Entry) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let service = format!("swarmie.agent-tools.{}.{}", self.owner, tool);
        tokio::task::spawn_blocking(move || operation(platform_entry(&service)?))
            .await
            .map_err(|_| unavailable())?
    }
}
pub fn unavailable() -> Error {
    Error::Provider("Secure provider credential storage is unavailable. Unlock the system vault or explicitly select session-only storage".into())
}
#[async_trait]
impl SecretVault for PlatformVault {
    async fn read(&self, tool: &str) -> Result<Option<String>> {
        self.operation(tool, |entry| match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(_) => Err(unavailable()),
        })
        .await
    }
    async fn write(&self, tool: &str, secret: String) -> Result<()> {
        self.operation(tool, move |entry| {
            entry.set_password(&secret).map_err(|_| unavailable())
        })
        .await
    }
    async fn delete(&self, tool: &str) -> Result<()> {
        self.operation(tool, |entry| match entry.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(_) => Err(unavailable()),
        })
        .await
    }
}
fn platform_entry(service: &str) -> Result<keyring_core::Entry> {
    use keyring_core::api::CredentialStoreApi;
    #[cfg(target_os = "linux")]
    let store = zbus_secret_service_keyring_store::Store::new().map_err(|_| unavailable())?;
    #[cfg(target_os = "macos")]
    let store = apple_native_keyring_store::keychain::Store::new().map_err(|_| unavailable())?;
    #[cfg(windows)]
    let store = windows_native_keyring_store::Store::new().map_err(|_| unavailable())?;
    #[cfg(any(target_os = "linux", target_os = "macos", windows))]
    {
        store
            .build(service, "provider-credential", None)
            .map_err(|_| unavailable())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = service;
        Err(unavailable())
    }
}
