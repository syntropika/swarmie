use super::*;
use process::ManagedProcess;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

pub(super) const SDK_VERSION: &str = "1.0.37";
const FRAME_LIMIT: usize = 8 * 1024 * 1024;
const DISCOVERY_LIMIT: usize = 64 * 1024;

/// The bridge token is local process authentication, never a Cursor account key.
pub(super) struct Bridge {
    client: reqwest::Client,
    url: reqwest::Url,
    token: String,
    secrets: Vec<String>,
    process: ManagedProcess,
    stderr: tokio::task::JoinHandle<()>,
    pub(super) pinned_contract: bool,
}

pub(super) fn executable() -> Option<PathBuf> {
    if let Some(path) = configured_executable("cursor")
        .or_else(|| std::env::var_os("CURSOR_SDK_BRIDGE_BIN").map(PathBuf::from))
    {
        return find_executable(&path.to_string_lossy());
    }
    if let Some(root) = agent_tools_root() {
        let name = if cfg!(windows) {
            "cursor-sdk-bridge.exe"
        } else {
            "cursor-sdk-bridge"
        };
        let path = root.join("cursor/bin").join(name);
        if path.is_file() {
            return Some(path);
        }
    }
    find_executable("cursor-sdk-bridge")
}

pub(super) fn key(env: &BTreeMap<String, String>) -> Option<String> {
    env.get("CURSOR_API_KEY")
        .cloned()
        .or_else(|| std::env::var("CURSOR_API_KEY").ok())
        .filter(|key| !key.trim().is_empty())
}

