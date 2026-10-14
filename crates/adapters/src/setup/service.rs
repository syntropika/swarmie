use super::{CredentialValidation, NativeSetup, installation, providers, vault::SecretVault};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use swarmie::{AgentToolExecutionOverlay, AgentToolSetupPort, Error, Result};
use tokio::sync::Mutex;

#[derive(Clone, Serialize, Deserialize)]
struct Profile {
    provider: String,
    method: String,
    storage: String,
    verified: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    config_home: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    config: Value,
}
#[derive(Clone, Serialize, Deserialize)]
struct Credential {
    provider: String,
    method: String,
    api_key: String,
}
#[derive(Serialize, Deserialize)]
struct Metadata {
    owner: uuid::Uuid,
    #[serde(default)]
    profiles: BTreeMap<String, Profile>,
    #[serde(default)]
    blocked: BTreeSet<String>,
}
struct Flow {
    harness: String,
    profile: Profile,
    value: Value,
    install_cancel: Option<Arc<AtomicBool>>,
}
struct CachedValidation {
    provider: String,
    key: String,
    value: CredentialValidation,
    at: std::time::Instant,
}
struct State {
    profiles: BTreeMap<String, Profile>,
    session_keys: BTreeMap<String, Credential>,
    blocked: BTreeSet<String>,
    flows: BTreeMap<uuid::Uuid, Flow>,
    installations: BTreeMap<uuid::Uuid, tokio::task::JoinHandle<()>>,
}

