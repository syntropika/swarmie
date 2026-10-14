// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
//! Per-user account authority independent of UI lifetime, Harnesses, and execution nodes.
mod cloud;
pub mod vault;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
pub use cloud::validate_origin;
use cloud::{Cloud, Credentials};
use rand::RngCore;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use vault::Vault;

pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("The account service is busy. Try again in {0} seconds")]
    RateLimited(u64),
    #[error("Invalid account request or response")]
    Invalid,
    #[error("Account service is unavailable; the request was not replayed")]
    Unavailable,
    #[error("Sign in again to use your account")]
    Unauthenticated,
    #[error("Profile changed; read it again before saving")]
    Conflict,
    #[error(
        "Secure account storage is unavailable. Unlock the system vault or explicitly use a session-only login"
    )]
    Vault,
    #[error("A sign-in is already in progress")]
    Busy,
}
/// Only application-approved profile fields can cross the native presentation boundary.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountProfile {
    pub id: uuid::Uuid,
    pub display_name: Option<String>,
    pub email: String,
    pub email_verified: bool,
    pub avatar_url: Option<String>,
    pub revision: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub status: String,
    pub generation: u64,
    pub profile: Option<AccountProfile>,
    pub persistence: String,
    pub login_id: Option<uuid::Uuid>,
    pub authorization_url: Option<String>,
}
/// Public metadata never includes bearer values, provider references or credential hashes.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSession {
    pub id: uuid::Uuid,
    pub installation_id: uuid::Uuid,
    pub authenticated_at: String,
    pub created_at: String,
    pub last_used_at: String,
    pub expires_at: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPage {
    pub current: AccountSession,
    pub others: Vec<AccountSession>,
    pub next_cursor: Option<uuid::Uuid>,
}
struct Pending {
    id: uuid::Uuid,
    state: String,
    verifier: String,
    callback: String,
    expires: Instant,
    session_only: bool,
    authorization_url: String,
}
struct State {
    blocked: bool,
    vault_unavailable: bool,
    credentials: Option<Credentials>,
    pending: Option<Pending>,
    generation: u64,
    session_only: bool,
    last_used: Instant,
    renewal_after: Option<Instant>,
}
/// Serializes code redemption, renewal, profile writes and logout under one credential owner.
pub struct AccountManager {
    cloud: Cloud,
    origin: String,
    installation: uuid::Uuid,
    vault: Arc<dyn Vault>,
    state: Mutex<State>,
}
#[derive(Serialize, Deserialize)]
struct Stored {
    origin: String,
    refresh_token: String,
}
impl AccountManager {
    pub async fn new(
        origin: &str,
        installation: uuid::Uuid,
        vault: Arc<dyn Vault>,
    ) -> Result<Self> {
        let cloud = Cloud::new(origin)?;
        let origin = validate_origin(origin)?.to_string();
        let mut vault_unavailable = false;
        let credentials = match vault.read().await {
            Ok(Some(value)) => {
                let stored: Stored = serde_json::from_str(&value).map_err(|_| Error::Vault)?;
                if stored.origin != origin {
                    return Err(Error::Vault);
                }
                Some(Credentials {
                    access_token: String::new(),
                    refresh_token: stored.refresh_token,
                    expires_at: 0,
                    profile: None,
                })
            }
            Ok(None) => None,
            Err(Error::Vault) => {
                vault_unavailable = true;
                None
            }
            Err(error) => return Err(error),
        };
        // Every restored credential must be revalidated before account-dependent work.
        let state = State {
            blocked: false,
            vault_unavailable,
            credentials,
            pending: None,
            generation: 0,
            session_only: false,
            last_used: Instant::now(),
            renewal_after: None,
        };
        let manager = Self {
            cloud,
            origin,
            installation,
            vault,
            state: Mutex::new(state),
        };
        Ok(manager)
    }
    fn snapshot(state: &State) -> Snapshot {
        Snapshot {
            status: if state.blocked {
                "storageUnavailable"
            } else if state.pending.is_some() {
                "signingIn"
            } else if state.credentials.is_some() {
                "signedIn"
            } else if state.vault_unavailable {
                "storageUnavailable"
            } else {
                "signedOut"
            }
            .into(),
            generation: state.generation,
            profile: state.credentials.as_ref().and_then(|c| c.profile.clone()),
            persistence: if state.session_only {
                "session"
            } else {
                "secureVault"
            }
            .into(),
            login_id: state.pending.as_ref().map(|p| p.id),
            authorization_url: state.pending.as_ref().map(|p| p.authorization_url.clone()),
        }
    }
    fn expire(state: &mut State) {
        if state
            .pending
            .as_ref()
            .is_some_and(|p| p.expires <= Instant::now())
        {
            state.pending = None;
            state.generation += 1;
        }
    }
    async fn save(&self, state: &State) -> Result<()> {
        if state.session_only {
            return Ok(());
        }
        let credentials = state.credentials.as_ref().ok_or(Error::Unauthenticated)?;
        self.vault
            .write(
                serde_json::to_string(&Stored {
                    origin: self.origin.clone(),
                    refresh_token: credentials.refresh_token.clone(),
                })
                .map_err(|_| Error::Invalid)?,
            )
            .await
    }
    async fn ensure(&self, state: &mut State) -> Result<()> {
        if state.blocked {
            return Err(Error::Vault);
        }
        let credentials = state.credentials.as_ref().ok_or(Error::Unauthenticated)?;
        if credentials.expires_at > now() + 60 {
            return Ok(());
        }
        if let Some(deadline) = state.renewal_after
            && deadline > Instant::now()
        {
            return Err(Error::RateLimited(
                deadline.saturating_duration_since(Instant::now()).as_secs() + 1,
            ));
        }
        let token = credentials.refresh_token.clone();
        state.blocked = true;
        let result = self
            .cloud
            .request(
                Method::POST,
                "/auth/native/refresh",
                Some(json!({"refreshToken":token})),
                None,
            )
            .await;
        match result {
            Err(Error::RateLimited(seconds)) => {
                // An explicit rejection precedes rotation; preserve the current one-use credential.
                state.blocked = false;
                state.renewal_after = Some(Instant::now() + Duration::from_secs(seconds));
                Err(Error::RateLimited(seconds))
            }
            Ok(value) => {
                let credentials: Credentials = match serde_json::from_value(value) {
                    Ok(value) => value,
                    Err(_) => {
                        self.clear(state).await?;
                        return Err(Error::Invalid);
                    }
                };
                state.credentials = Some(credentials);
                state.generation += 1;
                if let Err(error) = self.save(state).await {
                    self.clear(state).await?;
                    return Err(error);
                }
                state.blocked = false;
                state.renewal_after = None;
                Ok(())
            }
            // A renewal's outcome can be uncertain. Never replay its rotating token automatically.
            Err(error) => {
                self.clear(state).await?;
                Err(error)
            }
        }
    }
    async fn clear(&self, state: &mut State) -> Result<()> {
        state.blocked = true;
        if !state.session_only {
            self.vault.delete().await?;
        }
        state.credentials = None;
        state.pending = None;
        state.generation += 1;
        state.vault_unavailable = false;
        state.blocked = false;
        state.renewal_after = None;
        Ok(())
    }
    pub async fn read(&self) -> Result<Snapshot> {
        let mut state = self.state.lock().await;
        Self::expire(&mut state);
        state.last_used = Instant::now();
        if state.blocked {
            self.clear(&mut state).await?;
        }
        if state.vault_unavailable {
            match self.vault.read().await {
                Ok(Some(value)) => {
                    let stored: Stored = serde_json::from_str(&value).map_err(|_| Error::Vault)?;
                    if stored.origin != self.origin {
                        return Err(Error::Vault);
                    }
                    state.credentials = Some(Credentials {
                        access_token: String::new(),
                        refresh_token: stored.refresh_token,
                        expires_at: 0,
                        profile: None,
                    });
                    state.vault_unavailable = false;
                }
                Ok(None) => state.vault_unavailable = false,
                Err(Error::Vault) => return Ok(Self::snapshot(&state)),
                Err(error) => return Err(error),
            }
        }
        if state.credentials.is_some() {
            self.ensure(&mut state).await?;
            let token = &state.credentials.as_ref().unwrap().access_token;
            match self
                .cloud
                .request(Method::GET, "/api/native/profile", None, Some(token))
                .await
            {
                Ok(profile) => {
                    state.credentials.as_mut().unwrap().profile =
                        Some(serde_json::from_value(profile).map_err(|_| Error::Invalid)?);
                }
                Err(Error::Unauthenticated) => {
                    self.clear(&mut state).await?;
                }
                Err(error) => return Err(error),
            }
        }
        Ok(Self::snapshot(&state))
    }
    /// Observers receive only retained presentation state; observation never renews credentials.
    pub async fn retained_snapshot(&self) -> Snapshot {
        Self::snapshot(&*self.state.lock().await)
    }
    pub async fn start(&self, callback: String, session_only: bool) -> Result<Value> {
        let mut state = self.state.lock().await;
        Self::expire(&mut state);
        if state.pending.is_some() || state.credentials.is_some() || state.blocked {
            return Err(Error::Busy);
        }
        // Durable login cannot silently degrade; probe the vault before browser interaction.
        if !session_only {
            self.vault.check().await?;
        } else {
            self.vault.delete().await?;
        }
        let verifier = random();
        let nonce = random();
        let id = uuid::Uuid::new_v4();
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let value=self.cloud.request(Method::POST,"/auth/native/request",Some(json!({"installationId":self.installation,"callback":callback,"state":nonce,"challenge":challenge})),None).await?;
        let url = self
            .cloud
            .authorization_url(value["authorizationUrl"].as_str().ok_or(Error::Invalid)?)?;
        state.pending = Some(Pending {
            id,
            state: nonce,
            verifier,
            callback,
            expires: Instant::now() + Duration::from_secs(600),
            session_only,
            authorization_url: url.clone(),
        });
        state.generation += 1;
        Ok(
            json!({"loginId":id,"authorizationUrl":url,"expiresIn":600,"persistence":if session_only{"session"}else{"secureVault"}}),
        )
    }
    pub async fn complete(&self, nonce: &str, code: &str) -> Result<()> {
        let mut state = self.state.lock().await;
        Self::expire(&mut state);
        let pending = state.pending.as_ref().ok_or(Error::Unauthenticated)?;
        if pending.state != nonce {
            return Err(Error::Unauthenticated);
        }
        let pending = state.pending.take().unwrap();
        state.generation += 1;
        let value=self.cloud.request(Method::POST,"/auth/native/redeem",Some(json!({"code":code,"verifier":pending.verifier,"callback":pending.callback,"installationId":self.installation})),None).await?;
        state.session_only = pending.session_only;
        state.vault_unavailable = false;
        // Session-only login must not leave a prior durable family available on restart.
        if state.session_only {
            match self.vault.read().await {
                Ok(Some(_)) => self.vault.delete().await?,
                Ok(None) | Err(Error::Vault) => {}
                Err(error) => return Err(error),
            }
        }
        state.credentials = Some(serde_json::from_value(value).map_err(|_| Error::Invalid)?);
        if let Err(error) = self.save(&state).await {
            self.clear(&mut state).await?;
            return Err(error);
        }
        Ok(())
    }
    pub async fn cancel(&self, id: uuid::Uuid) -> Result<Snapshot> {
        let mut state = self.state.lock().await;
        Self::expire(&mut state);
        if state.pending.as_ref().is_some_and(|p| p.id != id) {
            return Err(Error::Invalid);
        }
        state.pending = None;
        state.generation += 1;
        Ok(Self::snapshot(&state))
    }
    pub async fn logout(&self) -> Result<Value> {
        self.logout_for(None).await
    }
    pub async fn logout_for(&self, expected_account: Option<uuid::Uuid>) -> Result<Value> {
        let mut state = self.state.lock().await;
        if let Some(expected) = expected_account
            && state
                .credentials
                .as_ref()
                .and_then(|credentials| credentials.profile.as_ref())
                .map(|profile| profile.id)
                != Some(expected)
        {
            return Err(Error::Unauthenticated);
        }
        let unknown = state.vault_unavailable;
        let token = state.credentials.as_ref().map(|c| c.refresh_token.clone());
        self.clear(&mut state).await?;
        let revoked = match token {
            Some(token) => self
                .cloud
                .request(
                    Method::POST,
                    "/auth/native/revoke",
                    Some(json!({"refreshToken":token})),
                    None,
                )
                .await
                .is_ok(),
            None => !unknown,
        };
        Ok(json!({"account":Self::snapshot(&state),"remoteRevocationConfirmed":revoked}))
    }
    pub async fn update(&self, body: Value) -> Result<Snapshot> {
        self.edit(Method::PATCH, "/api/native/profile", body).await
    }
    pub async fn avatar(&self, body: Value) -> Result<Snapshot> {
        self.edit(Method::POST, "/api/native/profile/avatar", body)
            .await
    }
    fn expect_account(state: &State, expected: uuid::Uuid) -> Result<()> {
        if state
            .credentials
            .as_ref()
            .and_then(|credentials| credentials.profile.as_ref())
            .map(|profile| profile.id)
            != Some(expected)
        {
            return Err(Error::Unauthenticated);
        }
        Ok(())
    }
    /// Explicit security reads are independent of observation and execution-node state.
    pub async fn sessions(
        &self,
        expected: uuid::Uuid,
        after: Option<uuid::Uuid>,
    ) -> Result<SessionPage> {
        let mut state = self.state.lock().await;
        Self::expect_account(&state, expected)?;
        self.ensure(&mut state).await?;
        let path = after.map_or_else(
            || "/api/native/sessions".to_owned(),
            |id| format!("/api/native/sessions?after={id}"),
        );
        let token = &state.credentials.as_ref().unwrap().access_token;
        match self
            .cloud
            .request(Method::GET, &path, None, Some(token))
            .await
        {
            Ok(value) => serde_json::from_value(value).map_err(|_| Error::Invalid),
            Err(Error::Unauthenticated) => {
                self.clear(&mut state).await?;
                Err(Error::Unauthenticated)
            }
            Err(error) => Err(error),
        }
    }
    /// Revoke one session or all native sessions. An uncertain response is never retried.
    pub async fn revoke_sessions(
        &self,
        expected: uuid::Uuid,
        session: Option<uuid::Uuid>,
    ) -> Result<Snapshot> {
        let mut state = self.state.lock().await;
        Self::expect_account(&state, expected)?;
        self.ensure(&mut state).await?;
        let path = session.map_or_else(
            || "/api/native/sessions".to_owned(),
            |id| format!("/api/native/sessions/{id}"),
        );
        let token = &state.credentials.as_ref().unwrap().access_token;
        let value = match self
            .cloud
            .request(Method::DELETE, &path, None, Some(token))
            .await
        {
            Ok(value) => value,
            Err(Error::Unauthenticated) => {
                self.clear(&mut state).await?;
                return Err(Error::Unauthenticated);
            }
            Err(error) => return Err(error),
        };
        let current = value["currentSessionRevoked"]
            .as_bool()
            .ok_or(Error::Invalid)?;
        if current {
            self.clear(&mut state).await?;
        }
        Ok(Self::snapshot(&state))
    }
    async fn edit(&self, method: Method, path: &str, body: Value) -> Result<Snapshot> {
        let mut state = self.state.lock().await;
        self.ensure(&mut state).await?;
        let credentials = state.credentials.as_ref().unwrap();
        if body["accountId"]
            != serde_json::to_value(
                credentials
                    .profile
                    .as_ref()
                    .ok_or(Error::Unauthenticated)?
                    .id,
            )
            .map_err(|_| Error::Invalid)?
        {
            return Err(Error::Unauthenticated);
        }
        let token = &credentials.access_token;
        let profile = self
            .cloud
            .request(method, path, Some(body), Some(token))
            .await?;
        state.credentials.as_mut().unwrap().profile =
            Some(serde_json::from_value(profile).map_err(|_| Error::Invalid)?);
        state.generation += 1;
        Ok(Self::snapshot(&state))
    }
    /// Runs in the owner process. Idle installations do not rotate unused grants indefinitely.
    pub async fn tick(&self) {
        let mut state = self.state.lock().await;
        Self::expire(&mut state);
        if state.credentials.is_some() && state.last_used.elapsed() < Duration::from_secs(1200) {
            let _ = self.ensure(&mut state).await;
        }
    }
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
fn random() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}