impl Bridge {
    pub(super) async fn spawn(o: &SpawnOptions) -> Result<Arc<Self>> {
        Self::spawn_with_deadline(o, Duration::from_secs(30)).await
    }
    async fn spawn_with_deadline(
        o: &SpawnOptions,
        startup_deadline: Duration,
    ) -> Result<Arc<Self>> {
        let exe = o.config["executable"]
            .as_str()
            .map(PathBuf::from)
            .or_else(executable)
            .ok_or_else(|| {
                Error::Provider("Install Cursor SDK Bridge or set SWARMIE_CURSOR_EXECUTABLE".into())
            })?;
        let mut args: Vec<String> = o.config["args"]
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();
        let pinned_contract = pinned_contract(&exe);
        args.extend([
            "--workspace".into(),
            o.cwd.to_string_lossy().into_owned(),
            "--host".into(),
            "127.0.0.1".into(),
            "--port".into(),
            "0".into(),
        ]);
        let mut env = o.execution.env.clone();
        env.insert("CURSOR_SDK_CLIENT_LANGUAGE".into(), "rust".into());
        if let Some(home) = &o.execution.config_home {
            std::fs::create_dir_all(home)?;
            args.extend([
                "--state-root".into(),
                std::fs::canonicalize(home)?.to_string_lossy().into_owned(),
            ]);
        }
        let (process, stderr) = ManagedProcess::spawn_with_stderr(
            &exe,
            &args,
            &o.cwd,
            &env.into_iter().collect::<Vec<_>>(),
        )
        .await?;
        let mut reader = BufReader::new(stderr);
        // Diagnostic output can include credentials. Neither the ready line nor arbitrary
        // stderr is forwarded into persisted events or startup errors.
        let ready = tokio::time::timeout(startup_deadline, async {
            while let Some(line) = discovery_line(&mut reader).await? {
                if let Some(json) = line.strip_prefix(b"cursor-sdk-bridge ready ") {
                    let value: Value = serde_json::from_slice(json).map_err(|_| {
                        Error::Provider("Invalid Cursor bridge discovery payload".into())
                    })?;
                    return Ok(value);
                }
            }
            Err(Error::Provider(
                "Cursor SDK Bridge exited before readiness".into(),
            ))
        })
        .await
        .map_err(|_| Error::Timeout("Cursor SDK Bridge startup".into()))??;
        let stderr = tokio::spawn(async move {
            let _ = tokio::io::copy(&mut reader, &mut tokio::io::sink()).await;
        });
        let result = async {
            if ready["schemaVersion"] != 1
                || ready["transport"] != "tcp"
                || ready["protocol"] != "connect"
            {
                return Err(Error::Provider(
                    "Unsupported Cursor bridge discovery protocol".into(),
                ));
            }
            let url = reqwest::Url::parse(
                ready["url"]
                    .as_str()
                    .ok_or_else(|| Error::Provider("Cursor bridge did not report a URL".into()))?,
            )
            .map_err(|_| Error::Provider("Invalid Cursor bridge endpoint".into()))?;
            let loopback = url
                .host_str()
                .and_then(|host| host.parse::<std::net::IpAddr>().ok())
                .is_some_and(|ip| ip.is_loopback());
            if url.scheme() != "http"
                || !loopback
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
                || url.path() != "/"
            {
                return Err(Error::Provider(
                    "Cursor bridge endpoint must be a plain loopback HTTP URL".into(),
                ));
            }
            let token_path = ready["authTokenFile"].as_str().ok_or_else(|| {
                Error::Provider("Cursor bridge did not report its token file".into())
            })?;
            let mut bytes = Vec::new();
            tokio::fs::File::open(token_path)
                .await?
                .take(4097)
                .read_to_end(&mut bytes)
                .await?;
            let token = String::from_utf8(bytes)
                .map_err(|_| Error::Provider("Invalid Cursor bridge token encoding".into()))?
                .trim()
                .to_owned();
            if token.is_empty() || token.len() > 4096 || token.contains(['\r', '\n']) {
                return Err(Error::Provider(
                    "Invalid Cursor bridge authentication token".into(),
                ));
            }
            let client = reqwest::Client::builder()
                .http1_only()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|e| Error::Provider(e.to_string()))?;
            Ok((url, token, client))
        }
        .await;
        let (url, token, client) = match result {
            Ok(value) => value,
            Err(error) => {
                stderr.abort();
                let _ = process.kill().await;
                return Err(error);
            }
        };
        let mut secrets = vec![token.clone()];
        if let Some(key) = key(&o.execution.env) {
            secrets.push(key);
        }
        let bridge = Arc::new(Self {
            url,
            token,
            secrets,
            client,
            process,
            stderr,
            pinned_contract,
        });
        bridge
            .call("SdkBridgeControlService", "Ping", json!({}))
            .await?;
        let version = bridge
            .call("SdkBridgeControlService", "GetVersion", json!({}))
            .await?;
        if version["protocolVersion"] != "sdk.v1" {
            return Err(Error::Provider("Cursor bridge requires sdk.v1".into()));
        }
        Ok(bridge)
    }
    fn request(&self, service: &str, method: &str) -> reqwest::RequestBuilder {
        let mut url = self.url.clone();
        url.set_path(&format!("/sdk.v1.{service}/{method}"));
        self.client
            .post(url)
            .bearer_auth(&self.token)
            .header("Connect-Protocol-Version", "1")
    }
    pub(super) async fn call(&self, service: &str, method: &str, body: Value) -> Result<Value> {
        self.call_timeout(service, method, body, true).await
    }
    pub(super) async fn call_timeout(
        &self,
        service: &str,
        method: &str,
        body: Value,
        bounded: bool,
    ) -> Result<Value> {
        let request = self.request(service, method).json(&body);
        let request = if bounded {
            request.timeout(Duration::from_secs(30))
        } else {
            request
        };
        let mut response = request
            .send()
            .await
            .map_err(|_| Error::Provider(format!("Cursor {method} transport failed")))?;
        let status = response.status();
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| Error::Provider(format!("Cursor {method} response failed")))?
        {
            if bytes.len().saturating_add(chunk.len()) > FRAME_LIMIT {
                return Err(Error::Provider("Cursor response exceeds 8 MiB".into()));
            }
            bytes.extend_from_slice(&chunk);
        }
        let mut value: Value = serde_json::from_slice(&bytes)
            .map_err(|_| Error::Provider(format!("Cursor {method} returned invalid JSON")))?;
        if !status.is_success() {
            return Err(connect_error(&value, &self.secrets));
        }
        self.sanitize(&mut value);
        Ok(value)
    }
    pub(super) async fn send(&self, body: Value) -> Result<Frames> {
        let payload = serde_json::to_vec(&body)?;
        if payload.len() > FRAME_LIMIT {
            return Err(Error::Rejected("Cursor request exceeds 8 MiB".into()));
        }
        let mut framed = Vec::with_capacity(payload.len() + 5);
        framed.push(0);
        framed.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        framed.extend_from_slice(&payload);
        let request = self
            .request("SdkAgentService", "Send")
            .header("Content-Type", "application/connect+json")
            .body(framed);
        // Bound response headers only, not long turns or idle tool pauses.
        let response = tokio::time::timeout(Duration::from_secs(30), request.send())
            .await
            .map_err(|_| {
                Error::Timeout("Cursor Send handshake; submission may have been accepted".into())
            })?
            .map_err(|_| {
                Error::Provider(
                    "Cursor Send transport failed; submission may have been accepted".into(),
                )
            })?;
        if !response.status().is_success() {
            // HTTP error status does not certify a submission has no native effects.
            return Err(Error::Provider(format!(
                "Cursor Send rejected HTTP {}",
                response.status().as_u16()
            )));
        }
        Ok(Frames {
            response,
            buffer: Vec::new(),
            pending: Vec::new(),
            position: 0,
            ended: false,
            secrets: self.secrets.clone(),
        })
    }
    pub(super) async fn shutdown(&self) -> Result<()> {
        let _ = tokio::time::timeout(
            Duration::from_secs(5),
            self.call(
                "SdkBridgeControlService",
                "Shutdown",
                json!({"graceSeconds":0}),
            ),
        )
        .await;
        self.process.kill().await
    }
    pub(super) async fn kill(&self) -> Result<()> {
        self.process.kill().await
    }
    pub(super) fn exited(&self) -> bool {
        self.process.exited()
    }
    pub(super) fn sanitize(&self, value: &mut Value) {
        redact(value, &self.secrets);
    }
}
pub(super) fn pinned_contract(exe: &std::path::Path) -> bool {
    use std::io::Read;
    let Some(root) = exe.parent().and_then(|bin| bin.parent()) else {
        return false;
    };
    let Ok(file) = std::fs::File::open(root.join("manifest.json")) else {
        return false;
    };
    let mut bytes = Vec::new();
    if file.take(8193).read_to_end(&mut bytes).is_err() || bytes.len() > 8192 {
        return false;
    }
    serde_json::from_slice::<Value>(&bytes)
        .ok()
        .is_some_and(|manifest| {
            manifest["protocol"] == "sdk.v1" && manifest["sdkVersion"] == SDK_VERSION
        })
}
impl Drop for Bridge {
    fn drop(&mut self) {
        self.stderr.abort();
    }
}

