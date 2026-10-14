use super::{CredentialValidation, NativeReadiness, NativeSetup, providers};
use crate::*;
use async_trait::async_trait;
use process::ManagedProcess;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use swarmie::{AgentToolExecutionOverlay, Error, Result};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::{Mutex, RwLock},
};
use uuid::Uuid;

pub struct NativeAgentTools {
    flows: Arc<RwLock<BTreeMap<Uuid, Arc<Login>>>>,
    opencode: super::opencode_auth::OpencodeAuth,
}
impl Default for NativeAgentTools {
    fn default() -> Self {
        Self::new()
    }
}
impl NativeAgentTools {
    pub fn new() -> Self {
        Self {
            flows: Arc::new(RwLock::new(BTreeMap::new())),
            opencode: super::opencode_auth::OpencodeAuth::new(),
        }
    }
}
impl Drop for NativeAgentTools {
    fn drop(&mut self) {
        let flows = self.flows.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let entries = std::mem::take(&mut *flows.write().await);
                for login in entries.into_values() {
                    let _public = login.public.write().await;
                    login.cancelled.store(true, Ordering::SeqCst);
                    *login.credential.lock().await = None;
                    let _ = login.process.kill().await;
                }
            });
        }
    }
}
struct Login {
    public: RwLock<Value>,
    process: Arc<ManagedProcess>,
    stdin: Mutex<Box<dyn tokio::io::AsyncWrite + Send + Unpin>>,
    cancelled: AtomicBool,
    sdk: bool,
    native_google: bool,
    selected_google: AtomicBool,
    google_menu_ready: AtomicBool,
    credential: Mutex<Option<String>>,
    overlay: Mutex<AgentToolExecutionOverlay>,
}
fn executable(harness: &str, overlay: Option<&AgentToolExecutionOverlay>) -> Option<PathBuf> {
    if let Some(path) = overlay.and_then(|overlay| overlay.config["executable"].as_str()) {
        return process::find_executable(path);
    }
    if harness == "cursor" {
        return crate::cursor::bridge_executable();
    }
    let default = match harness {
        "antigravity" => "agy",
        "claude" => "claude",
        "codex" => "codex",
        "grok" => "grok",
        "pi" => "pi",
        "opencode" => "opencode",
        "gemini" => "gemini",
        "qwen" => "qwen",
        "hermes" => "hermes",
        "kiro" => "kiro-cli",
        _ => return None,
    };
    crate::configured_executable(harness)
        .and_then(|path| process::find_executable(&path.to_string_lossy()))
        .or_else(|| process::find_executable(default))
}
fn scope_environment(harness: &str, home: Option<&Path>) -> BTreeMap<String, String> {
    let Some(home) = home else {
        return BTreeMap::new();
    };
    let variable = match harness {
        "codex" => "CODEX_HOME",
        "claude" => "CLAUDE_CONFIG_DIR",
        "grok" => "GROK_HOME",
        "pi" => "PI_CODING_AGENT_DIR",
        "opencode" => "OPENCODE_CONFIG_DIR",
        "antigravity" => "HOME",
        _ => return BTreeMap::new(),
    };
    let mut env = BTreeMap::from([(variable.into(), home.to_string_lossy().into_owned())]);
    if harness == "grok" {
        env.insert(
            "GROK_AUTH_PATH".into(),
            home.join("auth.json").to_string_lossy().into_owned(),
        );
    }
    if harness == "antigravity" && cfg!(windows) {
        env.insert("USERPROFILE".into(), home.to_string_lossy().into_owned());
    }
    env
}
async fn status(
    harness: &str,
    overlay: Option<&AgentToolExecutionOverlay>,
) -> Result<NativeReadiness> {
    if harness == "cursor" {
        let mut options = SpawnOptions::new("cursor", std::env::current_dir()?);
        if let Some(overlay) = overlay {
            options.config = overlay.config.clone();
            options.execution.env = overlay.env.clone();
            options.execution.config_home = overlay.config_home.clone();
        }
        let probe = crate::cursor::probe_options(&options).await?;
        return Ok(NativeReadiness {
            installed: probe.available,
            auth_state: probe.auth_state,
            message: probe.message,
        });
    }
    if harness == "claude"
        && overlay.is_some_and(|overlay| overlay.config["executable"].is_string())
    {
        let mut options = SpawnOptions::new("claude", std::env::current_dir()?);
        options.config = overlay.unwrap().config.clone();
        if crate::claude::sdk_readiness(&options).await.is_err() {
            return Ok(NativeReadiness {
                installed: false,
                auth_state: "unknown".into(),
                message: Some("Claude Agent SDK support or Node.js is unavailable on this runtime node. Install the agent tool support before connecting.".into()),
            });
        }
    }
    if overlay.is_none_or(|overlay| overlay.config["executable"].is_null()) {
        let detection = match harness {
            "codex" => CodexHarness.detect().await?,
            "claude" => ClaudeHarness.detect().await?,
            "pi" => PiHarness.detect().await?,
            "opencode" => OpenCodeHarness.detect().await?,
            "antigravity" => AntigravityHarness.detect().await?,
            _ => crate::detection(
                harness,
                if harness == "kiro" {
                    "kiro-cli"
                } else {
                    harness
                },
                Capabilities::default(),
            ),
        };
        if !detection.available {
            return Ok(NativeReadiness {
                installed: false,
                auth_state: "unknown".into(),
                message: Some(
                    "This agent tool's runtime dependencies are unavailable on this node.".into(),
                ),
            });
        }
    }
    let Some(exe) = executable(harness, overlay) else {
        return Ok(NativeReadiness {
            installed: false,
            auth_state: "unknown".into(),
            message: Some("Install this agent tool on the execution node first.".into()),
        });
    };
    let args: Vec<String> = match harness {
        "codex" => vec!["login", "status"],
        "claude" => vec!["auth", "status", "--json"],
        _ => vec!["--version"],
    }
    .into_iter()
    .map(String::from)
    .collect();
    let env = scope_environment(
        harness,
        overlay.and_then(|overlay| overlay.config_home.as_deref()),
    );
    let cwd = std::env::current_dir()?;
    let (process, _stdin, stdout, stderr) = match ManagedProcess::spawn_with_pipes(
        &exe,
        &args,
        &cwd,
        &env.into_iter().collect::<Vec<_>>(),
    )
    .await
    {
        Ok(value) => value,
        Err(_) => {
            return Ok(NativeReadiness {
                installed: false,
                auth_state: "unknown".into(),
                message: Some("The native readiness process could not start.".into()),
            });
        }
    };
    use tokio::io::AsyncReadExt;
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        let mut bytes = Vec::new();
        let mut diagnostics = Vec::new();
        let mut stdout = stdout.take(65537);
        let mut stderr = stderr.take(65537);
        let (output, error) = tokio::join!(
            stdout.read_to_end(&mut bytes),
            stderr.read_to_end(&mut diagnostics)
        );
        output?;
        error?;
        if bytes.len() > 65536 || diagnostics.len() > 65536 {
            return Err(std::io::Error::other("readiness output exceeds limit"));
        }
        Ok::<_, std::io::Error>((bytes, diagnostics))
    })
    .await;
    let _ = process.kill().await;
    let mut result_state = NativeReadiness {
        installed: true,
        auth_state: "unknown".into(),
        message: None,
    };
    if let Ok(Ok((bytes, diagnostics))) = result {
        if harness == "codex" {
            let text = format!(
                "{} {}",
                String::from_utf8_lossy(&bytes),
                String::from_utf8_lossy(&diagnostics)
            )
            .to_lowercase();
            result_state.auth_state = if text.contains("not logged in") {
                "unauthenticated"
            } else if text.contains("logged in") {
                "authenticated"
            } else {
                "unknown"
            }
            .into();
        } else if harness == "claude" {
            let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
            result_state.auth_state = match value["loggedIn"].as_bool() {
                Some(true) => "authenticated",
                Some(false) => "unauthenticated",
                None => "unknown",
            }
            .into();
        } else if bytes.is_empty() {
            result_state.installed = false;
            result_state.message = Some(
                "The installed executable could not report its version on this runtime node."
                    .into(),
            );
        }
    } else {
        result_state.installed = false;
        result_state.message =
            Some("The native readiness check timed out or exceeded its output budget.".into());
    }
    Ok(result_state)
}

