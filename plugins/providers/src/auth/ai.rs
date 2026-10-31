use swarmie_ai::catalog::ModelIdentity;
use swarmie_ai::{
    AuthProfile, CredentialFuture, CredentialSource, Credentials, Effect, Error, Failure,
};

use crate::auth::{ChatgptRegistration, Credential, Provider, Result, invalid, now_ms};

/// A captured, published direct-plan credential for one selected registration and route.
/// Login and renewal use this crate's dynamic-registration protocol before capture;
/// account custody remains responsible for publication and rotated refresh tokens.
/// This source contains no login fallback, token store or background renewal task.
pub struct ChatgptCredentialSource {
    identity: ModelIdentity,
    access_token: zeroize::Zeroizing<String>,
    expires_at_ms: u64,
}

impl ChatgptCredentialSource {
    /// Captures credentials only after the host has published them for the selected registration.
    /// The selected registration is checked independently to refuse account/client substitution.
    /// Capturing authentication does not qualify a model or enable an inference profile.
    pub fn new(
        identity: ModelIdentity,
        selected: &ChatgptRegistration,
        credential: Credential,
    ) -> Result<Self> {
        selected.validate()?;
        let actual = credential.chatgpt.as_deref().ok_or_else(invalid)?;
        actual.validate()?;
        identity.validate().map_err(|_| invalid())?;
        if credential.provider != Provider::Chatgpt
            || identity.provider != "chatgpt"
            || identity.adapter != "swarmie-ai.responses.chatgpt.v1"
            || identity.endpoint != "https://api.openai.com/v1/responses"
            || credential.endpoint != "https://api.openai.com/v1/"
            || actual.client_id != selected.client_id
            || actual.host_id != selected.host_id
            || actual.identity.issuer != selected.identity.issuer
            || actual.identity.subject != selected.identity.subject
            || credential.expires_at_ms <= now_ms()
            || credential.access_token.is_empty()
            || credential.access_token.len() > 16384
            || credential
                .access_token
                .chars()
                .any(|c| c.is_control() || c.is_whitespace())
        {
            return Err(invalid());
        }
        Ok(Self {
            identity,
            access_token: credential.access_token,
            expires_at_ms: credential.expires_at_ms,
        })
    }
}

impl CredentialSource for ChatgptCredentialSource {
    fn resolve<'a>(&'a self, identity: &'a ModelIdentity) -> CredentialFuture<'a> {
        Box::pin(async move {
            let failure = if identity != &self.identity {
                Some(Failure::Credential)
            } else if self.expires_at_ms <= now_ms() {
                Some(Failure::Expired)
            } else {
                None
            };
            if let Some(failure) = failure {
                return Err(Error {
                    failure,
                    effect: Effect::NotDispatched,
                });
            }
            Credentials::new(
                self.identity.clone(),
                AuthProfile::ChatGpt,
                Some(self.access_token.clone()),
                Some(self.expires_at_ms),
            )
        })
    }
}
