use super::installation;
use futures_util::StreamExt;
use process::ManagedProcess;
use reqwest::{Client, Method, Url};
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
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, BufReader},
    sync::{Mutex, RwLock},
    task::JoinHandle,
};
use uuid::Uuid;

const MAX_RESPONSE: usize = 128 * 1024;
const LOGIN_DEADLINE: Duration = Duration::from_secs(900);

pub(super) struct OpencodeAuth {
    flows: Arc<RwLock<BTreeMap<Uuid, Arc<Login>>>>,
}
struct Login {
    public: RwLock<Value>,
    process: Arc<ManagedProcess>,
    directory: PathBuf,
    config_home: PathBuf,
    data_home: PathBuf,
    provider: String,
    cancelled: AtomicBool,
    code_mode: AtomicBool,
    callback_running: AtomicBool,
    http: RwLock<Option<Http>>,
    method: RwLock<Option<usize>>,
    workers: Mutex<Vec<JoinHandle<()>>>,
}
#[derive(Clone)]
struct Http {
    client: Client,
    base: Url,
    password: String,
    directory: PathBuf,
}

impl OpencodeAuth {
    pub(super) fn new() -> Self {
        Self {
            flows: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }
    pub(super) async fn contains(&self, id: Uuid) -> bool {
        self.flows.read().await.contains_key(&id)
    }
    pub(super) async fn start(&self, id: Uuid, provider: &str, root: &Path) -> Result<Value> {
        let managed = installation::managed_overlay(root, "opencode");
        let executable = managed.config["executable"]
            .as_str()
            .map(PathBuf::from)
            .or_else(|| crate::configured_executable("opencode"))
            .and_then(|path| process::find_executable(&path.to_string_lossy()))
            .or_else(|| process::find_executable("opencode"))
            .ok_or_else(|| {
                Error::Provider("Install OpenCode before connecting its provider account".into())
            })?;
        self.start_process(id, provider, root, &executable, &[])
            .await
    }
    async fn start_process(
        &self,
        id: Uuid,
        provider: &str,
        root: &Path,
        executable: &Path,
        prefix: &[String],
    ) -> Result<Value> {
        if !matches!(provider, "openai" | "github-copilot") {
            return Err(Error::Unsupported(
                "Managed OpenCode OAuth for this provider".into(),
            ));
        }
        if self.contains(id).await {
            return Err(Error::Invalid("OpenCode login ID is already active".into()));
        }
        let directory = root.join("native-profiles/opencode").join(id.to_string());
        let parent = directory
            .parent()
            .ok_or_else(|| Error::Invalid("Invalid OpenCode login profile".into()))?;
        private_directory(parent)?;
        std::fs::create_dir(&directory)?;
        private_directory(&directory)?;
        let config_home = directory.join("config");
        let data_home = directory.join("data");
        for child in [
            &config_home,
            &data_home,
            &directory.join("cache"),
            &directory.join("xdg-config"),
            &directory.join("state"),
        ] {
            private_directory(child)?;
        }
        let password = format!("{}{}", Uuid::new_v4(), Uuid::new_v4());
        let env = vec![
            (
                "OPENCODE_CONFIG_DIR".into(),
                config_home.to_string_lossy().into_owned(),
            ),
            (
                "XDG_DATA_HOME".into(),
                data_home.to_string_lossy().into_owned(),
            ),
            (
                "XDG_CACHE_HOME".into(),
                directory.join("cache").to_string_lossy().into_owned(),
            ),
            (
                "XDG_CONFIG_HOME".into(),
                directory.join("xdg-config").to_string_lossy().into_owned(),
            ),
            (
                "XDG_STATE_HOME".into(),
                directory.join("state").to_string_lossy().into_owned(),
            ),
            ("OPENCODE_SERVER_USERNAME".into(), "swarmie".into()),
            ("OPENCODE_SERVER_PASSWORD".into(), password.clone()),
            ("OPENCODE_AUTH_CONTENT".into(), String::new()),
            ("OPENCODE_CONFIG_CONTENT".into(), "{}".into()),
        ];
        let mut args = prefix.to_vec();
        args.extend(["serve", "--hostname", "127.0.0.1", "--port", "0"].map(String::from));
        let pipes = ManagedProcess::spawn_with_pipes(executable, &args, &directory, &env).await;
        let (process, stdin, stdout, stderr) = match pipes {
            Ok(pipes) => pipes,
            Err(_) => {
                let _ = std::fs::remove_dir_all(&directory);
                return Err(Error::Provider(
                    "OpenCode sign-in server could not be started".into(),
                ));
            }
        };
        drop(stdin);
        let public =
            json!({"id":id,"status":"pending","message":"Starting OpenCode native sign-in"});
        let login = Arc::new(Login {
            public: RwLock::new(public.clone()),
            process: Arc::new(process),
            directory,
            config_home,
            data_home,
            provider: provider.into(),
            cancelled: AtomicBool::new(false),
            code_mode: AtomicBool::new(false),
            callback_running: AtomicBool::new(false),
            http: RwLock::new(None),
            method: RwLock::new(None),
            workers: Mutex::new(vec![]),
        });
        self.flows.write().await.insert(id, login.clone());
        let output = login.clone();
        let startup = tokio::spawn(async move {
            if initialize(&output, stdout, password).await.is_err() {
                fail(&output, "OpenCode native sign-in could not be initialized. Check the installed version and provider connectivity").await;
            }
        });
        let errors = login.clone();
        let stderr = tokio::spawn(async move {
            discard(stderr, &errors).await;
        });
        let watcher = login.clone();
        let monitor = tokio::spawn(async move {
            let started = tokio::time::Instant::now();
            loop {
                tokio::time::sleep(Duration::from_millis(200)).await;
                if watcher.public.read().await["status"] != "pending" {
                    break;
                }
                if watcher.process.exited() {
                    fail(
                        &watcher,
                        "OpenCode sign-in server stopped before authorization was confirmed",
                    )
                    .await;
                    break;
                }
                if started.elapsed() >= LOGIN_DEADLINE {
                    fail(&watcher, "OpenCode sign-in expired. Start a new connection").await;
                    break;
                }
            }
        });
        login
            .workers
            .lock()
            .await
            .extend([startup, stderr, monitor]);
        Ok(public)
    }
    async fn login(&self, id: Uuid) -> Result<Arc<Login>> {
        self.flows
            .read()
            .await
            .get(&id)
            .cloned()
            .ok_or_else(|| Error::Invalid("Unknown OpenCode login flow".into()))
    }
    pub(super) async fn flow(&self, id: Uuid) -> Result<Value> {
        Ok(self.login(id).await?.public.read().await.clone())
    }
    pub(super) async fn input(&self, id: Uuid, values: Value) -> Result<Value> {
        let login = self.login(id).await?;
        if login.public.read().await["status"] != "pending"
            || !login.code_mode.load(Ordering::SeqCst)
        {
            return Err(Error::Invalid(
                "This OpenCode login is not waiting for an authorization code".into(),
            ));
        }
        if values.as_object().is_none_or(|values| values.len() != 1) {
            return Err(Error::Invalid(
                "Enter the requested OpenCode authorization code".into(),
            ));
        }
        let code = values["code"]
            .as_str()
            .filter(|value| {
                !value.trim().is_empty()
                    && value.len() <= 8192
                    && !value.chars().any(char::is_control)
            })
            .ok_or_else(|| Error::Invalid("Enter a valid OpenCode authorization code".into()))?
            .to_owned();
        if login.callback_running.swap(true, Ordering::SeqCst) {
            return Err(Error::Busy);
        }
        // Register the callback while holding the task registry so cancellation cannot
        // miss a newly started request. The coordinator must never wait for OAuth here.
        let mut workers = login.workers.lock().await;
        let pending = {
            let mut public = login.public.write().await;
            if public["status"] != "pending" || login.cancelled.load(Ordering::SeqCst) {
                login.callback_running.store(false, Ordering::SeqCst);
                return Ok(public.clone());
            }
            public.as_object_mut().unwrap().remove("inputFields");
            public["message"] = json!("Finishing native OpenCode sign-in");
            public.clone()
        };
        let callback_login = login.clone();
        workers.push(tokio::spawn(async move {
            let result = callback(&callback_login, Some(&code)).await;
            callback_login.callback_running.store(false, Ordering::SeqCst);
            if result.is_err() {
                fail(&callback_login, "OpenCode did not confirm the authorization code. Start a new connection and try again").await;
            }
        }));
        Ok(pending)
    }
    pub(super) async fn cancel(&self, id: Uuid) -> Result<Value> {
        let login = self.login(id).await?;
        {
            let mut public = login.public.write().await;
            if public["status"] != "pending" {
                return Ok(public.clone());
            }
            login.cancelled.store(true, Ordering::SeqCst);
            *public = json!({"id":id,"status":"cancelled","message":"OpenCode sign-in cancelled"});
        }
        stop(&login).await;
        let _ = std::fs::remove_dir_all(&login.directory);
        Ok(login.public.read().await.clone())
    }
    pub(super) async fn release(&self, id: Uuid) -> Result<()> {
        if let Some(login) = self.flows.write().await.remove(&id) {
            let connected = login.public.read().await["status"] == "connected";
            stop(&login).await;
            if !connected {
                let _ = std::fs::remove_dir_all(&login.directory);
            }
        }
        Ok(())
    }
    pub(super) async fn shutdown(&self) -> Result<()> {
        let ids: Vec<_> = self.flows.read().await.keys().copied().collect();
        for id in ids {
            if self.flow(id).await?["status"] == "pending" {
                let _ = self.cancel(id).await;
            }
            self.release(id).await?;
        }
        Ok(())
    }
    pub(super) async fn login_overlay(
        &self,
        id: Uuid,
    ) -> Result<Option<AgentToolExecutionOverlay>> {
        let login = self.login(id).await?;
        if login.public.read().await["status"] != "connected" {
            return Ok(None);
        }
        Ok(Some(AgentToolExecutionOverlay {
            config_home: Some(login.config_home.clone()),
            config: json!({"provider":login.provider,"nativeAuthDataHome":login.data_home}),
            ..Default::default()
        }))
    }
}
impl Drop for OpencodeAuth {
    fn drop(&mut self) {
        let flows = self.flows.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                for login in std::mem::take(&mut *flows.write().await).into_values() {
                    stop(&login).await;
                }
            });
        }
    }
}