#[async_trait]
impl NativeSetup for NativeAgentTools {
    async fn readiness(&self, harness: &str) -> Result<NativeReadiness> {
        status(harness, None).await
    }
    async fn readiness_at(&self, harness: &str, home: Option<&Path>) -> Result<NativeReadiness> {
        status(
            harness,
            Some(&AgentToolExecutionOverlay {
                config_home: home.map(Path::to_path_buf),
                ..Default::default()
            }),
        )
        .await
    }
    async fn readiness_with(
        &self,
        harness: &str,
        overlay: &AgentToolExecutionOverlay,
    ) -> Result<NativeReadiness> {
        status(harness, Some(overlay)).await
    }
    async fn validate_key(
        &self,
        harness: &str,
        provider: &str,
        key: &str,
    ) -> Result<CredentialValidation> {
        providers::validate_method(harness, provider, "api_key")?;
        validate_key(provider, key).await
    }
    async fn validate_key_with(
        &self,
        harness: &str,
        provider: &str,
        key: &str,
        overlay: &AgentToolExecutionOverlay,
    ) -> Result<CredentialValidation> {
        providers::validate_method(harness, provider, "api_key")?;
        if harness == "cursor" {
            cursor_key_check_with(key, overlay).await
        } else {
            validate_key(provider, key).await
        }
    }
    async fn start_login(
        &self,
        id: Uuid,
        harness: &str,
        provider: &str,
        method: &str,
        private_root: &Path,
    ) -> Result<Value> {
        providers::validate_method(harness, provider, method)?;
        if harness == "opencode" {
            return self.opencode.start(id, provider, private_root).await;
        }
        let sdk = harness == "cursor" || harness == "pi";
        let profile = private_root
            .join("native-profiles")
            .join(harness)
            .join(id.to_string());
        std::fs::create_dir_all(&profile)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&profile, std::fs::Permissions::from_mode(0o700))?;
        }
        let mut overlay = AgentToolExecutionOverlay {
            config_home: (!sdk || harness == "pi").then_some(profile.clone()),
            ..Default::default()
        };
        if harness == "pi" {
            overlay.config = json!({"provider":provider});
        }
        let managed = super::installation::managed_overlay(private_root, harness);
        if let Some(config) = managed.config.as_object() {
            for (key, value) in config {
                overlay.config[key] = value.clone();
            }
        }
        let (exe, args) = if sdk {
            let worker = auth_worker_with(&overlay.config).ok_or_else(|| {
                Error::Unsupported(
                    "Install the runtime's agent authentication support assets first".into(),
                )
            })?;
            let node = node_executable().ok_or_else(|| {
                Error::Unsupported(
                    "Native SDK authentication requires Node.js 22.13 or later".into(),
                )
            })?;
            verify_auth_node(&node).await?;
            (node, vec![worker.to_string_lossy().into_owned()])
        } else {
            let exe = executable(harness, Some(&overlay))
                .ok_or_else(|| Error::Provider("Agent tool executable is unavailable".into()))?;
            let args = match (harness, method) {
                ("codex", "device") => vec!["login", "--device-auth"],
                ("claude", "console") => vec!["auth", "login", "--console"],
                ("grok", "device") => vec!["login", "--device-auth"],
                ("antigravity", "browser") if cfg!(unix) => vec![],
                _ => {
                    return Err(Error::Unsupported(
                        "Native authentication requires a supported provider interface".into(),
                    ));
                }
            };
            (exe, args.into_iter().map(String::from).collect())
        };
        let mut env = scope_environment(harness, overlay.config_home.as_deref());
        env.insert("NO_OPEN_BROWSER".into(), "1".into());
        env.insert("BROWSER".into(), "false".into());
        let native_google = harness == "antigravity";
        #[cfg(unix)]
        if native_google {
            env.insert("TERM".into(), "xterm-256color".into());
            // The native documented SSH flow returns a URL and manual code.
            env.insert("SSH_CONNECTION".into(), "127.0.0.1 0 127.0.0.1 0".into());
        }
        type Input = Box<dyn tokio::io::AsyncWrite + Send + Unpin>;
        type Output = Box<dyn tokio::io::AsyncRead + Send + Unpin>;
        let (process, stdin, stdout, stderr): (ManagedProcess, Input, Output, Option<Output>) =
            if native_google {
                #[cfg(unix)]
                {
                    let (process, input, output) = ManagedProcess::spawn_with_terminal(
                        &exe,
                        &args,
                        &profile,
                        &env.into_iter().collect::<Vec<_>>(),
                    )
                    .await?;
                    (process, Box::new(input), Box::new(output), None)
                }
                #[cfg(not(unix))]
                return Err(Error::Unsupported(
                    "Native Google sign-in terminal on this platform".into(),
                ));
            } else {
                let (process, input, output, error) = ManagedProcess::spawn_with_pipes(
                    &exe,
                    &args,
                    &profile,
                    &env.into_iter().collect::<Vec<_>>(),
                )
                .await?;
                (
                    process,
                    Box::new(input),
                    Box::new(output),
                    Some(Box::new(error)),
                )
            };
        let login = Arc::new(Login {
            public: RwLock::new(
                json!({"id":id,"status":"pending","message":"Waiting for provider sign-in"}),
            ),
            process: Arc::new(process),
            stdin: Mutex::new(stdin),
            cancelled: AtomicBool::new(false),
            sdk,
            native_google,
            selected_google: AtomicBool::new(false),
            google_menu_ready: AtomicBool::new(false),
            credential: Mutex::new(None),
            overlay: Mutex::new(overlay),
        });
        self.flows.write().await.insert(id, login.clone());
        if sdk {
            login
                .stdin
                .lock()
                .await
                .write_all(
                    format!(
                        "{}\n",
                        json!({"operation":"login","harness":harness,"provider":provider})
                    )
                    .as_bytes(),
                )
                .await?;
        }
        spawn_reader(stdout, login.clone(), harness.into(), provider.into());
        if let Some(stderr) = stderr {
            spawn_reader(stderr, login.clone(), harness.into(), provider.into());
        }
        let monitor = login.clone();
        tokio::spawn(async move {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(300);
            loop {
                tokio::time::sleep(Duration::from_millis(100)).await;
                if monitor.cancelled.load(Ordering::SeqCst) {
                    return;
                }
                if monitor.public.read().await["status"] != "pending" {
                    let _ = monitor.process.kill().await;
                    return;
                }
                if monitor.process.exited() {
                    let mut public = monitor.public.write().await;
                    if public["status"] == "pending" {
                        public["status"] = json!("failed");
                        public["message"] =
                            json!("The provider sign-in process ended without a confirmed result");
                    }
                    return;
                }
                if tokio::time::Instant::now() >= deadline {
                    monitor.cancelled.store(true, Ordering::SeqCst);
                    let _ = monitor.process.kill().await;
                    let mut public = monitor.public.write().await;
                    public["status"] = json!("failed");
                    public["message"] = json!("Provider sign-in expired. Start again.");
                    return;
                }
            }
        });
        Ok(login.public.read().await.clone())
    }
    async fn flow(&self, id: Uuid) -> Result<Value> {
        if self.opencode.contains(id).await {
            return self.opencode.flow(id).await;
        }
        let login = self
            .flows
            .read()
            .await
            .get(&id)
            .cloned()
            .ok_or_else(|| Error::Invalid("Unknown authentication flow".into()))?;
        Ok(login.public.read().await.clone())
    }
    async fn input(&self, id: Uuid, values: Value) -> Result<Value> {
        if self.opencode.contains(id).await {
            return self.opencode.input(id, values).await;
        }
        let login = self
            .flows
            .read()
            .await
            .get(&id)
            .cloned()
            .ok_or_else(|| Error::Invalid("Unknown authentication flow".into()))?;
        let input = values["code"]
            .as_str()
            .or_else(|| values["value"].as_str())
            .ok_or_else(|| Error::Invalid("Authentication input requires a code".into()))?;
        if input.len() > 8192
            || input.contains(['\r', '\n', '\0'])
            || login.cancelled.load(Ordering::SeqCst)
            || login.public.read().await["status"] != "pending"
        {
            return Err(Error::Invalid(
                "Invalid or expired authentication input".into(),
            ));
        }
        let frame = if login.sdk {
            format!("{}\n", json!({"operation":"input","value":input}))
        } else if login.native_google {
            format!("{input}\r")
        } else {
            format!("{input}\n")
        };
        tokio::time::timeout(
            Duration::from_secs(5),
            login.stdin.lock().await.write_all(frame.as_bytes()),
        )
        .await
        .map_err(|_| Error::Timeout("Authentication input delivery".into()))??;
        Ok(login.public.read().await.clone())
    }
    async fn cancel(&self, id: Uuid) -> Result<Value> {
        if self.opencode.contains(id).await {
            return self.opencode.cancel(id).await;
        }
        let login = self
            .flows
            .read()
            .await
            .get(&id)
            .cloned()
            .ok_or_else(|| Error::Invalid("Unknown authentication flow".into()))?;
        let mut public = login.public.write().await;
        login.cancelled.store(true, Ordering::SeqCst);
        *login.credential.lock().await = None;
        let _ = login.process.kill().await;
        public["status"] = json!("cancelled");
        public["message"] = json!("Sign-in cancelled");
        clear_authorization(&mut public);
        Ok(public.clone())
    }
    async fn login_credential(&self, id: Uuid) -> Result<Option<String>> {
        if self.opencode.contains(id).await {
            return Ok(None);
        }
        let login = self
            .flows
            .read()
            .await
            .get(&id)
            .cloned()
            .ok_or_else(|| Error::Invalid("Unknown authentication flow".into()))?;
        Ok(login.credential.lock().await.clone())
    }
    async fn login_overlay(&self, id: Uuid) -> Result<Option<AgentToolExecutionOverlay>> {
        if self.opencode.contains(id).await {
            return self.opencode.login_overlay(id).await;
        }
        let login = self
            .flows
            .read()
            .await
            .get(&id)
            .cloned()
            .ok_or_else(|| Error::Invalid("Unknown authentication flow".into()))?;
        Ok(Some(login.overlay.lock().await.clone()))
    }
    async fn release(&self, id: Uuid) -> Result<()> {
        if self.opencode.contains(id).await {
            return self.opencode.release(id).await;
        }
        if let Some(login) = self.flows.write().await.remove(&id) {
            let _public = login.public.write().await;
            login.cancelled.store(true, Ordering::SeqCst);
            *login.credential.lock().await = None;
            let _ = login.process.kill().await;
        }
        Ok(())
    }
    async fn shutdown(&self) -> Result<()> {
        self.opencode.shutdown().await?;
        let ids: Vec<_> = self.flows.read().await.keys().copied().collect();
        for id in ids {
            let _ = self.cancel(id).await;
        }
        self.flows.write().await.clear();
        Ok(())
    }
}
pub(super) fn auth_worker() -> Option<PathBuf> {
    std::env::var_os("SWARMIE_AGENT_AUTH_WORKER")
        .map(PathBuf::from)
        .or_else(|| crate::agent_tools_root().map(|root| root.join("auth/worker.mjs")))
        .filter(|path| path.is_file())
        .or_else(|| {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../packages/agent-auth-worker/worker.mjs");
            path.is_file().then_some(path)
        })
}
fn auth_worker_with(config: &Value) -> Option<PathBuf> {
    if let Some(path) = config["authWorkerPath"].as_str() {
        return PathBuf::from(path).is_file().then(|| PathBuf::from(path));
    }
    auth_worker()
}
pub(super) fn sdk_auth_available_with(config: &Value) -> bool {
    auth_worker_with(config).is_some_and(|worker| {
        worker.parent().is_some_and(|parent| {
            parent
                .join("node_modules/@cursor/sdk/package.json")
                .is_file()
                && parent
                    .join("node_modules/@earendil-works/pi-ai/package.json")
                    .is_file()
        })
    }) && node_executable().is_some()
}
fn node_executable() -> Option<PathBuf> {
    std::env::var_os("SWARMIE_NODE_EXECUTABLE")
        .and_then(|path| process::find_executable(&path.to_string_lossy()))
        .or_else(|| process::find_executable("node"))
}
async fn verify_auth_node(node: &Path) -> Result<()> {
    use tokio::io::AsyncReadExt;
    let (process, _stdin, stdout, _stderr) = ManagedProcess::spawn_with_pipes(
        node,
        &["--version".into()],
        &std::env::current_dir()?,
        &[],
    )
    .await?;
    let mut stdout = stdout.take(129);
    let mut bytes = Vec::new();
    let result = tokio::time::timeout(Duration::from_secs(3), stdout.read_to_end(&mut bytes)).await;
    let _ = process.kill().await;
    if !matches!(result, Ok(Ok(_))) || bytes.len() > 128 {
        return Err(Error::Provider(
            "The authentication Node.js version check failed or timed out".into(),
        ));
    }
    let version = String::from_utf8_lossy(&bytes);
    let numbers: Vec<u32> = version
        .trim()
        .trim_start_matches('v')
        .split('.')
        .filter_map(|part| part.parse().ok())
        .collect();
    if numbers.len() < 2 || numbers[0] < 22 || numbers[0] == 22 && numbers[1] < 13 {
        return Err(Error::Unsupported(
            "Native SDK authentication requires Node.js 22.13 or later".into(),
        ));
    }
    Ok(())
}
async fn cursor_key_check(key: &str) -> Result<CredentialValidation> {
    cursor_key_check_with(key, &AgentToolExecutionOverlay::default()).await
}
async fn cursor_key_check_with(
    key: &str,
    overlay: &AgentToolExecutionOverlay,
) -> Result<CredentialValidation> {
    if key.is_empty() || key.len() > 8192 || key.contains(['\r', '\n', '\0']) {
        return Err(Error::Invalid("Invalid provider API key".into()));
    }
    let mut options = SpawnOptions::new("cursor", std::env::current_dir()?);
    options.config = overlay.config.clone();
    options
        .execution
        .env
        .insert("CURSOR_API_KEY".into(), key.into());
    Ok(match crate::cursor::verify_account(&options, key).await {
        Ok(()) => CredentialValidation { auth_state: "authenticated".into(), message: None },
        Err(_) => CredentialValidation { auth_state: "unknown".into(), message: Some("The API key is configured but Cursor account verification is unavailable on this runtime node.".into()) },
    })
}
async fn validate_key(provider: &str, key: &str) -> Result<CredentialValidation> {
    if key.is_empty() || key.len() > 8192 || key.contains(['\r', '\n', '\0']) {
        return Err(Error::Invalid("Invalid provider API key".into()));
    }
    if provider == "cursor" {
        return cursor_key_check(key).await;
    }
    let url = match provider {
        "openai" => "https://api.openai.com/v1/models",
        "anthropic" => "https://api.anthropic.com/v1/models",
        "google" => "https://generativelanguage.googleapis.com/v1beta/models",
        "xai" => "https://api.x.ai/v1/models",
        "openrouter" => "https://openrouter.ai/api/v1/auth/key",
        _ => {
            return Err(Error::Unsupported(
                "Provider API-key validation is unavailable".into(),
            ));
        }
    };
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| Error::Provider("Provider account check could not start".into()))?;
    let mut request = client.get(url);
    if provider == "anthropic" {
        request = request
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01");
    } else if provider == "google" {
        request = request.header("x-goog-api-key", key);
    } else {
        request = request.bearer_auth(key);
    }
    let response = request
        .send()
        .await
        .map_err(|_| Error::Provider("Provider account check failed or timed out".into()))?;
    let auth_state = if response.status().is_success() {
        "authenticated"
    } else if matches!(response.status().as_u16(), 401 | 403) {
        "unauthenticated"
    } else {
        "unknown"
    };
    Ok(CredentialValidation {
        auth_state: auth_state.into(),
        message: (auth_state != "authenticated").then_some(
            "The provider could not confirm this API key. Check the key and account access.".into(),
        ),
    })
}
fn spawn_reader<T: tokio::io::AsyncRead + Send + Unpin + 'static>(
    stream: T,
    login: Arc<Login>,
    harness: String,
    provider: String,
) {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stream);
        loop {
            let mut line = Vec::new();
            let result = async {
                loop {
                    let bytes = reader.fill_buf().await?;
                    if bytes.is_empty() {
                        return Ok::<_, std::io::Error>(!line.is_empty());
                    }
                    let count = bytes
                        .iter()
                        .position(|byte| *byte == b'\n')
                        .map(|index| index + 1)
                        .unwrap_or(bytes.len());
                    if line.len() + count > 64 * 1024 {
                        return Err(std::io::Error::other("Authentication output exceeds limit"));
                    }
                    let complete = bytes[count - 1] == b'\n';
                    line.extend_from_slice(&bytes[..count]);
                    reader.consume(count);
                    if !login.sdk && !complete {
                        cli_line(&login, &provider, &String::from_utf8_lossy(&line)).await;
                    }
                    if complete {
                        return Ok(true);
                    }
                }
            }
            .await;
            if result.is_err() {
                let mut public = login.public.write().await;
                if public["status"] == "pending" {
                    public["status"] = json!("failed");
                    public["message"] =
                        json!("The provider authentication output exceeded its limit");
                    clear_authorization(&mut public);
                }
                drop(public);
                let _ = login.process.kill().await;
                break;
            }
            if !matches!(result, Ok(true)) || login.cancelled.load(Ordering::SeqCst) {
                break;
            }
            let line = String::from_utf8_lossy(&line);
            if login.sdk {
                if let Ok(frame) = serde_json::from_str::<Value>(&line) {
                    sdk_frame(&login, &harness, &provider, frame).await;
                }
            } else {
                cli_line(&login, &provider, &line).await;
            }
        }
    });
}
async fn sdk_frame(login: &Login, harness: &str, provider: &str, frame: Value) {
    let mut public = login.public.write().await;
    if login.cancelled.load(Ordering::SeqCst) || public["status"] != "pending" {
        return;
    }
    match frame["type"].as_str() {
        Some("authorization") => {
            if let Some(url) = frame["url"]
                .as_str()
                .filter(|url| authorization_url(provider, url))
            {
                public["authorizationUrl"] = json!(url);
                if let Some(code) = frame["userCode"]
                    .as_str()
                    .filter(|code| valid_device_code(code))
                {
                    public["userCode"] = json!(code);
                }
            }
        }
        Some("input") => {
            public["inputFields"] = json!([{"id":"code","label":"Authorization code","secret":true,"placeholder":"Paste the code from the provider"}]);
        }
        Some("completed") => {
            if login.cancelled.load(Ordering::SeqCst) {
                return;
            }
            if harness == "cursor" {
                let Some(key) = frame["apiKey"].as_str().filter(|key| {
                    !key.is_empty() && key.len() <= 8192 && !key.contains(['\r', '\n', '\0'])
                }) else {
                    return;
                };
                *login.credential.lock().await = Some(key.into());
            } else if harness == "pi" {
                let credential = &frame["nativeCredential"];
                if credential["type"] != "oauth"
                    || !credential["access"].is_string()
                    || !credential["refresh"].is_string()
                {
                    return;
                }
                let home = login.overlay.lock().await.config_home.clone().unwrap();
                let path = home.join("auth.json");
                if write_native_credential(&path, &json!({provider:credential})).is_err() {
                    public["status"] = json!("failed");
                    public["message"] = json!(
                        "The native credential store could not be updated. Check this runtime's storage permissions and try again."
                    );
                    clear_authorization(&mut public);
                    return;
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
                }
            }
            public["status"] = json!("connected");
            public["message"] = json!("Provider sign-in completed");
            clear_authorization(&mut public);
            drop(public);
            let _ = login.process.kill().await;
        }
        Some("failed") | Some("cancelled") => {
            public["status"] = json!(if frame["type"] == "cancelled" {
                "cancelled"
            } else {
                "failed"
            });
            public["message"] = json!("Provider sign-in could not be completed");
            clear_authorization(&mut public);
        }
        _ => (),
    }
}
async fn cli_line(login: &Login, provider: &str, line: &str) {
    let mut public = login.public.write().await;
    if login.cancelled.load(Ordering::SeqCst) || public["status"] != "pending" {
        return;
    }
    if login.native_google {
        let clean = strip_terminal_sequences(line);
        let compact: String = clean.chars().filter(|char| !char.is_whitespace()).collect();
        if compact.contains("Selectloginmethod:") {
            login.google_menu_ready.store(true, Ordering::SeqCst);
        }
        if login.google_menu_ready.load(Ordering::SeqCst)
            && compact.contains(">1.GoogleOAuth")
            && !login.selected_google.swap(true, Ordering::SeqCst)
        {
            let _ = login.stdin.lock().await.write_all(b"\r").await;
        }
    }
    // Native terminal hyperlinks contain an unwrapped URL inside OSC8 metadata.
    if let Some(start) = line.find("https://") {
        let url = line[start..]
            .split(|char: char| char.is_whitespace() || char == '\u{1b}' || char == '\u{7}')
            .next()
            .unwrap_or_default();
        if authorization_url(provider, url) {
            public["authorizationUrl"] = json!(url);
            public["message"] = json!("Complete sign-in in your browser");
        }
    }
    let clean = strip_terminal_sequences(line);
    let line = clean.as_str();
    if let Some(start) = line.find("https://") {
        let url = line[start..]
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .trim_end_matches(['"', '\'', '.', ')']);
        if authorization_url(provider, url) {
            public["authorizationUrl"] = json!(url);
            public["message"] = json!("Complete sign-in in your browser");
        }
    }
    let lower = line.to_lowercase();
    if matches!(provider, "openai" | "xai")
        && public["authorizationUrl"].is_string()
        && let Some(code) = line.split_whitespace().find(|word| valid_device_code(word))
    {
        public["userCode"] = json!(code);
    }

    if lower.contains("successfully logged in")
        || lower.contains("login successful")
        || lower.contains("logged in successfully")
        || lower.contains("authentication successful")
        || lower.trim_start_matches("✓ ").starts_with("signed in")
    {
        public["status"] = json!("connected");
        public["message"] = json!("Native provider sign-in completed");
        clear_authorization(&mut public);
    }
    if lower.contains("authorization code") || lower.contains("paste") && lower.contains("code") {
        public["inputFields"] = json!([{"id":"code","label":"Authorization code","secret":true,"placeholder":"Paste the code from the provider"}]);
    }
}
fn strip_terminal_sequences(line: &str) -> String {
    let mut result = String::new();
    let mut chars = line.chars().peekable();
    while let Some(char) = chars.next() {
        if char == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for char in chars.by_ref() {
                    if ('@'..='~').contains(&char) {
                        break;
                    }
                }
            } else if chars.peek() == Some(&']') {
                chars.next();
                let mut escape = false;
                for char in chars.by_ref() {
                    if char == '\u{7}' || escape && char == '\\' {
                        break;
                    }
                    escape = char == '\u{1b}';
                }
            }
            continue;
        }
        if !char.is_control() || char == '\n' || char == '\t' {
            result.push(char);
        }
    }
    result
}
pub(super) fn authorization_url(provider: &str, url: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(url) else {
        return false;
    };
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.port().is_some_and(|port| port != 443)
    {
        return false;
    }
    let hosts: &[&str] = match provider {
        "openai" | "openai-codex" => &["auth.openai.com", "chatgpt.com"],
        "anthropic" => &["console.anthropic.com", "platform.claude.com"],
        "cursor" => &["cursor.com", "www.cursor.com"],
        "xai" => &["auth.x.ai", "accounts.x.ai", "console.x.ai", "x.ai"],
        "google" => &["accounts.google.com"],
        "github-copilot" => &["github.com"],
        _ => return false,
    };
    hosts.contains(&url.host_str().unwrap_or_default())
        && !url
            .query_pairs()
            .any(|(name, _)| ["access_token", "refresh_token", "api_key"].contains(&name.as_ref()))
}

