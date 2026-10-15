use adapters::setup::{
    AgentToolSetup, CredentialValidation, NativeReadiness, NativeSetup, providers,
    vault::SecretVault,
};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use swarmie::{AgentToolExecutionOverlay, AgentToolSetupPort, Error, Result};
use tokio::sync::Mutex;

#[derive(Default)]
struct MemoryVault {
    values: Mutex<BTreeMap<String, String>>,
    unavailable: AtomicBool,
    writes: AtomicUsize,
}
#[async_trait]
impl SecretVault for MemoryVault {
    async fn read(&self, tool: &str) -> Result<Option<String>> {
        if self.unavailable.load(Ordering::SeqCst) {
            return Err(Error::Provider("Vault unavailable".into()));
        }
        Ok(self.values.lock().await.get(tool).cloned())
    }
    async fn write(&self, tool: &str, value: String) -> Result<()> {
        if self.unavailable.load(Ordering::SeqCst) {
            return Err(Error::Provider("Vault unavailable".into()));
        }
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.values.lock().await.insert(tool.into(), value);
        Ok(())
    }
    async fn delete(&self, tool: &str) -> Result<()> {
        if self.unavailable.load(Ordering::SeqCst) {
            return Err(Error::Provider("Vault unavailable".into()));
        }
        self.values.lock().await.remove(tool);
        Ok(())
    }
}
#[derive(Default)]
struct FakeNative {
    flows: Mutex<BTreeMap<uuid::Uuid, Value>>,
    minted: Mutex<Option<String>>,
    authenticated: AtomicBool,
    auth_state: Mutex<Option<String>>,
    installed: AtomicBool,
    probe_gate: Mutex<Option<Arc<tokio::sync::Notify>>>,
    probe_entered: tokio::sync::Notify,
    validations: AtomicUsize,
    releases: AtomicUsize,
}
#[async_trait]
impl NativeSetup for FakeNative {
    async fn readiness(&self, _: &str) -> Result<NativeReadiness> {
        let gate = self.probe_gate.lock().await.clone();
        if let Some(gate) = gate {
            self.probe_entered.notify_one();
            gate.notified().await;
        }
        Ok(NativeReadiness {
            installed: self.installed.load(Ordering::SeqCst),
            auth_state: self.auth_state.lock().await.clone().unwrap_or_else(|| {
                if self.authenticated.load(Ordering::SeqCst) {
                    "authenticated"
                } else {
                    "unauthenticated"
                }
                .into()
            }),
            message: None,
        })
    }
    async fn validate_key(&self, _: &str, _: &str, key: &str) -> Result<CredentialValidation> {
        self.validations.fetch_add(1, Ordering::SeqCst);
        if key == "throw-secret" {
            return Err(Error::Provider(format!("Failed credential {key}")));
        }
        Ok(CredentialValidation {
            auth_state: match key {
                "bad-key" => "unauthenticated",
                "unverified-key" => "unknown",
                _ => "authenticated",
            }
            .into(),
            message: None,
        })
    }
    async fn start_login(
        &self,
        id: uuid::Uuid,
        _: &str,
        _: &str,
        _: &str,
        _: &Path,
    ) -> Result<Value> {
        let value = json!({"id":id,"status":"pending","authorizationUrl":"https://provider.example/login","inputFields":[{"id":"code","label":"Code","secret":true}],"tokens":{"accessToken":"hidden-native-secret"}});
        self.flows.lock().await.insert(id, value.clone());
        Ok(value)
    }
    async fn flow(&self, id: uuid::Uuid) -> Result<Value> {
        self.flows
            .lock()
            .await
            .get(&id)
            .cloned()
            .ok_or(Error::Closed)
    }
    async fn input(&self, id: uuid::Uuid, _: Value) -> Result<Value> {
        self.authenticated.store(true, Ordering::SeqCst);
        let value = json!({"id":id,"status":"connected","accessToken":"hidden-native-secret"});
        self.flows.lock().await.insert(id, value.clone());
        Ok(value)
    }
    async fn cancel(&self, id: uuid::Uuid) -> Result<Value> {
        let value = json!({"id":id,"status":"cancelled"});
        self.flows.lock().await.insert(id, value.clone());
        Ok(value)
    }
    async fn release(&self, id: uuid::Uuid) -> Result<()> {
        self.releases.fetch_add(1, Ordering::SeqCst);
        self.flows.lock().await.remove(&id);
        Ok(())
    }
    async fn login_credential(&self, _: uuid::Uuid) -> Result<Option<String>> {
        Ok(self.minted.lock().await.clone())
    }
    async fn login_overlay(&self, _: uuid::Uuid) -> Result<Option<AgentToolExecutionOverlay>> {
        Ok(None)
    }
    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}
