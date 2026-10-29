//! Direct ChatGPT plan authorization and verified OpenID identity.

use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header, jwk::JwkSet};
use oauth2::{CsrfToken, TokenResponse};
use reqwest::Client;
use serde::Deserialize;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{
    ChatgptRegistration, Credential, Provider, Result, VerifiedIdentity, invalid, now_ms,
    unavailable,
};

pub(crate) const AUTHORIZE: &str = "https://auth.openai.com/api/accounts/authorize";
pub(crate) const TOKEN: &str = "https://auth.openai.com/api/accounts/oauth/token";
pub(crate) const JWKS: &str = "https://auth.openai.com/.well-known/jwks.json";
pub(crate) const RESOURCE: &str = "https://api.openai.com/v1";
pub(crate) const SCOPE: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
pub(crate) const DYNAMIC_CLIENT: &str = "dynamic_agent_client";
const ISSUER: &str = "https://auth.openai.com";

pub(crate) struct Attempt {
    pub host_id: String,
    pub nonce: Zeroizing<String>,
    pub returning: Option<ChatgptRegistration>,
    pub jwks_url: String,
}

pub(crate) struct CodeGrant {
    pub code: Zeroizing<String>,
    pub verifier: Zeroizing<String>,
    pub client_id: String,
    pub redirect: String,
}

impl Attempt {
    pub fn new(host_id: Uuid, returning: Option<&Credential>, jwks_url: String) -> Result<Self> {
        if host_id.is_nil() {
            return Err(invalid());
        }
        let host_id = format!("urn:uuid:{host_id}");
        let returning = returning
            .map(|credential| {
                if credential.provider != Provider::Chatgpt {
                    return Err(invalid());
                }
                let registration = credential.chatgpt.as_deref().cloned().ok_or_else(invalid)?;
                registration.validate()?;
                if registration.host_id != host_id {
                    return Err(invalid());
                }
                Ok(registration)
            })
            .transpose()?;
        Ok(Self {
            host_id,
            nonce: Zeroizing::new(CsrfToken::new_random().secret().to_owned()),
            returning,
            jwks_url,
        })
    }

    pub fn issued_client(&self, callback: Option<String>) -> Result<String> {
        let client = match (&self.returning, callback) {
            (Some(previous), None) => previous.client_id.clone(),
            (Some(previous), Some(client)) if client == previous.client_id => client,
            (None, Some(client)) => client,
            _ => return Err(invalid()),
        };
        crate::credential::valid_client_id(&client)?;
        Ok(client)
    }

    pub async fn exchange(
        &self,
        client: Client,
        token_url: String,
        grant: CodeGrant,
    ) -> Result<Credential> {
        let token = crate::standard::exchange_code(
            client.clone(),
            &grant.client_id,
            token_url,
            grant.code.to_string(),
            grant.verifier.to_string(),
            &grant.redirect,
            Some(RESOURCE),
        )
        .await?;
        let id_token = token.extra_fields().id_token.as_ref().ok_or_else(invalid)?;
        let identity = validate_identity(
            &client,
            &self.jwks_url,
            id_token.secret(),
            &grant.client_id,
            &self.nonce,
            true,
        )
        .await?;
        if self.returning.as_ref().is_some_and(|previous| {
            previous.identity.issuer != identity.issuer
                || previous.identity.subject != identity.subject
        }) {
            return Err(invalid());
        }
        credential(
            &token,
            ChatgptRegistration {
                client_id: grant.client_id,
                identity,
                host_id: self.host_id.clone(),
                id_token: Zeroizing::new(id_token.secret().to_owned()),
                scopes: Vec::new(),
                nonce: self.nonce.clone(),
            },
        )
    }
}

pub(crate) async fn refresh(
    client: Client,
    token_url: String,
    jwks_url: &str,
    previous: &Credential,
) -> Result<Credential> {
    let mut registration = previous.chatgpt.as_deref().cloned().ok_or_else(invalid)?;
    registration.validate()?;
    if previous.provider != Provider::Chatgpt || previous.endpoint != "https://api.openai.com/v1/" {
        return Err(invalid());
    }
    let token = crate::standard::refresh(
        client.clone(),
        Provider::Chatgpt,
        &registration.client_id,
        token_url,
        &previous.refresh_token,
        Some(RESOURCE),
    )
    .await?;
    if let Some(id_token) = &token.extra_fields().id_token {
        let identity = validate_identity(
            &client,
            jwks_url,
            id_token.secret(),
            &registration.client_id,
            &registration.nonce,
            false,
        )
        .await?;
        if identity.issuer != registration.identity.issuer
            || identity.subject != registration.identity.subject
        {
            return Err(invalid());
        }
        registration.identity = identity;
        registration.id_token = Zeroizing::new(id_token.secret().to_owned());
    }
    credential(&token, registration)
}