pub(super) fn valid_device_code(code: &str) -> bool {
    code.len() >= 7
        && code.len() <= 16
        && code.contains('-')
        && code.split('-').all(|part| {
            (3..=8).contains(&part.len())
                && part
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        })
}
fn clear_authorization(public: &mut Value) {
    for name in ["authorizationUrl", "userCode", "inputFields"] {
        public.as_object_mut().unwrap().remove(name);
    }
}
fn write_native_credential(path: &Path, value: &Value) -> std::io::Result<()> {
    use std::io::Write;
    let invalid = || {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Invalid native credential store",
        )
    };
    let update = value
        .as_object()
        .filter(|entries| {
            !entries.is_empty()
                && entries
                    .values()
                    .all(|credential| credential["type"] == "oauth")
        })
        .ok_or_else(invalid)?;
    let mut stored = match std::fs::read(path) {
        Ok(bytes) => {
            if bytes.len() > 65536 {
                return Err(invalid());
            }
            let value: Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
            value
                .as_object()
                .filter(|entries| {
                    entries.values().all(|credential| {
                        matches!(credential["type"].as_str(), Some("oauth" | "api_key"))
                    })
                })
                .cloned()
                .ok_or_else(invalid)?
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => serde_json::Map::new(),
        Err(error) => return Err(error),
    };
    for (provider, credential) in update {
        stored.insert(provider.clone(), credential.clone());
    }
    let temporary = path.with_file_name(format!("auth-{}.tmp", Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&temporary)?;
        file.write_all(&serde_json::to_vec(&stored)?)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn managed_authentication_assets_are_scoped_to_the_requested_installation() {
        let root = std::env::temp_dir().join(format!("swarmie-auth-assets-{}", Uuid::new_v4()));
        let worker = root.join("worker.mjs");
        for package in ["@cursor/sdk", "@earendil-works/pi-ai"] {
            let directory = root.join("node_modules").join(package);
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join("package.json"), b"{}").unwrap();
        }
        std::fs::write(&worker, b"// Managed authentication fixture\n").unwrap();
        let config = json!({"authWorkerPath":worker});
        assert_eq!(auth_worker_with(&config), Some(worker.clone()));
        assert_eq!(
            sdk_auth_available_with(&config),
            node_executable().is_some()
        );
        std::fs::remove_file(worker).unwrap();
        assert_eq!(auth_worker_with(&config), None);
        assert!(!sdk_auth_available_with(&config));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn native_google_terminal_selects_observed_oauth_and_keeps_code_private() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("swarmie-google-terminal-{}", Uuid::new_v4()));
        let managed = root.join("managed/antigravity");
        let release = managed.join("releases/fixture");
        std::fs::create_dir_all(&release).unwrap();
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/native_auth.py");
        let quoted = format!("'{}'", fixture.to_string_lossy().replace('\'', "'\\''"));
        let executable = release.join("agy");
        std::fs::write(
            &executable,
            format!("#!/bin/sh\nexec python3 {quoted} google \"$@\"\n"),
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(
            managed.join("active.json"),
            br#"{"version":"1.3.5","executable":"releases/fixture/agy"}"#,
        )
        .unwrap();
        let native = NativeAgentTools::new();
        for cancel in [false, true] {
            let id = Uuid::new_v4();
            native
                .start_login(id, "antigravity", "google", "browser", &root)
                .await
                .unwrap();
            let flow = tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    let flow = native.flow(id).await.unwrap();
                    if flow["inputFields"].is_array() && flow["authorizationUrl"].is_string() {
                        break flow;
                    }
                    assert_eq!(flow["status"], "pending", "{flow}");
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            assert!(
                flow["authorizationUrl"]
                    .as_str()
                    .unwrap()
                    .ends_with(&"a".repeat(200))
            );
            if cancel {
                let receipt = native.cancel(id).await.unwrap();
                assert_eq!(receipt["status"], "cancelled");
                assert!(receipt["authorizationUrl"].is_null());
                assert!(receipt["inputFields"].is_null());
            } else {
                native
                    .input(id, json!({"code":"fixture-private-google-code"}))
                    .await
                    .unwrap();
                tokio::time::timeout(Duration::from_secs(3), async {
                    loop {
                        let flow = native.flow(id).await.unwrap();
                        assert!(!flow.to_string().contains("fixture-private-google-code"));
                        if flow["status"] == "connected" {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await
                .unwrap();
            }
            let login = native.flows.read().await[&id].clone();
            native.release(id).await.unwrap();
            assert!(login.process.exited());
        }
        native.shutdown().await.unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn codex_readiness_recognizes_native_status_written_to_stderr() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("swarmie-auth-status-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let executable = root.join("codex-status");
        let overlay = AgentToolExecutionOverlay {
            config: json!({"executable":executable}),
            ..Default::default()
        };
        for (banner, exit, expected) in [
            ("Logged in using ChatGPT", 0, "authenticated"),
            ("Not logged in", 1, "unauthenticated"),
        ] {
            std::fs::write(&executable, format!("#!/usr/bin/env python3\nimport sys\nprint({banner:?},file=sys.stderr)\nsys.exit({exit})\n")).unwrap();
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
            let readiness = status("codex", Some(&overlay)).await.unwrap();
            assert!(readiness.installed);
            assert_eq!(readiness.auth_state, expected);
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    async fn fixture(mode: &str, sdk: bool) -> (NativeAgentTools, Uuid, Arc<Login>, PathBuf) {
        let root = std::env::temp_dir().join(format!("swarmie-auth-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let script =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/native_auth.py");
        let args = vec![script.to_string_lossy().into_owned(), mode.into()];
        let (process, stdin, stdout, stderr) = ManagedProcess::spawn_with_pipes(
            &process::find_executable("python3").unwrap(),
            &args,
            &root,
            &[],
        )
        .await
        .unwrap();
        let id = Uuid::new_v4();
        let login = Arc::new(Login {
            public: RwLock::new(json!({"id":id,"status":"pending"})),
            process: Arc::new(process),
            stdin: Mutex::new(Box::new(stdin)),
            cancelled: AtomicBool::new(false),
            sdk,
            native_google: false,
            selected_google: AtomicBool::new(false),
            google_menu_ready: AtomicBool::new(false),
            credential: Mutex::new(None),
            overlay: Mutex::new(AgentToolExecutionOverlay {
                config_home: Some(root.clone()),
                ..Default::default()
            }),
        });
        let native = NativeAgentTools::new();
        native.flows.write().await.insert(id, login.clone());
        let provider = if mode == "console" {
            "anthropic"
        } else {
            "openai"
        };
        spawn_reader(stdout, login.clone(), "codex".into(), provider.into());
        spawn_reader(stderr, login.clone(), "codex".into(), provider.into());
        (native, id, login, root)
    }
    async fn wait_field(login: &Login, field: &str) -> Value {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let public = login.public.read().await.clone();
                if !public[field].is_null() {
                    return public;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap()
    }
    #[tokio::test]
    async fn console_prompt_without_newline_accepts_private_input_and_sanitizes_logs() {
        let (native, id, login, root) = fixture("console", false).await;
        let flow = wait_field(&login, "inputFields").await;
        assert_eq!(
            flow["authorizationUrl"],
            "https://console.anthropic.com/oauth/authorize?state=fixture"
        );
        assert!(!flow.to_string().contains("fixture-secret"));
        native
            .input(id, json!({"code":"fixture-private-code"}))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while native.flow(id).await.unwrap()["status"] == "pending" {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(native.flow(id).await.unwrap()["status"], "connected");
        assert!(
            !native
                .flow(id)
                .await
                .unwrap()
                .to_string()
                .contains("fixture-private-code")
        );
        native.release(id).await.unwrap();
        assert!(native.flow(id).await.is_err());
        assert!(login.process.exited());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn device_banner_exposes_code_and_cancellation_kills_process_and_clears_fields() {
        let (native, id, login, root) = fixture("device", false).await;
        let flow = wait_field(&login, "userCode").await;
        assert_eq!(flow["userCode"], "ABCD-EFGH");
        assert_eq!(native.cancel(id).await.unwrap()["status"], "cancelled");
        assert!(login.process.exited());
        assert!(native.input(id, json!({"code":"late-code"})).await.is_err());
        cli_line(&login, "openai", "Login successful").await;
        let flow = native.flow(id).await.unwrap();
        assert_eq!(flow["status"], "cancelled");
        assert!(flow["authorizationUrl"].is_null() && flow["userCode"].is_null());
        native.release(id).await.unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn sdk_key_is_private_consumable_and_late_completion_cannot_revive_cancelled_flow() {
        let (native, id, login, root) = fixture("idle", true).await;
        sdk_frame(
            &login,
            "cursor",
            "cursor",
            json!({"type":"completed","apiKey":"fixture-private-sdk-key"}),
        )
        .await;
        assert_eq!(native.flow(id).await.unwrap()["status"], "connected");
        assert!(
            !native
                .flow(id)
                .await
                .unwrap()
                .to_string()
                .contains("fixture-private-sdk-key")
        );
        assert_eq!(
            native.login_credential(id).await.unwrap().as_deref(),
            Some("fixture-private-sdk-key")
        );
        assert!(login.process.exited());
        native.cancel(id).await.unwrap();
        sdk_frame(
            &login,
            "cursor",
            "cursor",
            json!({"type":"completed","apiKey":"late-secret"}),
        )
        .await;
        assert!(native.login_credential(id).await.unwrap().is_none());
        assert_eq!(native.flow(id).await.unwrap()["status"], "cancelled");
        native.release(id).await.unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn oversized_native_output_never_reaches_public_state_and_shutdown_owns_children() {
        let (native, id, login, root) = fixture("overflow", false).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(native.flow(id).await.unwrap()["status"], "failed");
        assert!(!native.flow(id).await.unwrap().to_string().contains("xxxxx"));
        native.shutdown().await.unwrap();
        assert!(login.process.exited());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn authorization_urls_and_device_codes_are_restricted_to_safe_vendor_values() {
        assert!(authorization_url(
            "cursor",
            "https://cursor.com/login?state=opaque"
        ));
        for url in [
            "http://cursor.com/login",
            "https://cursor.com.evil.test/login",
            "https://user:secret@cursor.com/login",
            "https://cursor.com/login#token",
            "https://cursor.com/login?access_token=secret",
            "https://cursor.com:444/login",
        ] {
            assert!(!authorization_url("cursor", url));
        }
        assert!(valid_device_code("ABCD-EFGH"));
        assert!(!valid_device_code("FIXTURE-SECRET-DO-NOT-PUBLISH"));
        assert!(!valid_device_code("abcd-efgh"));
    }
    #[test]
    fn native_oauth_file_is_owner_only_and_rejects_invalid_updates() {
        let root = std::env::temp_dir().join(format!("swarmie-oauth-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("auth.json");
        write_native_credential(&path, &json!({"fixture":{"type":"oauth"}})).unwrap();
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(write_native_credential(&path, &json!({"overwrite":true})).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn repeated_native_sign_in_updates_only_the_selected_provider_and_preserves_history() {
        let root = std::env::temp_dir().join(format!("swarmie-oauth-repeat-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("auth.json");
        std::fs::write(root.join("history.jsonl"), b"retained native history").unwrap();
        write_native_credential(
            &path,
            &json!({"openai-codex":{"type":"oauth","access":"first","refresh":"refresh-first"}}),
        )
        .unwrap();
        write_native_credential(&path, &json!({"github-copilot":{"type":"oauth","access":"copilot","refresh":"refresh-copilot"}})).unwrap();
        write_native_credential(&path, &json!({"openai-codex":{"type":"oauth","access":"renewed","refresh":"refresh-renewed"}})).unwrap();
        let stored: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(stored["openai-codex"]["access"], "renewed");
        assert_eq!(stored["github-copilot"]["access"], "copilot");
        assert_eq!(
            std::fs::read(root.join("history.jsonl")).unwrap(),
            b"retained native history"
        );
        assert!(std::fs::read_dir(&root).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")
        }));
        std::fs::remove_dir_all(root).unwrap();
    }
}