pub(super) struct Frames {
    response: reqwest::Response,
    buffer: Vec<u8>,
    pending: Vec<u8>,
    position: usize,
    ended: bool,
    secrets: Vec<String>,
}
impl Frames {
    pub(super) async fn next(&mut self) -> Result<Option<Value>> {
        if self.ended {
            return Ok(None);
        }
        self.fill(5).await?;
        let length = u32::from_be_bytes(self.buffer[1..5].try_into().unwrap()) as usize;
        if length > FRAME_LIMIT {
            return Err(Error::Provider("Cursor stream frame exceeds 8 MiB".into()));
        }
        let flags = self.buffer[0];
        if flags != 0 && flags != 2 {
            return Err(Error::Provider(
                "Unsupported Cursor stream frame flags".into(),
            ));
        }
        self.fill(length + 5).await?;
        let mut value: Value = if length == 0 && flags == 2 {
            json!({})
        } else {
            serde_json::from_slice(&self.buffer[5..length + 5])
                .map_err(|_| Error::Provider("Invalid Cursor stream JSON".into()))?
        };
        self.buffer.clear();
        if flags == 2 {
            self.ended = true;
            if let Some(error) = value.get("error") {
                return Err(connect_error(error, &self.secrets));
            }
            return Ok(None);
        }
        redact(&mut value, &self.secrets);
        Ok(Some(value))
    }
    async fn fill(&mut self, required: usize) -> Result<()> {
        while self.buffer.len() < required {
            if self.position == self.pending.len() {
                let chunk = self
                    .response
                    .chunk()
                    .await
                    .map_err(|_| Error::Provider("Cursor run stream disconnected".into()))?
                    .ok_or_else(|| {
                        Error::Provider("Cursor run stream ended without Connect end frame".into())
                    })?;
                self.pending = chunk.to_vec();
                self.position = 0;
            }
            let count = (required - self.buffer.len()).min(self.pending.len() - self.position);
            self.buffer
                .extend_from_slice(&self.pending[self.position..self.position + count]);
            self.position += count;
        }
        Ok(())
    }
}
async fn discovery_line(
    reader: &mut BufReader<tokio::process::ChildStderr>,
) -> Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                Ok(Some(line))
            };
        }
        let count = chunk
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|index| index + 1)
            .unwrap_or(chunk.len());
        if line.len().saturating_add(count) > DISCOVERY_LIMIT {
            return Err(Error::Provider(
                "Cursor bridge discovery line exceeds 64 KiB".into(),
            ));
        }
        let done = chunk[count - 1] == b'\n';
        line.extend_from_slice(&chunk[..count]);
        reader.consume(count);
        if done {
            return Ok(Some(line));
        }
    }
}

