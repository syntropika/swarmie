use crate::auth::Result;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use super::invalid;

const PREFIX: &str = "swarmie-oauth-v1:";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Chatgpt,
    Kimi,
    Copilot,
    OpenRouter,
    Xai,
}

impl Provider {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "openai" | "chatgpt" => Ok(Self::Chatgpt),
            "kimi" => Ok(Self::Kimi),
            "github-copilot" => Ok(Self::Copilot),
            "openrouter" => Ok(Self::OpenRouter),
            "xai" => Ok(Self::Xai),
            _ => Err(invalid()),
        }
    }
}

/// Native-only credential material. Clones also erase their token allocations on drop.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credential {
    pub provider: Provider,
    #[serde(with = "secret")]
    pub access_token: Zeroizing<String>,
    #[serde(with = "secret")]
    pub refresh_token: Zeroizing<String>,
    pub expires_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chatgpt: Option<Box<ChatgptRegistration>>,
    pub endpoint: String,
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credential")
            .field("provider", &self.provider)
            .field("expires_at_ms", &self.expires_at_ms)
            .finish_non_exhaustive()
    }
}

mod secret {
    use serde::{Deserialize, Deserializer, Serializer};
    use zeroize::Zeroizing;

    pub fn serialize<S: Serializer>(value: &str, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(value)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Zeroizing<String>, D::Error> {
        String::deserialize(deserializer).map(Zeroizing::new)
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

pub fn encode(value: &Credential) -> Result<Zeroizing<String>> {
    let body = Zeroizing::new(serde_json::to_string(value).map_err(|_| invalid())?);
    let encoded = Zeroizing::new(format!("{PREFIX}{}", body.as_str()));
    if encoded.len() > 32768 {
        return Err(invalid());
    }
    Ok(encoded)
}

/// Ordinary API keys are returned as `None`; malformed tagged credentials are never API keys.
pub fn decode(value: &str) -> Result<Option<Credential>> {
    let Some(body) = value.strip_prefix(PREFIX) else {
        return Ok(None);
    };
    if value.len() > 32768 {
        return Err(invalid());
    }
    let credential: Credential = serde_json::from_str(body).map_err(|_| invalid())?;
    if credential.access_token.is_empty()
        || (credential.refresh_token.is_empty() && credential.provider != Provider::OpenRouter)
        || credential.endpoint.is_empty()
        || credential.access_token.chars().any(char::is_control)
        || credential.refresh_token.chars().any(char::is_control)
    {
        return Err(invalid());
    }
    if credential.provider == Provider::Chatgpt {
        let registration = credential.chatgpt.as_ref().ok_or_else(invalid)?;
        registration.validate()?;
        if credential.endpoint != "https://api.openai.com/v1/" {
            return Err(invalid());
        }
    } else if credential.chatgpt.is_some() {
        return Err(invalid());
    }
    Ok(Some(credential))
}

/// Identity established only after the provider ID token passes signature and claims validation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedIdentity {
    pub issuer: String,
    pub subject: String,
    pub email: Option<String>,
}

/// Protected direct-plan registration, bound to an issued client and a persistent native host.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatgptRegistration {
    pub client_id: String,
    pub identity: VerifiedIdentity,
    pub host_id: String,
    #[serde(with = "secret")]
    pub id_token: Zeroizing<String>,
    pub scopes: Vec<String>,
    #[serde(with = "secret")]
    pub nonce: Zeroizing<String>,
}
impl std::fmt::Debug for ChatgptRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatgptRegistration")
            .field("client_id", &self.client_id)
            .finish_non_exhaustive()
    }
}
impl ChatgptRegistration {
    pub(crate) fn validate(&self) -> Result<()> {
        valid_client_id(&self.client_id)?;
        let host = self
            .host_id
            .strip_prefix("urn:uuid:")
            .and_then(|value| uuid::Uuid::parse_str(value).ok())
            .filter(|id| !id.is_nil());
        if host.is_none()
            || self.identity.issuer != "https://auth.openai.com"
            || self.identity.subject.is_empty()
            || self.identity.subject.len() > 512
            || self.identity.subject.chars().any(char::is_control)
            || self.id_token.is_empty()
            || self.id_token.len() > 16384
            || self.nonce.is_empty()
            || self.nonce.len() > 512
            || !self
                .scopes
                .iter()
                .any(|scope| scope == "chatgpt.tokens.use.direct")
        {
            return Err(invalid());
        }
        Ok(())
    }
}
pub(crate) fn valid_client_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 512
        || !value.is_ascii()
        || value.chars().any(char::is_control)
        || value == "dynamic_agent_client"
    {
        Err(invalid())
    } else {
        Ok(())
    }
}