fn credential(
    token: &crate::standard::Token,
    mut registration: ChatgptRegistration,
) -> Result<Credential> {
    let access = token.access_token().secret();
    let refresh = token
        .refresh_token()
        .map(|value| value.secret())
        .filter(|value| !value.is_empty())
        .ok_or_else(invalid)?;
    if access.is_empty()
        || access.len() > 16384
        || refresh.len() > 16384
        || access.chars().any(char::is_control)
        || refresh.chars().any(char::is_control)
        || token.token_type() != &oauth2::basic::BasicTokenType::Bearer
    {
        return Err(invalid());
    }
    registration.scopes = token
        .scopes()
        .ok_or_else(invalid)?
        .iter()
        .map(|scope| scope.as_str().to_owned())
        .collect();
    registration.validate()?;
    let lifetime = token
        .expires_in()
        .filter(|value| !value.is_zero())
        .ok_or_else(invalid)?
        .as_secs()
        .min(31_536_000);
    Ok(Credential {
        provider: Provider::Chatgpt,
        access_token: Zeroizing::new(access.to_owned()),
        refresh_token: Zeroizing::new(refresh.to_owned()),
        expires_at_ms: now_ms().saturating_add(lifetime * 1000),
        chatgpt: Some(Box::new(registration)),
        endpoint: "https://api.openai.com/v1/".into(),
    })
}

#[derive(Clone, Deserialize)]
struct Claims {
    iss: String,
    sub: String,
    exp: u64,
    iat: u64,
    nonce: Option<String>,
    email: Option<String>,
    email_verified: Option<bool>,
    azp: Option<String>,
}

async fn validate_identity(
    client: &Client,
    jwks_url: &str,
    id_token: &str,
    client_id: &str,
    nonce: &str,
    require_nonce: bool,
) -> Result<VerifiedIdentity> {
    if id_token.len() > 16384 {
        return Err(invalid());
    }
    let header = decode_header(id_token).map_err(|_| invalid())?;
    if header.alg != Algorithm::RS256 {
        return Err(invalid());
    }
    let kid = header
        .kid
        .as_deref()
        .filter(|kid| !kid.is_empty() && kid.len() < 512)
        .ok_or_else(invalid)?;
    let (status, body) = crate::protocol::read(client.get(jwks_url)).await?;
    if !status.is_success() {
        return Err(unavailable());
    }
    let keys: JwkSet = serde_json::from_slice(&body).map_err(|_| invalid())?;
    if keys.keys.len() > 32 {
        return Err(invalid());
    }
    let matching: Vec<_> = keys
        .keys
        .iter()
        .filter(|key| key.common.key_id.as_deref() == Some(kid))
        .collect();
    if matching.len() != 1 {
        return Err(invalid());
    }
    let key = matching[0];
    if !matches!(
        key.algorithm,
        jsonwebtoken::jwk::AlgorithmParameters::RSA(_)
    ) || key
        .common
        .key_algorithm
        .is_some_and(|algorithm| algorithm != jsonwebtoken::jwk::KeyAlgorithm::RS256)
        || key
            .common
            .public_key_use
            .as_ref()
            .is_some_and(|usage| *usage != jsonwebtoken::jwk::PublicKeyUse::Signature)
    {
        return Err(invalid());
    }
    let key = DecodingKey::from_jwk(key).map_err(|_| invalid())?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[client_id]);
    validation.set_issuer(&[ISSUER]);
    validation.set_required_spec_claims(&["exp", "iss", "sub", "aud"]);
    validation.leeway = 0;
    validation.validate_nbf = true;
    let claims = decode::<Claims>(id_token, &key, &validation)
        .map_err(|_| invalid())?
        .claims;
    if claims.sub.is_empty()
        || claims.sub.len() > 512
        || claims.sub.chars().any(char::is_control)
        || claims.exp <= now_ms() / 1000
        || claims.iat > now_ms() / 1000 + 60
        || claims
            .azp
            .as_deref()
            .is_some_and(|authorized| authorized != client_id)
    {
        return Err(invalid());
    }
    match claims.nonce {
        Some(actual) if CsrfToken::new(actual.clone()) != CsrfToken::new(nonce.to_owned()) => {
            return Err(invalid());
        }
        None if require_nonce => return Err(invalid()),
        _ => {}
    }
    Ok(VerifiedIdentity {
        issuer: claims.iss,
        subject: claims.sub,
        email: claims.email.filter(|email| {
            claims.email_verified == Some(true)
                && !email.is_empty()
                && email.len() <= 1024
                && !email.chars().any(char::is_control)
        }),
    })
}

#[cfg(test)]
mod tests;
