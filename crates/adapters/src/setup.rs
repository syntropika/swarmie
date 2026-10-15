pub mod installation;
pub mod native;
pub(super) mod opencode_auth;
pub mod providers;
mod service;
pub mod vault;

use async_trait::async_trait;
use serde_json::Value;
use std::path::Path;
use swarmie::Result;

pub use service::AgentToolSetup;

#[derive(Clone, Default)]
pub struct NativeReadiness {
    pub installed: bool,
    pub auth_state: String,
    pub message: Option<String>,
}
#[derive(Clone, Default)]
pub struct CredentialValidation {
    pub auth_state: String,
    pub message: Option<String>,
}

/// Provider operations stay on the runtime node and never submit inference prompts.
#[async_trait]
pub trait NativeSetup: Send + Sync {
    async fn readiness(&self, harness: &str) -> Result<NativeReadiness>;
    async fn readiness_at(
        &self,
        harness: &str,
        _config_home: Option<&Path>,
    ) -> Result<NativeReadiness> {
        self.readiness(harness).await
    }
    async fn readiness_with(
        &self,
        harness: &str,
        overlay: &swarmie::AgentToolExecutionOverlay,
    ) -> Result<NativeReadiness> {
        self.readiness_at(harness, overlay.config_home.as_deref())
            .await
    }
    async fn validate_key(
        &self,
        harness: &str,
        provider: &str,
        key: &str,
    ) -> Result<CredentialValidation>;
    async fn validate_key_with(
        &self,
        harness: &str,
        provider: &str,
        key: &str,
        _overlay: &swarmie::AgentToolExecutionOverlay,
    ) -> Result<CredentialValidation> {
        self.validate_key(harness, provider, key).await
    }
    async fn start_login(
        &self,
        id: uuid::Uuid,
        harness: &str,
        provider: &str,
        method: &str,
        private_root: &Path,
    ) -> Result<Value>;
    async fn flow(&self, id: uuid::Uuid) -> Result<Value>;
    async fn input(&self, id: uuid::Uuid, values: Value) -> Result<Value>;
    async fn cancel(&self, id: uuid::Uuid) -> Result<Value>;
    async fn release(&self, _id: uuid::Uuid) -> Result<()> {
        Ok(())
    }
    async fn login_credential(&self, _id: uuid::Uuid) -> Result<Option<String>> {
        Ok(None)
    }
    async fn login_overlay(
        &self,
        _id: uuid::Uuid,
    ) -> Result<Option<swarmie::AgentToolExecutionOverlay>> {
        Ok(None)
    }
    async fn shutdown(&self) -> Result<()>;
}
