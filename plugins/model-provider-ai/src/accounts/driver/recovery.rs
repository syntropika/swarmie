use super::{AiAccountDriver, failure};
use sha2::{Digest, Sha256};
use swarmie_provider_auth::Credential;
use swarmie_providers::{AccountRecord, ProtocolBinding};
use zeroize::Zeroizing;

impl AiAccountDriver {
    /// Bind the exact injected registration and nonsecret protocol options.
    /// Trusted endpoint configuration must not contain credential material.
    /// This performs no I/O and grants no account or recovery authority.
    pub fn protocol_binding(&self) -> tembra::Result<ProtocolBinding> {
        // Bump this domain when protocol semantics or tuple framing changes.
        let encoded = serde_json::to_vec(&(
            "swarmie.ai-account-driver.protocol.v1",
            self.registration_scope,
            &self.chatgpt.browser.authorization_url,
            &self.chatgpt.browser.token_url,
            &self.chatgpt.jwks_url,
            self.chatgpt.browser.callback_port,
            self.chatgpt.browser.timeout.as_secs(),
            self.chatgpt.browser.timeout.subsec_nanos(),
            &self.catalog_endpoint,
        ))
        .map_err(|_| failure("the account protocol binding could not be encoded"))?;
        Ok(ProtocolBinding::new(Sha256::digest(encoded).into()))
    }

    /// A pure decision over the exact account and credential capture. The owner
    /// must retain that capture and supplied time through its renewal intent and
    /// call to renew. A true result does not authorize an exchange or a retry.
    pub fn needs_renewal(
        &self,
        account: &AccountRecord,
        credential: &Zeroizing<String>,
        now_ms: u64,
    ) -> tembra::Result<bool> {
        Ok(self
            .renewal_credential(account, credential, now_ms)?
            .is_some())
    }

    pub(super) fn renewal_credential(
        &self,
        account: &AccountRecord,
        encoded: &str,
        now_ms: u64,
    ) -> tembra::Result<Option<Credential>> {
        Ok(self
            .credential(account, encoded)?
            .filter(|credential| credential.expires_at_ms <= now_ms.saturating_add(60_000)))
    }
}