/// One provider connection per tool, owned by the retained runtime identity.
pub struct AgentToolSetup {
    owner: uuid::Uuid,
    root: PathBuf,
    native: Arc<dyn NativeSetup>,
    vault: Arc<dyn SecretVault>,
    state: Arc<Mutex<State>>,
    checks: Mutex<BTreeMap<String, CachedValidation>>,
}
impl AgentToolSetup {
    pub fn new(
        owner: uuid::Uuid,
        private_root: impl AsRef<Path>,
        native: Arc<dyn NativeSetup>,
        vault: Arc<dyn SecretVault>,
    ) -> Result<Self> {
        let root = private_root.as_ref().join("agent-tool-connections");
        private_directory(&root)?;
        let root = std::fs::canonicalize(root)?;
        let metadata = match std::fs::read(root.join("connections.json")) {
            Ok(bytes) => {
                if bytes.len() > 65536 {
                    return Err(Error::Invalid(
                        "Agent tool connection metadata exceeds its size limit".into(),
                    ));
                }
                let stored: Metadata = serde_json::from_slice(&bytes)
                    .map_err(|_| Error::Invalid("Invalid agent tool connection metadata".into()))?;
                if stored.owner != owner {
                    return Err(Error::Invalid(
                        "Agent tool connection metadata belongs to another runtime owner".into(),
                    ));
                }
                for harness in stored.profiles.keys().chain(stored.blocked.iter()) {
                    providers::descriptor(harness)?;
                }
                stored
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Metadata {
                owner,
                profiles: BTreeMap::new(),
                blocked: BTreeSet::new(),
            },
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            owner,
            root,
            native,
            vault,
            checks: Mutex::new(BTreeMap::new()),
            state: Arc::new(Mutex::new(State {
                profiles: metadata.profiles,
                blocked: metadata.blocked,
                session_keys: BTreeMap::new(),
                flows: BTreeMap::new(),
                installations: BTreeMap::new(),
            })),
        })
    }
    fn persist(
        &self,
        profiles: &BTreeMap<String, Profile>,
        blocked: &BTreeSet<String>,
    ) -> Result<()> {
        let profiles = profiles
            .iter()
            .filter(|(_, profile)| profile.storage != "session")
            .map(|(id, profile)| (id.clone(), profile.clone()))
            .collect();
        let bytes = serde_json::to_vec(&Metadata {
            owner: self.owner,
            profiles,
            blocked: blocked.clone(),
        })?;
        let temporary = self
            .root
            .join(format!("connections-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            use std::io::Write;
            let mut options = std::fs::OpenOptions::new();
            options.create_new(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            std::fs::rename(&temporary, self.root.join("connections.json"))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temporary);
        }
        result
    }
    async fn credential(
        &self,
        harness: &str,
        profile: &Profile,
        session: Option<&Credential>,
    ) -> Result<Credential> {
        let credential = if profile.storage == "session" {
            session.cloned().ok_or_else(|| {
                Error::Invalid(
                    "The session-only connection has expired; reconnect this agent tool".into(),
                )
            })?
        } else {
            let secret = self.vault.read(harness).await?.ok_or_else(|| Error::Invalid("Provider credentials are missing from the secure vault; reconnect this agent tool".into()))?;
            serde_json::from_str::<Credential>(&secret)
                .map_err(|_| Error::Invalid("Invalid secure provider credential".into()))?
        };
        if credential.provider != profile.provider || credential.method != profile.method {
            return Err(Error::Invalid(
                "Provider connection and secure credential do not match; reconnect this agent tool"
                    .into(),
            ));
        }
        Ok(credential)
    }
    fn method(harness: &str, provider: &str, method: &str) -> Result<String> {
        let catalog = providers::catalog(harness)?;
        catalog["providers"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|entry| entry["id"] == provider)
            .and_then(|entry| entry["methods"].as_array())
            .and_then(|methods| methods.iter().find(|entry| entry["id"] == method))
            .and_then(|method| method["kind"].as_str())
            .map(String::from)
            .ok_or_else(|| {
                Error::Invalid("Unknown provider or connection method for this agent tool".into())
            })
    }
    async fn add_flow(
        &self,
        state: &mut State,
        id: uuid::Uuid,
        harness: String,
        profile: Profile,
        value: Value,
    ) -> Result<Value> {
        if state.flows.len() >= 128 {
            let old = state
                .flows
                .iter()
                .find(|(_, flow)| flow.value["status"] != "pending")
                .map(|(id, _)| *id);
            if let Some(old) = old {
                state.flows.remove(&old);
                let _ = self.native.release(old).await;
            } else {
                return Err(Error::Busy);
            }
        }
        let value = public_flow(id, value)?;
        state.flows.insert(
            id,
            Flow {
                harness,
                profile,
                value: value.clone(),
                install_cancel: None,
            },
        );
        Ok(value)
    }
    async fn apply_native_flow(
        &self,
        state: &mut State,
        id: uuid::Uuid,
        value: Value,
    ) -> Result<Value> {
        let mut value = public_flow(id, value)?;
        let flow = state
            .flows
            .get(&id)
            .ok_or_else(|| Error::Invalid("Unknown agent tool connection flow".into()))?;
        if value["status"] == "connected" {
            let harness = flow.harness.clone();
            let mut profile = flow.profile.clone();
            let existing =
                Self::method(&harness, &profile.provider, &profile.method)? == "existing";
            if !existing {
                profile.verified = true;
            }
            let credential = if existing {
                None
            } else {
                self.native.login_credential(id).await?
            };
            let credential = if let Some(key) = credential {
                if key.trim().is_empty() || key.len() > 16384 || key.chars().any(char::is_control) {
                    return Err(Error::Invalid(
                        "The provider returned an invalid managed credential".into(),
                    ));
                }
                if let Some(message) = value["message"].as_str() {
                    value["message"] = json!(message.replace(&key, "[redacted]"));
                }
                Some(Credential {
                    provider: profile.provider.clone(),
                    method: profile.method.clone(),
                    api_key: key,
                })
            } else {
                profile.storage = "native".into();
                if let Some(overlay) = if existing {
                    Some(providers::existing_overlay(&harness, &profile.provider)?)
                } else {
                    self.native.login_overlay(id).await?
                } {
                    if !overlay.env.is_empty() {
                        return Err(Error::Invalid(
                            "A native login returned credentials without a secure storage method"
                                .into(),
                        ));
                    }
                    profile.config_home = overlay.config_home;
                    profile.config = overlay.config;
                }
                None
            };
            if let Err(error) = self.commit(state, &harness, profile, credential).await {
                state.flows.get_mut(&id).unwrap().value["message"] = json!(
                    "Sign-in completed but credential storage is unavailable. Unlock storage and check again, or cancel and choose session-only storage"
                );
                return Err(error);
            }
        }
        state.flows.get_mut(&id).unwrap().value = value.clone();
        if value["status"] == "connected"
            || value["status"] == "failed"
            || value["status"] == "cancelled"
        {
            let _ = self.native.release(id).await;
        }
        Ok(value)
    }
    async fn commit(
        &self,
        state: &mut State,
        harness: &str,
        profile: Profile,
        credential: Option<Credential>,
    ) -> Result<()> {
        let previous_secure = state
            .profiles
            .get(harness)
            .is_some_and(|profile| profile.storage == "secure");
        let changes_vault = profile.storage == "secure" || previous_secure;
        let previous_secret = if changes_vault {
            self.vault.read(harness).await?
        } else {
            None
        };
        if profile.storage == "secure" {
            let credential = credential.as_ref().ok_or_else(|| {
                Error::Invalid("A secure connection requires a provider credential".into())
            })?;
            self.vault
                .write(harness, serde_json::to_string(credential)?)
                .await?;
        } else if previous_secure {
            self.vault.delete(harness).await?;
        }
        let mut profiles = state.profiles.clone();
        profiles.insert(harness.into(), profile.clone());
        let mut blocked = state.blocked.clone();
        if profile.storage == "session" {
            blocked.insert(harness.into());
        } else {
            blocked.remove(harness);
        }
        if let Err(error) = self.persist(&profiles, &blocked) {
            if changes_vault {
                if let Some(secret) = previous_secret {
                    let _ = self.vault.write(harness, secret).await;
                } else {
                    let _ = self.vault.delete(harness).await;
                }
            }
            return Err(error);
        }
        self.checks.lock().await.remove(harness);
        state.profiles = profiles;
        state.blocked = blocked;
        if profile.storage == "session" {
            state.session_keys.insert(
                harness.into(),
                credential.ok_or_else(|| {
                    Error::Invalid("A session connection requires a provider credential".into())
                })?,
            );
        } else {
            state.session_keys.remove(harness);
        }
        Ok(())
    }
    async fn cached_validation(
        &self,
        harness: &str,
        provider: &str,
        key: &str,
    ) -> Result<CredentialValidation> {
        if let Some(cached) = self.checks.lock().await.get(harness)
            && cached.provider == provider
            && cached.key == key
            && cached.at.elapsed() < std::time::Duration::from_secs(10)
        {
            return Ok(cached.value.clone());
        }
        let value = self.validation(harness, provider, key).await?;
        self.checks.lock().await.insert(
            harness.into(),
            CachedValidation {
                provider: provider.into(),
                key: key.into(),
                value: value.clone(),
                at: std::time::Instant::now(),
            },
        );
        Ok(value)
    }
    async fn validation(
        &self,
        harness: &str,
        provider: &str,
        key: &str,
    ) -> Result<CredentialValidation> {
        let overlay = self.managed_overlay(harness, AgentToolExecutionOverlay::default());
        self.native.validate_key_with(harness, provider, key, &overlay).await.map_err(|_| Error::Provider("Provider credential verification failed. Check the credential and provider connectivity".into()))
    }
}

#[async_trait]
impl AgentToolSetupPort for AgentToolSetup {
    async fn descriptor(&self, harness: &str) -> Result<Value> {
        if !supported(harness) {
            return Ok(
                json!({"harness":harness,"installed":false,"manual":true,"authState":"unknown","storage":{"persistent":false,"description":"Credentials for this ACP tool are managed by its native provider"},"providers":[],"message":"Connect this ACP tool using its provider's native instructions. Existing credentials are used at launch"}),
            );
        }
        let (profile, session, blocked, pending_flow) = {
            let state = self.state.lock().await;
            (
                state.profiles.get(harness).cloned(),
                state.session_keys.get(harness).cloned(),
                state.blocked.contains(harness),
                state
                    .flows
                    .values()
                    .find(|flow| flow.harness == harness && flow.value["status"] == "pending")
                    .map(|flow| flow.value.clone()),
            )
        };
        let config_home = profile
            .as_ref()
            .and_then(|profile| profile.config_home.as_deref());
        let overlay = self.managed_overlay(
            harness,
            AgentToolExecutionOverlay {
                config_home: config_home.map(PathBuf::from),
                ..Default::default()
            },
        );
        let mut descriptor = providers::descriptor_with(harness, &overlay.config)?;
        let native = self.native.readiness_with(harness, &overlay).await?;
        let persistent = self.vault.check(harness).await.is_ok();
        descriptor["harness"] = json!(harness);
        descriptor["installed"] = json!(native.installed);
        descriptor["installation"] = json!({"supported":installation::supported(harness),"message":installation::message(harness)});
        descriptor["storage"] = json!({"persistent":persistent,"description":if persistent {"API keys can be stored in the system credential vault. Native sign-in uses the provider's own credential storage."} else {"The system credential vault is unavailable. API keys require explicit session-only storage. Native sign-in uses the provider's own credential storage."}});
        descriptor["authState"] = json!(native.auth_state);
        if let Some(message) = native.message {
            descriptor["message"] = json!(message);
        }
        if let Some(mut profile) = profile {
            if profile.storage == "secure" || profile.storage == "session" {
                match self.credential(harness, &profile, session.as_ref()).await {
                    Ok(credential) => match self
                        .cached_validation(harness, &profile.provider, &credential.api_key)
                        .await
                    {
                        Ok(verified) => {
                            profile.verified = verified.auth_state == "authenticated";
                            descriptor["authState"] = json!(verified.auth_state);
                            if let Some(message) = verified.message {
                                descriptor["message"] =
                                    json!(message.replace(&credential.api_key, "[redacted]"));
                            }
                        }
                        Err(_) => {
                            profile.verified = false;
                            descriptor["authState"] = json!("unknown");
                            descriptor["message"] = json!(
                                "The configured provider credential could not be verified; check provider connectivity or reconnect"
                            );
                        }
                    },
                    Err(_) => {
                        profile.verified = false;
                        descriptor["authState"] = json!("unauthenticated");
                        descriptor["message"] = json!(
                            "The configured provider credential is unavailable; unlock the secure vault or reconnect"
                        );
                    }
                }
            } else {
                profile.verified = descriptor["authState"] == "authenticated";
            }
            descriptor["connection"] = json!({"provider":profile.provider,"method":profile.method,"storage":profile.storage,"verified":profile.verified});
        } else if blocked {
            descriptor["authState"] = json!("unauthenticated");
            descriptor["message"] = json!(
                "This agent tool is disconnected. Choose a connection method to use it again"
            );
        }
        if let Some(flow) = pending_flow {
            descriptor["flow"] = flow;
        }
        Ok(descriptor)
    }
    async fn start(&self, params: Value) -> Result<Value> {
        let harness = field(&params, "harness")?;
        let provider = field(&params, "provider")?;
        let method = field(&params, "method")?;
        let kind = Self::method(harness, provider, method)?;
        let storage = field(&params, "storage")?;
        if storage != "secure" && storage != "session" {
            return Err(Error::Invalid(
                "Choose secure or session-only credential storage".into(),
            ));
        }
        let mut state = self.state.lock().await;
        if state
            .flows
            .values()
            .any(|flow| flow.harness == harness && flow.value["status"] == "pending")
        {
            return Err(Error::Busy);
        }
        let id = uuid::Uuid::new_v4();
        let mut profile = Profile {
            provider: provider.into(),
            method: method.into(),
            storage: storage.into(),
            verified: false,
            config_home: None,
            config: Value::Null,
        };
        if kind == "api_key" {
            let key = field(&params["values"], "apiKey")?;
            if key.trim().is_empty() || key.len() > 16384 || key.chars().any(char::is_control) {
                return Err(Error::Invalid("Enter a valid provider API key".into()));
            }
            let verified = self.validation(harness, provider, key).await?;
            if verified.auth_state == "unauthenticated" {
                return Err(Error::Invalid("The provider rejected this API key".into()));
            }
            profile.verified = verified.auth_state == "authenticated";
            let credential = Credential {
                provider: provider.into(),
                method: method.into(),
                api_key: key.into(),
            };
            self.commit(&mut state, harness, profile.clone(), Some(credential))
                .await?;
            let message = if profile.verified {
                "Provider credential verified without running a model"
            } else {
                "Provider credential configured; authentication has not been confirmed"
            };
            self.add_flow(
                &mut state,
                id,
                harness.into(),
                profile,
                json!({"status":"connected","message":message}),
            )
            .await
        } else if kind == "existing" {
            let overlay =
                self.managed_overlay(harness, providers::existing_overlay(harness, provider)?);
            let native = self.native.readiness_with(harness, &overlay).await?;
            if !native.installed || native.auth_state == "unauthenticated" {
                return Err(Error::Invalid("This agent tool is missing or its native provider reports that sign-in is required; install it or use a sign-in method first".into()));
            }
            profile.verified = native.auth_state == "authenticated";
            let message = if profile.verified {
                "Using the authenticated native provider account"
            } else {
                "Using the configured native provider account; authentication has not been confirmed"
            };
            let flow = self
                .add_flow(
                    &mut state,
                    id,
                    harness.into(),
                    profile,
                    json!({"status":"connected","message":message}),
                )
                .await?;
            self.apply_native_flow(&mut state, id, flow).await
        } else if kind == "browser" || kind == "device" {
            let value = self
                .native
                .start_login(id, harness, provider, method, &self.root)
                .await?;
            let flow = self
                .add_flow(&mut state, id, harness.into(), profile, value)
                .await?;
            if flow["status"] == "connected" {
                self.apply_native_flow(&mut state, id, flow).await
            } else {
                Ok(flow)
            }
        } else {
            Err(Error::Unsupported("Connection method".into()))
        }
    }
    async fn flow(&self, id: &str) -> Result<Value> {
        let id = flow_id(id)?;
        let mut state = self.state.lock().await;
        let current = state
            .flows
            .get(&id)
            .ok_or_else(|| Error::Invalid("Unknown agent tool connection flow".into()))?
            .value
            .clone();
        if state.flows[&id].install_cancel.is_some() {
            return Ok(current);
        }
        if current["status"] != "pending" {
            return Ok(current);
        }
        let value = self.native.flow(id).await?;
        self.apply_native_flow(&mut state, id, value).await
    }
    async fn input(&self, id: &str, values: Value) -> Result<Value> {
        let id = flow_id(id)?;
        let mut state = self.state.lock().await;
        let current = state
            .flows
            .get(&id)
            .ok_or_else(|| Error::Invalid("Unknown agent tool connection flow".into()))?;
        if current.install_cancel.is_some() {
            return Err(Error::Invalid(
                "An installation flow does not accept login input".into(),
            ));
        }
        if current.value["status"] != "pending" {
            return Err(Error::Invalid(
                "The connection flow is no longer accepting input".into(),
            ));
        }
        let fields = current.value["inputFields"]
            .as_array()
            .ok_or_else(|| Error::Invalid("This connection flow is not requesting input".into()))?;
        let inputs = values
            .as_object()
            .ok_or_else(|| Error::Invalid("Connection input must be an object".into()))?;
        if inputs.is_empty()
            || inputs.iter().any(|(id, value)| {
                !fields.iter().any(|field| field["id"] == id.as_str())
                    || value.as_str().is_none_or(|text| {
                        text.is_empty() || text.len() > 16384 || text.chars().any(char::is_control)
                    })
            })
        {
            return Err(Error::Invalid(
                "Connection input contains invalid or unexpected fields".into(),
            ));
        }
        let value = self.native.input(id, values).await?;
        self.apply_native_flow(&mut state, id, value).await
    }
    async fn cancel(&self, id: &str) -> Result<Value> {
        let id = flow_id(id)?;
        let mut state = self.state.lock().await;
        let current = state
            .flows
            .get(&id)
            .ok_or_else(|| Error::Invalid("Unknown agent tool connection flow".into()))?
            .value
            .clone();
        if current["status"] != "pending" {
            return Ok(current);
        }
        if let Some(cancel) = &state.flows[&id].install_cancel {
            cancel.store(true, Ordering::SeqCst);
            return Ok(current);
        }
        self.native.cancel(id).await?;
        let value = json!({"id":id,"status":"cancelled","message":"Connection cancelled"});
        state.flows.get_mut(&id).unwrap().value = value.clone();
        let _ = self.native.release(id).await;
        Ok(value)
    }
    async fn disconnect(&self, harness: &str) -> Result<Value> {
        providers::descriptor(harness)?;
        let mut state = self.state.lock().await;
        let pending: Vec<_> = state
            .flows
            .iter()
            .filter(|(_, flow)| flow.harness == harness && flow.value["status"] == "pending")
            .map(|(id, _)| *id)
            .collect();
        for id in pending {
            if let Some(cancel) = &state.flows[&id].install_cancel {
                cancel.store(true, Ordering::SeqCst);
            } else {
                self.native.cancel(id).await?;
                state.flows.get_mut(&id).unwrap().value = json!({"id":id,"status":"cancelled"});
                let _ = self.native.release(id).await;
            }
        }
        let owned_secure = state
            .profiles
            .get(harness)
            .is_some_and(|profile| profile.storage == "secure");
        let previous_secret = if owned_secure {
            self.vault.read(harness).await?
        } else {
            None
        };
        if owned_secure {
            self.vault.delete(harness).await?;
        }
        let mut profiles = state.profiles.clone();
        profiles.remove(harness);
        let mut blocked = state.blocked.clone();
        blocked.insert(harness.into());
        if let Err(error) = self.persist(&profiles, &blocked) {
            if let Some(secret) = previous_secret {
                let _ = self.vault.write(harness, secret).await;
            }
            return Err(error);
        }
        self.checks.lock().await.remove(harness);
        state.profiles = profiles;
        state.blocked = blocked;
        state.session_keys.remove(harness);
        drop(state);
        self.descriptor(harness).await
    }
    async fn execution(&self, harness: &str) -> Result<AgentToolExecutionOverlay> {
        if !supported(harness) {
            return Ok(AgentToolExecutionOverlay::default());
        }
        providers::descriptor(harness)?;
        let state = self.state.lock().await;
        let Some(profile) = state.profiles.get(harness) else {
            if state.blocked.contains(harness) {
                return Err(Error::Invalid("This agent tool is disconnected; connect an account in Agent tools settings before starting a session".into()));
            }
            return Ok(self.managed_overlay(harness, AgentToolExecutionOverlay::default()));
        };
        if profile.storage == "native" {
            return Ok(self.managed_overlay(
                harness,
                AgentToolExecutionOverlay {
                    env: BTreeMap::new(),
                    config_home: profile.config_home.clone(),
                    config: profile.config.clone(),
                },
            ));
        }
        let credential = self
            .credential(harness, profile, state.session_keys.get(harness))
            .await?;
        providers::credential_overlay(
            harness,
            &profile.provider,
            &credential.api_key,
            &self.root.join(harness),
        )
        .map(|overlay| self.managed_overlay(harness, overlay))
    }
    async fn history_overlay(&self, harness: &str) -> Result<AgentToolExecutionOverlay> {
        let state = self.state.lock().await;
        let profile = state.profiles.get(harness);
        let overlay = if let Some(profile) = profile.filter(|profile| profile.storage != "native") {
            providers::api_history_overlay(harness, &profile.provider, &self.root.join(harness))
        } else {
            AgentToolExecutionOverlay {
                config_home: profile.and_then(|profile| profile.config_home.clone()),
                config: profile.map_or_else(|| json!({}), |profile| profile.config.clone()),
                ..Default::default()
            }
        };
        Ok(self.managed_overlay(harness, overlay))
    }
    async fn shutdown(&self) -> Result<()> {
        let installations = {
            let mut state = self.state.lock().await;
            for flow in state.flows.values() {
                if let Some(cancel) = &flow.install_cancel {
                    cancel.store(true, Ordering::SeqCst);
                }
            }
            std::mem::take(&mut state.installations)
        };
        for (_, mut task) in installations {
            if tokio::time::timeout(std::time::Duration::from_secs(5), &mut task)
                .await
                .is_err()
            {
                task.abort();
                let _ = task.await;
            }
        }
        self.native.shutdown().await?;
        self.state.lock().await.session_keys.clear();
        self.checks.lock().await.clear();
        Ok(())
    }
    async fn install(&self, harness: &str) -> Result<Value> {
        if !installation::supported(harness) {
            return Err(Error::Unsupported(
                "Managed installation for this agent tool".into(),
            ));
        }
        let mut state = self.state.lock().await;
        if state
            .flows
            .values()
            .any(|flow| flow.harness == harness && flow.value["status"] == "pending")
        {
            return Err(Error::Busy);
        }
        let id = uuid::Uuid::new_v4();
        let cancel = Arc::new(AtomicBool::new(false));
        let profile = Profile {
            provider: String::new(),
            method: String::new(),
            storage: "native".into(),
            verified: false,
            config_home: None,
            config: Value::Null,
        };
        let value = self.add_flow(
            &mut state,
            id,
            harness.into(),
            profile,
            json!({"status":"pending","operation":"install","message":"Preparing provider installation"}),
        ).await?;
        state.flows.get_mut(&id).unwrap().install_cancel = Some(cancel.clone());
        let shared = self.state.clone();
        let root = self.root.clone();
        let harness = harness.to_owned();
        let task = tokio::spawn(async move {
            let (progress, mut changes) = tokio::sync::mpsc::channel::<Value>(16);
            let updates = shared.clone();
            let publisher = tokio::spawn(async move {
                while let Some(change) = changes.recv().await {
                    let mut state = updates.lock().await;
                    if let Some(flow) = state.flows.get_mut(&id)
                        && flow.value["status"] == "pending"
                    {
                        if let Some(message) = change["message"].as_str() {
                            flow.value["message"] = json!(message);
                        }
                        if let Some(progress) = change.get("progress") {
                            flow.value["progress"] = progress.clone();
                        }
                    }
                }
            });
            let outcome = installation::install(&root, &harness, cancel.clone(), progress).await;
            let _ = publisher.await;
            let status = if outcome.is_ok() {
                "completed"
            } else if cancel.load(Ordering::SeqCst) {
                "cancelled"
            } else {
                "failed"
            };
            let message = match outcome {
                Ok(_) => "Provider installation completed".into(),
                Err(error) => error.to_string(),
            };
            if let Some(flow) = shared.lock().await.flows.get_mut(&id) {
                flow.value =
                    json!({"id":id,"status":status,"operation":"install","message":message});
            }
        });
        state.installations.insert(id, task);
        Ok(value)
    }
}

impl AgentToolSetup {
    fn managed_overlay(
        &self,
        harness: &str,
        mut overlay: AgentToolExecutionOverlay,
    ) -> AgentToolExecutionOverlay {
        let managed = installation::managed_overlay(&self.root, harness);
        if let Some(config) = managed.config.as_object() {
            if !overlay.config.is_object() {
                overlay.config = json!({});
            }
            for (key, value) in config {
                overlay.config[key] = value.clone();
            }
        }
        overlay
    }
}

fn private_directory(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
fn supported(harness: &str) -> bool {
    matches!(
        harness,
        "codex" | "claude" | "cursor" | "grok" | "pi" | "opencode" | "antigravity"
    )
}
fn field<'a>(value: &'a Value, name: &str) -> Result<&'a str> {
    value[name]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::Invalid(format!("Missing {name} in agent tool connection request")))
}
fn flow_id(id: &str) -> Result<uuid::Uuid> {
    uuid::Uuid::parse_str(id)
        .map_err(|_| Error::Invalid("Invalid agent tool connection flow ID".into()))
}
fn public_flow(id: uuid::Uuid, value: Value) -> Result<Value> {
    let status = value["status"]
        .as_str()
        .filter(|status| {
            matches!(
                *status,
                "pending" | "connected" | "failed" | "cancelled" | "completed"
            )
        })
        .ok_or_else(|| {
            Error::Provider("The native provider returned an invalid connection flow status".into())
        })?;
    let mut result = json!({"id":id,"status":status});
    for name in [
        "message",
        "authorizationUrl",
        "userCode",
        "expiresAt",
        "inputFields",
        "operation",
        "progress",
    ] {
        if let Some(value) = value.get(name).filter(|value| !value.is_null()) {
            result[name] = value.clone();
        }
    }
    Ok(result)
}