async fn initialize(
    login: &Arc<Login>,
    stdout: tokio::process::ChildStdout,
    password: String,
) -> Result<()> {
    let mut reader = BufReader::new(stdout);
    let base = tokio::time::timeout(Duration::from_secs(20), async {
        let mut total = 0;
        loop {
            let mut line = vec![];
            let read = (&mut reader)
                .take(4097)
                .read_until(b'\n', &mut line)
                .await?;
            total += read;
            if read == 0 || read > 4096 || total > 65536 {
                return Err(Error::Provider(
                    "OpenCode listener was not confirmed".into(),
                ));
            }
            if let Some(text) = std::str::from_utf8(&line)
                .ok()
                .and_then(|text| text.trim().strip_prefix("opencode server listening on "))
            {
                let url = Url::parse(text)
                    .map_err(|_| Error::Invalid("Invalid OpenCode listener".into()))?;
                if url.scheme() != "http"
                    || url.host_str() != Some("127.0.0.1")
                    || url.port().is_none_or(|port| port == 0)
                    || !url.username().is_empty()
                    || url.password().is_some()
                    || url.query().is_some()
                    || url.fragment().is_some()
                    || url.path() != "/"
                {
                    return Err(Error::Invalid(
                        "OpenCode sign-in listener must be private localhost".into(),
                    ));
                }
                return Ok(url);
            }
        }
    })
    .await
    .map_err(|_| Error::Timeout("OpenCode listener startup".into()))??;
    let output = login.clone();
    login.workers.lock().await.push(tokio::spawn(async move {
        discard(reader, &output).await;
    }));
    let client = Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .build()
        .map_err(|_| Error::Provider("OpenCode sign-in client could not be initialized".into()))?;
    let http = Http {
        client,
        base,
        password,
        directory: login.directory.clone(),
    };
    let methods = http
        .call(Method::GET, "/provider/auth", None, Duration::from_secs(30))
        .await?;
    let (index, inputs) = oauth_method(&methods, &login.provider)?;
    let mut body = json!({"method":index});
    if !inputs.as_object().is_none_or(|inputs| inputs.is_empty()) {
        body["inputs"] = inputs;
    }
    let authorization = http
        .call(
            Method::POST,
            &format!("/provider/{}/oauth/authorize", login.provider),
            Some(body),
            Duration::from_secs(30),
        )
        .await?;
    let url = authorization_url(&login.provider, &authorization["url"])?;
    let mode = authorization["method"]
        .as_str()
        .filter(|mode| matches!(*mode, "auto" | "code"))
        .ok_or_else(|| {
            Error::Provider("Unsupported native OpenCode authorization response".into())
        })?;
    *login.http.write().await = Some(http);
    *login.method.write().await = Some(index);
    login.code_mode.store(mode == "code", Ordering::SeqCst);
    {
        let mut public = login.public.write().await;
        if public["status"] != "pending" || login.cancelled.load(Ordering::SeqCst) {
            return Ok(());
        }
        public["authorizationUrl"] = json!(url);
        public["message"] = json!("Complete the provider's sign-in in your browser");
        if mode == "code" {
            public["inputFields"] = json!([{"id":"code","label":"Authorization code","secret":true,"placeholder":"Paste the code from the provider"}]);
        }
        if let Some(code) = device_code(&authorization["instructions"]) {
            public["userCode"] = json!(code);
        }
    }
    if mode == "auto" {
        login.callback_running.store(true, Ordering::SeqCst);
        callback(login, None).await?;
    }
    Ok(())
}
impl Http {
    async fn call(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        timeout: Duration,
    ) -> Result<Value> {
        let url = self
            .base
            .join(path)
            .map_err(|_| Error::Invalid("Invalid local OpenCode endpoint".into()))?;
        let mut request = self
            .client
            .request(method, url)
            .basic_auth("swarmie", Some(&self.password))
            .query(&[("directory", self.directory.to_string_lossy().as_ref())])
            .timeout(timeout);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.map_err(|_| {
            Error::Provider("The local OpenCode authentication request failed".into())
        })?;
        if !response.status().is_success() {
            return Err(Error::Provider(
                "OpenCode refused the native authentication request".into(),
            ));
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE as u64)
        {
            return Err(Error::Provider(
                "OpenCode authentication response exceeds its limit".into(),
            ));
        }
        let mut stream = response.bytes_stream();
        let mut bytes = vec![];
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| {
                Error::Provider("OpenCode authentication response was interrupted".into())
            })?;
            if bytes.len() + chunk.len() > MAX_RESPONSE {
                return Err(Error::Provider(
                    "OpenCode authentication response exceeds its limit".into(),
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| {
            Error::Provider("OpenCode returned an invalid native authentication response".into())
        })
    }
}
async fn callback(login: &Arc<Login>, code: Option<&str>) -> Result<()> {
    let http = login.http.read().await.clone().ok_or(Error::Closed)?;
    let index = login.method.read().await.ok_or(Error::Closed)?;
    let mut body = json!({"method":index});
    if let Some(code) = code {
        body["code"] = json!(code);
    }
    if http
        .call(
            Method::POST,
            &format!("/provider/{}/oauth/callback", login.provider),
            Some(body),
            LOGIN_DEADLINE,
        )
        .await?
        != true
    {
        return Err(Error::Provider(
            "OpenCode did not confirm native OAuth completion".into(),
        ));
    }
    {
        let mut public = login.public.write().await;
        if public["status"] == "pending" && !login.cancelled.load(Ordering::SeqCst) {
            *public = json!({"id":public["id"],"status":"connected","message":"Native OpenCode provider sign-in confirmed"});
        }
    }
    let _ = login.process.kill().await;
    Ok(())
}
async fn fail(login: &Login, message: &str) {
    let mut public = login.public.write().await;
    if public["status"] == "pending" {
        *public = json!({"id":public["id"],"status":"failed","message":message});
    }
    drop(public);
    let _ = login.process.kill().await;
}
async fn stop(login: &Login) {
    login.cancelled.store(true, Ordering::SeqCst);
    for task in std::mem::take(&mut *login.workers.lock().await) {
        task.abort();
    }
    let _ = login.process.kill().await;
}
async fn discard(mut reader: impl AsyncRead + Unpin, login: &Login) {
    let mut buffer = [0; 4096];
    let mut total = 0;
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(size) => {
                total += size;
                if total > MAX_RESPONSE {
                    fail(
                        login,
                        "OpenCode sign-in server exceeded its diagnostic output limit",
                    )
                    .await;
                    break;
                }
            }
        }
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
fn oauth_method(methods: &Value, provider: &str) -> Result<(usize, Value)> {
    let methods = methods[provider].as_array().ok_or_else(|| {
        Error::Unsupported(
            "This OpenCode version does not expose native OAuth for the selected provider".into(),
        )
    })?;
    let mut candidates: Vec<_> = methods
        .iter()
        .enumerate()
        .filter(|(_, method)| method["type"] == "oauth")
        .collect();
    candidates.sort_by_key(|(_, method)| {
        let label = method["label"].as_str().unwrap_or_default().to_lowercase();
        !label.contains("headless") && !label.contains("device")
    });
    for (index, method) in candidates {
        if let Some(inputs) = prompt_defaults(provider, &method["prompts"]) {
            return Ok((index, inputs));
        }
    }
    Err(Error::Unsupported(
        "Native OpenCode OAuth requires provider prompts that this setup does not support".into(),
    ))
}
fn prompt_defaults(provider: &str, prompts: &Value) -> Option<Value> {
    if prompts.is_null() {
        return Some(json!({}));
    }
    let mut inputs = json!({});
    for prompt in prompts.as_array()? {
        if let Some(condition) = prompt.get("when") {
            let key = condition["key"].as_str()?;
            let expected = condition["value"].as_str()?;
            if condition["op"] != "eq" {
                return None;
            }
            let actual = inputs[key].as_str()?;
            if actual != expected {
                continue;
            }
        }
        if provider == "github-copilot"
            && prompt["key"] == "deploymentType"
            && prompt["type"] == "select"
            && prompt["options"]
                .as_array()?
                .iter()
                .any(|option| option["value"] == "github.com")
        {
            inputs["deploymentType"] = json!("github.com");
        } else {
            return None;
        }
    }
    Some(inputs)
}
fn authorization_url(provider: &str, value: &Value) -> Result<String> {
    let url = value
        .as_str()
        .filter(|url| url.len() <= 8192)
        .and_then(|url| Url::parse(url).ok())
        .ok_or_else(|| Error::Provider("Invalid native provider authorization URL".into()))?;
    let allowed = match provider {
        "openai" => matches!(url.host_str(), Some("auth.openai.com" | "chatgpt.com")),
        "github-copilot" => url.host_str() == Some("github.com"),
        _ => false,
    };
    if !allowed
        || url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(Error::Provider(
            "The native provider returned an unexpected authorization URL".into(),
        ));
    }
    Ok(url.to_string())
}
fn device_code(value: &Value) -> Option<&str> {
    let code = value.as_str()?.trim().strip_prefix("Enter code:")?.trim();
    (code.len() >= 4
        && code.len() <= 32
        && code
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'))
    .then_some(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Workspace(PathBuf);
    impl Workspace {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("swarmie-opencode-oauth-{}", Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(path.join("global-auth.json"), "existing-user-credentials").unwrap();
            Self(path)
        }
        async fn start(&self, owner: &OpencodeAuth, id: Uuid, provider: &str, mode: &str) -> Value {
            owner
                .start_process(
                    id,
                    provider,
                    &self.0,
                    &process::find_executable("python3").unwrap(),
                    &[
                        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                            .join("tests/fixtures/opencode_oauth.py")
                            .to_string_lossy()
                            .into_owned(),
                        mode.into(),
                    ],
                )
                .await
                .unwrap()
        }
        fn directory(&self, id: Uuid) -> PathBuf {
            self.0.join("native-profiles/opencode").join(id.to_string())
        }
        fn requests(&self, id: Uuid) -> Vec<Value> {
            std::fs::read_to_string(self.directory(id).join("auth-requests.jsonl"))
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect()
        }
    }
    impl Drop for Workspace {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    async fn wait(owner: &OpencodeAuth, id: Uuid, predicate: impl Fn(&Value) -> bool) -> Value {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let value = owner.flow(id).await.unwrap();
                if predicate(&value) {
                    return value;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn native_auto_oauth_uses_only_auth_endpoints_and_preserves_private_provider_storage() {
        let workspace = Workspace::new();
        let owner = OpencodeAuth::new();
        let id = Uuid::new_v4();
        assert_eq!(
            workspace.start(&owner, id, "openai", "auto").await["status"],
            "pending"
        );
        let pending = wait(&owner, id, |value| value["authorizationUrl"].is_string()).await;
        assert_eq!(
            pending["authorizationUrl"],
            "https://auth.openai.com/codex/device"
        );
        assert_eq!(pending["userCode"], "ABCD-1234");
        let done = wait(&owner, id, |value| value["status"] == "connected").await;
        assert!(!done.to_string().contains("fixture-private-secret"));
        let calls = workspace.requests(id);
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0]["path"], "/provider/auth");
        assert_eq!(calls[1]["body"], json!({"method":2}));
        assert_eq!(calls[2]["body"], json!({"method":2}));
        assert!(
            calls
                .iter()
                .all(|call| call["authenticated"] == true && call["privateDirectory"] == true)
        );
        let overlay = owner.login_overlay(id).await.unwrap().unwrap();
        assert!(overlay.env.is_empty());
        assert_eq!(overlay.config["provider"], "openai");
        assert_eq!(
            overlay.config["nativeAuthDataHome"],
            json!(workspace.directory(id).join("data"))
        );
        assert_eq!(
            std::fs::read_to_string(workspace.0.join("global-auth.json")).unwrap(),
            "existing-user-credentials"
        );
        owner.release(id).await.unwrap();
        assert!(!owner.contains(id).await);
        assert!(
            workspace
                .directory(id)
                .join("data/opencode/auth.json")
                .is_file()
        );
    }

    #[tokio::test]
    async fn native_code_callback_accepts_requested_secret_input_without_echoing_instructions() {
        let workspace = Workspace::new();
        let owner = OpencodeAuth::new();
        let id = Uuid::new_v4();
        workspace.start(&owner, id, "openai", "code").await;
        let pending = wait(&owner, id, |value| value["inputFields"].is_array()).await;
        assert!(!pending.to_string().contains("fixture-private-secret"));
        assert_eq!(pending["inputFields"][0]["secret"], true);
        assert_eq!(
            owner
                .input(id, json!({"code":"private-test-code"}))
                .await
                .unwrap()["status"],
            "pending"
        );
        wait(&owner, id, |value| value["status"] == "connected").await;
        assert_eq!(
            workspace.requests(id)[2]["body"],
            json!({"method":2,"code":"private-test-code"})
        );
        owner.release(id).await.unwrap();
    }

    #[tokio::test]
    async fn copilot_native_domain_prompt_defaults_to_public_github() {
        let workspace = Workspace::new();
        let owner = OpencodeAuth::new();
        let id = Uuid::new_v4();
        workspace.start(&owner, id, "github-copilot", "auto").await;
        wait(&owner, id, |value| value["status"] == "connected").await;
        assert_eq!(
            workspace.requests(id)[1]["body"],
            json!({"method":1,"inputs":{"deploymentType":"github.com"}})
        );
        owner.release(id).await.unwrap();
    }

    #[tokio::test]
    async fn cancellation_stops_native_callback_and_cannot_publish_late_success() {
        let workspace = Workspace::new();
        let owner = OpencodeAuth::new();
        let id = Uuid::new_v4();
        workspace.start(&owner, id, "openai", "slow").await;
        wait(&owner, id, |value| value["authorizationUrl"].is_string()).await;
        assert_eq!(owner.cancel(id).await.unwrap()["status"], "cancelled");
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert_eq!(owner.flow(id).await.unwrap()["status"], "cancelled");
        assert!(!workspace.directory(id).exists());
        owner.release(id).await.unwrap();
    }

    #[tokio::test]
    async fn code_input_returns_pending_and_delayed_callback_remains_cancellable() {
        let workspace = Workspace::new();
        let owner = OpencodeAuth::new();
        let id = Uuid::new_v4();
        workspace.start(&owner, id, "openai", "slow_code").await;
        wait(&owner, id, |value| value["inputFields"].is_array()).await;
        let pending = tokio::time::timeout(
            Duration::from_secs(1),
            owner.input(id, json!({"code":"private-test-code"})),
        )
        .await
        .expect("Submitting a code must not wait for native OAuth completion")
        .unwrap();
        assert_eq!(pending["status"], "pending");
        assert!(pending.get("inputFields").is_none());
        assert!(!pending.to_string().contains("private-test-code"));
        tokio::time::timeout(Duration::from_secs(2), async {
            while workspace.requests(id).len() < 3 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("The delayed native callback must be in flight before cancellation");
        let cancelled = tokio::time::timeout(Duration::from_secs(3), owner.cancel(id))
            .await
            .expect("An outstanding code callback must remain cancellable")
            .unwrap();
        assert_eq!(cancelled["status"], "cancelled");
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert_eq!(owner.flow(id).await.unwrap()["status"], "cancelled");
        assert!(!workspace.directory(id).exists());
        owner.release(id).await.unwrap();
    }

    #[tokio::test]
    async fn untrusted_urls_and_native_errors_do_not_expose_raw_provider_diagnostics() {
        for mode in ["bad_url", "error", "oversized"] {
            let workspace = Workspace::new();
            let owner = OpencodeAuth::new();
            let id = Uuid::new_v4();
            workspace.start(&owner, id, "openai", mode).await;
            let failed = wait(&owner, id, |value| value["status"] == "failed").await;
            assert!(!failed.to_string().contains("fixture-private-secret"));
            assert!(failed.get("authorizationUrl").is_none());
            assert!(owner.login_overlay(id).await.unwrap().is_none());
            owner.release(id).await.unwrap();
            assert!(!workspace.directory(id).exists());
        }
    }

    #[test]
    fn unsupported_prompt_shapes_and_malformed_device_codes_are_rejected() {
        assert!(oauth_method(&json!({"openai":[{"type":"oauth","label":"headless","prompts":[{"type":"text","key":"unsupported"}]}]}), "openai").is_err());
        assert!(authorization_url("openai", &json!("http://auth.openai.com/login")).is_err());
        assert!(
            authorization_url(
                "github-copilot",
                &json!("https://github.com.attacker.example/login")
            )
            .is_err()
        );
        assert!(device_code(&json!("Enter code: \u{1b}[31mSECRET")).is_none());
        assert!(device_code(&json!("Arbitrary private instructions")).is_none());
    }
}