fn connect_error(value: &Value, secrets: &[String]) -> Error {
    let code = value["code"].as_str().unwrap_or("unknown");
    let code = if code.len() < 80 && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        code
    } else {
        "unknown"
    };
    let mut message = value["message"]
        .as_str()
        .unwrap_or("No provider diagnostic")
        .chars()
        .take(4096)
        .collect::<String>();
    for secret in secrets {
        message = message.replace(secret, "[redacted]");
    }
    Error::Provider(format!("Cursor SDK bridge RPC failed ({code}): {message}"))
}
fn redact(value: &mut Value, secrets: &[String]) {
    match value {
        Value::String(value) => {
            for secret in secrets {
                *value = value.replace(secret, "[redacted]");
            }
        }
        Value::Array(values) => {
            for value in values {
                redact(value, secrets);
            }
        }
        Value::Object(fields) => {
            for (key, value) in fields {
                if ["api_key", "apikey", "authtoken", "authorization"]
                    .contains(&key.to_lowercase().as_str())
                {
                    *value = json!("[redacted]");
                } else {
                    redact(value, secrets);
                }
            }
        }
        _ => (),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[tokio::test]
    async fn startup_deadline_stops_a_bridge_that_never_reports_ready() {
        let root =
            std::env::temp_dir().join(format!("swarmie-cursor-timeout-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let mut options = SpawnOptions::new("cursor", &root);
        options.config = json!({"executable":"python3","args":[PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cursor_sdk_bridge.py")]});
        options
            .execution
            .env
            .insert("FIXTURE_NEVER_READY".into(), "1".into());
        let result = Bridge::spawn_with_deadline(&options, Duration::from_secs(1)).await;
        assert!(matches!(result, Err(Error::Timeout(_))));
        #[cfg(unix)]
        {
            let pid = std::fs::read_to_string(root.join("bridge-pid")).unwrap();
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    let status = tokio::process::Command::new("kill")
                        .args(["-0", pid.trim()])
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status()
                        .await
                        .unwrap();
                    if !status.success() {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .unwrap();
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    #[ignore = "requires the pinned official standalone Cursor SDK Bridge artifact"]
    async fn official_bridge_handshake_without_provider_requests() {
        let root =
            std::env::temp_dir().join(format!("swarmie-cursor-smoke-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let mut options = SpawnOptions::new("cursor", &root);
        options.config = json!({"executable":std::env::var("SWARMIE_TEST_CURSOR_BRIDGE").expect("set the official bridge path")});
        options.execution.config_home = Some(root.join("state"));
        options
            .execution
            .env
            .insert("CURSOR_API_KEY".into(), String::new());
        let bridge = Bridge::spawn(&options).await.unwrap();
        let version = bridge
            .call("SdkBridgeControlService", "GetVersion", json!({}))
            .await
            .unwrap();
        assert_eq!(version["protocolVersion"], "sdk.v1");
        let ping = bridge
            .call("SdkBridgeControlService", "Ping", json!({}))
            .await
            .unwrap();
        assert!(ping["message"].is_string());
        bridge.shutdown().await.unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
