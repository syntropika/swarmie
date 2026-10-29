//! Native AI-provider authentication without application persistence or runtime ownership.
//! Tokens and device grants remain on the native caller. Browser-facing instructions contain
//! no device code, PKCE verifier, access token or refresh token.

mod browser;
mod chatgpt;
mod credential;
mod protocol;
mod standard;

pub use browser::{
    BrowserInstructions, BrowserLogin, BrowserOptions, ChatgptBrowserOptions, begin_browser,
    begin_browser_with_options, begin_chatgpt_browser, begin_chatgpt_browser_with_options,
};
pub use credential::{
    ChatgptRegistration, Credential, Provider, VerifiedIdentity, decode, encode, now_ms,
};
pub use protocol::{Device, Endpoints, Outcome, ProviderClient, trusted_copilot_endpoint};

/// Authentication failures intentionally exclude raw HTTP bodies and credentials.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    #[error("invalid or expired provider authorization")]
    Invalid,
    #[error("provider authorization is unavailable; retry or begin a new sign-in")]
    Unavailable,
    #[error("provider authorization was cancelled")]
    Cancelled,
    #[error("provider authorization has expired")]
    Expired,
    #[error("provider authorization was denied")]
    Denied,
    #[error("provider authorization was already consumed")]
    Consumed,
    #[error("local provider authorization callback is unavailable")]
    CallbackUnavailable,
}

/// A provider authentication result, independent of the caller's runtime framework.
pub type Result<T> = std::result::Result<T, AuthError>;

fn invalid() -> AuthError {
    AuthError::Invalid
}
fn unavailable() -> AuthError {
    AuthError::Unavailable
}

/// Public verification instructions. The identifier is opaque; grant secrets remain in Device.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AuthSession {
    pub id: String,
    pub provider: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub user_code: String,
    pub expires_at_ms: u64,
    pub interval_ms: u32,
}

impl Device {
    /// Returns an independent, secret-erasing grant clone with a later polling interval.
    pub fn with_interval_ms(&self, interval_ms: u32) -> Self {
        Self {
            provider: self.provider,
            code: self.code.clone(),
            user_code: self.user_code.clone(),
            interval_ms,
        }
    }
}

/// Refreshes a protected native credential using its provider's fixed production endpoints.
pub async fn refreshed(credential: &Credential) -> Result<Credential> {
    if credential.provider == Provider::OpenRouter {
        return Ok(credential.clone());
    }
    ProviderClient::new()?.refresh(credential).await
}

#[cfg(test)]
mod tests;