struct Fixture {
    root: PathBuf,
    owner: uuid::Uuid,
    vault: Arc<MemoryVault>,
    native: Arc<FakeNative>,
}
impl Fixture {
    fn new() -> Self {
        let value = Self {
            root: std::env::temp_dir()
                .join(format!("swarmie-agent-tool-setup-{}", uuid::Uuid::new_v4())),
            owner: uuid::Uuid::new_v4(),
            vault: Arc::new(MemoryVault::default()),
            native: Arc::new(FakeNative::default()),
        };
        value.native.installed.store(true, Ordering::SeqCst);
        value
    }
    fn service(&self) -> AgentToolSetup {
        AgentToolSetup::new(
            self.owner,
            &self.root,
            self.native.clone(),
            self.vault.clone(),
        )
        .unwrap()
    }
    fn metadata(&self) -> String {
        std::fs::read_to_string(self.root.join("agent-tool-connections/connections.json")).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
#[tokio::test]
async fn readonly_history_uses_private_profile_paths_without_vault_access_or_reconnection() {
    let fixture = Fixture::new();
    let service = fixture.service();
    service
        .start(request("api_key", "fixture-private-key", "secure"))
        .await
        .unwrap();
    let execution = service.execution("claude").await.unwrap();
    fixture.vault.unavailable.store(true, Ordering::SeqCst);
    let history = service.history_overlay("claude").await.unwrap();
    assert!(history.env.is_empty());
    assert_eq!(history.config_home, execution.config_home);
    assert!(!history.config.to_string().contains("fixture-private-key"));
    fixture.vault.unavailable.store(false, Ordering::SeqCst);
    service.disconnect("claude").await.unwrap();
    assert!(
        service
            .history_overlay("claude")
            .await
            .unwrap()
            .env
            .is_empty()
    );
    assert!(service.execution("claude").await.is_err());
}
fn request(kind: &str, key: &str, storage: &str) -> Value {
    let descriptor = providers::descriptor("claude").unwrap();
    for provider in descriptor["providers"].as_array().unwrap() {
        if let Some(method) = provider["methods"]
            .as_array()
            .unwrap()
            .iter()
            .find(|method| method["kind"] == kind)
        {
            return json!({"harness":"claude","provider":provider["id"],"method":method["id"],"values":{"apiKey":key},"storage":storage});
        }
    }
    panic!("missing fixture connection method {kind}")
}

#[tokio::test]
async fn secure_credentials_survive_restart_only_in_vault_and_never_cross_public_status() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let flow = service
        .start(request("api_key", "private-provider-key", "secure"))
        .await
        .unwrap();
    assert_eq!(flow["status"], "connected");
    let descriptor = service.descriptor("claude").await.unwrap();
    assert_eq!(descriptor["authState"], "authenticated");
    assert_eq!(descriptor["connection"]["storage"], "secure");
    assert!(!descriptor.to_string().contains("private-provider-key"));
    assert!(!fixture.metadata().contains("private-provider-key"));
    assert!(
        service
            .execution("claude")
            .await
            .unwrap()
            .env
            .values()
            .any(|value| value.contains("private-provider-key"))
    );
    drop(service);
    assert!(
        fixture
            .service()
            .execution("claude")
            .await
            .unwrap()
            .env
            .values()
            .any(|value| value.contains("private-provider-key"))
    );
}

#[tokio::test]
async fn session_only_storage_requires_explicit_choice_and_does_not_reappear_after_restart() {
    let fixture = Fixture::new();
    fixture.vault.unavailable.store(true, Ordering::SeqCst);
    let service = fixture.service();
    assert!(
        service
            .start(request("api_key", "ephemeral-provider-key", "secure"))
            .await
            .is_err()
    );
    assert_eq!(
        service
            .start(request("api_key", "ephemeral-provider-key", "session"))
            .await
            .unwrap()["status"],
        "connected"
    );
    assert_eq!(fixture.vault.writes.load(Ordering::SeqCst), 0);
    assert!(!fixture.metadata().contains("ephemeral-provider-key"));
    assert_eq!(
        service.descriptor("claude").await.unwrap()["storage"]["persistent"],
        false
    );
    assert!(!service.execution("claude").await.unwrap().env.is_empty());
    drop(service);
    fixture.native.authenticated.store(true, Ordering::SeqCst);
    assert!(fixture.service().execution("claude").await.is_err());
    assert_eq!(
        fixture.service().descriptor("claude").await.unwrap()["authState"],
        "unauthenticated"
    );
}

#[tokio::test]
async fn rejected_credentials_and_verification_failures_do_not_change_connection_or_echo_secrets() {
    let fixture = Fixture::new();
    let service = fixture.service();
    assert!(
        service
            .start(request("api_key", "bad-key", "secure"))
            .await
            .is_err()
    );
    let error = service
        .start(request("api_key", "throw-secret", "secure"))
        .await
        .unwrap_err();
    assert!(!error.to_string().contains("throw-secret"));
    assert_eq!(fixture.vault.writes.load(Ordering::SeqCst), 0);
    let flow = service
        .start(request("api_key", "unverified-key", "session"))
        .await
        .unwrap();
    assert_eq!(flow["status"], "connected");
    let descriptor = service.descriptor("claude").await.unwrap();
    assert_eq!(descriptor["authState"], "unknown");
    assert_eq!(descriptor["connection"]["verified"], false);
}

#[tokio::test]
async fn disconnect_removes_owned_credential_and_requires_explicit_native_reconnection() {
    let fixture = Fixture::new();
    let service = fixture.service();
    fixture.native.authenticated.store(true, Ordering::SeqCst);
    service
        .start(request("api_key", "owned-provider-key", "secure"))
        .await
        .unwrap();
    service.disconnect("claude").await.unwrap();
    assert!(fixture.vault.values.lock().await.is_empty());
    assert!(service.execution("claude").await.is_err());
    assert_eq!(
        service.descriptor("claude").await.unwrap()["authState"],
        "unauthenticated"
    );
    service
        .start(request("existing", "", "session"))
        .await
        .unwrap();
    assert!(service.execution("claude").await.unwrap().env.is_empty());
    assert_eq!(
        service.descriptor("claude").await.unwrap()["connection"]["storage"],
        "native"
    );
}

#[tokio::test]
async fn login_progress_is_whitelisted_correlated_and_cancellable() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let flow = service
        .start(request("browser", "", "session"))
        .await
        .unwrap();
    let id = flow["id"].as_str().unwrap();
    assert_eq!(flow["status"], "pending");
    assert!(flow.get("tokens").is_none());
    assert!(
        service
            .start(request("browser", "", "session"))
            .await
            .is_err()
    );
    assert_eq!(service.cancel(id).await.unwrap()["status"], "cancelled");
    assert_eq!(service.flow(id).await.unwrap()["status"], "cancelled");
    assert!(
        service
            .input(id, json!({"code":"private-code"}))
            .await
            .is_err()
    );
    assert!(
        service
            .flow(&uuid::Uuid::new_v4().to_string())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn browser_minted_credentials_use_explicit_secure_storage_and_never_public_flow_payloads() {
    let fixture = Fixture::new();
    *fixture.native.minted.lock().await = Some("minted-browser-key".into());
    let service = fixture.service();
    let flow = service
        .start(request("browser", "", "secure"))
        .await
        .unwrap();
    let connected = service
        .input(flow["id"].as_str().unwrap(), json!({"code":"private-code"}))
        .await
        .unwrap();
    assert_eq!(connected["status"], "connected");
    assert!(connected.get("accessToken").is_none());
    assert!(!fixture.metadata().contains("minted-browser-key"));
    assert!(
        service
            .execution("claude")
            .await
            .unwrap()
            .env
            .values()
            .any(|value| value.contains("minted-browser-key"))
    );
}

#[tokio::test]
async fn locked_secure_vault_never_falls_back_to_a_different_native_account() {
    let fixture = Fixture::new();
    let service = fixture.service();
    service
        .start(request("api_key", "saved-key", "secure"))
        .await
        .unwrap();
    fixture.vault.unavailable.store(true, Ordering::SeqCst);
    fixture.native.authenticated.store(true, Ordering::SeqCst);
    assert!(service.execution("claude").await.is_err());
    assert_eq!(
        service.descriptor("claude").await.unwrap()["authState"],
        "unauthenticated"
    );
}

#[tokio::test]
async fn profile_owner_is_retained_and_generic_acp_tools_keep_external_authentication() {
    let fixture = Fixture::new();
    let service = fixture.service();
    service.disconnect("claude").await.unwrap();
    assert!(
        AgentToolSetup::new(
            uuid::Uuid::new_v4(),
            &fixture.root,
            fixture.native.clone(),
            fixture.vault.clone()
        )
        .is_err()
    );
    let descriptor = service.descriptor("custom-acp").await.unwrap();
    assert_eq!(descriptor["manual"], true);
    assert!(descriptor["providers"].as_array().unwrap().is_empty());
    assert!(
        service
            .execution("custom-acp")
            .await
            .unwrap()
            .env
            .is_empty()
    );
}

#[tokio::test]
async fn slow_readiness_does_not_block_cancellation_or_other_coordinator_mutations() {
    let fixture = Fixture::new();
    let service = Arc::new(fixture.service());
    let flow = service
        .start(request("browser", "", "session"))
        .await
        .unwrap();
    let gate = Arc::new(tokio::sync::Notify::new());
    *fixture.native.probe_gate.lock().await = Some(gate.clone());
    let reader = service.clone();
    let reading = tokio::spawn(async move { reader.descriptor("claude").await });
    fixture.native.probe_entered.notified().await;
    assert_eq!(
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            service.cancel(flow["id"].as_str().unwrap())
        )
        .await
        .unwrap()
        .unwrap()["status"],
        "cancelled"
    );
    gate.notify_one();
    reading.await.unwrap().unwrap();
}

#[tokio::test]
async fn repeated_status_reads_reuse_short_verification_cache_and_replacement_rechecks() {
    let fixture = Fixture::new();
    let service = fixture.service();
    service
        .start(request("api_key", "verified-one", "session"))
        .await
        .unwrap();
    service.descriptor("claude").await.unwrap();
    service.descriptor("claude").await.unwrap();
    assert_eq!(fixture.native.validations.load(Ordering::SeqCst), 2);
    service
        .start(request("api_key", "verified-two", "session"))
        .await
        .unwrap();
    service.descriptor("claude").await.unwrap();
    assert_eq!(fixture.native.validations.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn failed_metadata_publication_restores_previous_vault_and_active_connection() {
    let fixture = Fixture::new();
    let service = fixture.service();
    service
        .start(request("api_key", "retained-key", "secure"))
        .await
        .unwrap();
    let original = fixture.vault.values.lock().await["claude"].clone();
    let metadata = fixture.root.join("agent-tool-connections/connections.json");
    let backup = metadata.with_file_name("connections-backup.json");
    std::fs::rename(&metadata, &backup).unwrap();
    std::fs::create_dir(&metadata).unwrap();
    assert!(
        service
            .start(request("api_key", "replacement-key", "secure"))
            .await
            .is_err()
    );
    assert_eq!(fixture.vault.values.lock().await["claude"], original);
    assert!(
        service
            .execution("claude")
            .await
            .unwrap()
            .env
            .values()
            .any(|value| value.contains("retained-key"))
    );
    std::fs::remove_dir(&metadata).unwrap();
    std::fs::rename(backup, metadata).unwrap();
}

#[tokio::test]
async fn successful_native_commit_releases_private_login_state_and_replays_public_receipt() {
    let fixture = Fixture::new();
    *fixture.native.minted.lock().await = Some("managed-browser-key".into());
    let service = fixture.service();
    let flow = service
        .start(request("browser", "", "secure"))
        .await
        .unwrap();
    let id = flow["id"].as_str().unwrap();
    let connected = service
        .input(id, json!({"code":"private-code"}))
        .await
        .unwrap();
    assert_eq!(fixture.native.releases.load(Ordering::SeqCst), 1);
    assert!(fixture.native.flows.lock().await.is_empty());
    assert_eq!(service.flow(id).await.unwrap(), connected);
    assert_eq!(fixture.native.releases.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn completed_native_login_can_retry_secure_commit_after_vault_unlock() {
    let fixture = Fixture::new();
    *fixture.native.minted.lock().await = Some("retry-browser-key".into());
    let service = fixture.service();
    let flow = service
        .start(request("browser", "", "secure"))
        .await
        .unwrap();
    let id = flow["id"].as_str().unwrap();
    fixture.vault.unavailable.store(true, Ordering::SeqCst);
    assert!(
        service
            .input(id, json!({"code":"private-code"}))
            .await
            .is_err()
    );
    assert_eq!(fixture.native.releases.load(Ordering::SeqCst), 0);
    fixture.vault.unavailable.store(false, Ordering::SeqCst);
    assert_eq!(service.flow(id).await.unwrap()["status"], "connected");
    assert_eq!(fixture.native.releases.load(Ordering::SeqCst), 1);
    assert!(
        service
            .execution("claude")
            .await
            .unwrap()
            .env
            .values()
            .any(|value| value.contains("retry-browser-key"))
    );
}

#[tokio::test]
async fn existing_installed_account_with_unknown_authentication_is_explicitly_configured_unverified()
 {
    let fixture = Fixture::new();
    *fixture.native.auth_state.lock().await = Some("unknown".into());
    let service = fixture.service();
    service.disconnect("pi").await.unwrap();
    let flow = service.start(json!({"harness":"pi","provider":"anthropic","method":"existing","values":{},"storage":"session"})).await.unwrap();
    assert_eq!(flow["status"], "connected");
    assert!(
        flow["message"]
            .as_str()
            .unwrap()
            .contains("not been confirmed")
    );
    let descriptor = service.descriptor("pi").await.unwrap();
    assert_eq!(descriptor["authState"], "unknown");
    assert_eq!(descriptor["connection"]["verified"], false);
    let overlay = service.execution("pi").await.unwrap();
    assert_eq!(overlay.config["provider"], "anthropic");
    assert!(overlay.env.is_empty());
    assert_eq!(
        fixture.service().execution("pi").await.unwrap().config["provider"],
        "anthropic"
    );
    service.start(json!({"harness":"opencode","provider":"openai","method":"existing","values":{},"storage":"secure"})).await.unwrap();
    assert_eq!(
        service.execution("opencode").await.unwrap().config["provider"],
        "openai"
    );
}

#[tokio::test]
async fn existing_reuse_rejects_known_signed_out_and_missing_agent_tools() {
    let fixture = Fixture::new();
    let service = fixture.service();
    assert!(
        service
            .start(request("existing", "", "session"))
            .await
            .is_err()
    );
    fixture.native.authenticated.store(true, Ordering::SeqCst);
    fixture.native.installed.store(false, Ordering::SeqCst);
    assert!(
        service
            .start(request("existing", "", "session"))
            .await
            .is_err()
    );
    assert!(fixture.vault.values.lock().await.is_empty());
}

#[tokio::test]
async fn disconnect_returns_full_descriptor_and_failed_metadata_restores_secure_credential() {
    let fixture = Fixture::new();
    let service = fixture.service();
    service
        .start(request("api_key", "retained-disconnect-key", "secure"))
        .await
        .unwrap();
    let original = fixture.vault.values.lock().await["claude"].clone();
    let metadata = fixture.root.join("agent-tool-connections/connections.json");
    let backup = metadata.with_file_name("connections-backup.json");
    std::fs::rename(&metadata, &backup).unwrap();
    std::fs::create_dir(&metadata).unwrap();
    assert!(service.disconnect("claude").await.is_err());
    assert_eq!(fixture.vault.values.lock().await["claude"], original);
    assert!(
        service
            .execution("claude")
            .await
            .unwrap()
            .env
            .values()
            .any(|value| value.contains("retained-disconnect-key"))
    );
    std::fs::remove_dir(&metadata).unwrap();
    std::fs::rename(backup, metadata).unwrap();
    let descriptor = service.disconnect("claude").await.unwrap();
    assert_eq!(descriptor["harness"], "claude");
    assert_eq!(descriptor["installed"], true);
    assert_eq!(descriptor["authState"], "unauthenticated");
    assert!(descriptor["providers"].is_array());
    assert!(descriptor["storage"].is_object());
    assert!(descriptor.get("connection").is_none());
}
