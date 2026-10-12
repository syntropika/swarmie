//! Local execution-owner discovery and attachment, independent of client lifetime.
use clap::Subcommand;
use mpai::{Error, LocalStoreIdentity, Result, RuntimeIdentity};
use protocol::RuntimeService;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::process::{Child, Command};

#[derive(Subcommand)]
pub enum RuntimeCommand {
    Status {
        #[arg(long, env = "MPAI_TOKEN")]
        token: Option<String>,
    },
    /// Stop the execution owner and its native processes; retained history is preserved.
    Stop {
        #[arg(long, env = "MPAI_TOKEN")]
        token: Option<String>,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Endpoint {
    pub url: String,
    pub runtime_identity: RuntimeIdentity,
    pub desktop_assets: Option<PathBuf>,
    pub authenticated: bool,
}
fn record_path(root: &Path) -> PathBuf {
    root.join("runtime/http-endpoint.json")
}
fn regular(path: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => Ok(true),
        Ok(_) => Err(Error::Storage(
            "runtime record must be a regular file".into(),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}
fn private_options() -> std::fs::OpenOptions {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}
pub(crate) fn read(root: &Path) -> Result<Option<Endpoint>> {
    let path = record_path(root);
    if !regular(&path)? {
        return Ok(None);
    }
    if std::fs::metadata(&path)?.len() > 16 * 1024 {
        return Err(Error::Storage("runtime record exceeds 16 KiB".into()));
    }
    let endpoint: Endpoint = serde_json::from_slice(&std::fs::read(path)?)?;
    let url = reqwest::Url::parse(&endpoint.url)
        .map_err(|_| Error::Invalid("invalid runtime endpoint".into()))?;
    if url.scheme() != "http"
        || !url.host_str().is_some_and(|h| {
            h.trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
        })
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || url.port().is_none()
    {
        return Err(Error::Invalid(
            "runtime endpoint must be an explicit loopback HTTP origin".into(),
        ));
    }
    Ok(Some(endpoint))
}
pub struct Registration {
    root: PathBuf,
    identity: RuntimeIdentity,
}
impl Drop for Registration {
    fn drop(&mut self) {
        if read(&self.root)
            .ok()
            .flatten()
            .is_some_and(|r| r.runtime_identity == self.identity)
        {
            let _ = std::fs::remove_file(record_path(&self.root));
        }
    }
}
pub async fn register(
    root: &Path,
    service: &RuntimeService,
    address: SocketAddr,
    assets: Option<&Path>,
    authenticated: bool,
) -> Result<Registration> {
    let identity = service.runtime().identity().await?;
    let endpoint = Endpoint {
        url: format!("http://{address}"),
        runtime_identity: identity.clone(),
        desktop_assets: match assets {
            Some(path) => Some(tokio::fs::canonicalize(path).await?),
            None => None,
        },
        authenticated,
    };
    let path = record_path(root);
    regular(&path)?;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut options = private_options();
    options.create_new(true);
    use std::io::Write;
    let result = (|| -> Result<()> {
        let mut file = options.open(&temporary)?;
        file.write_all(&serde_json::to_vec(&endpoint)?)?;
        file.sync_all()?;
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result?;
    Ok(Registration {
        root: root.to_owned(),
        identity,
    })
}
async fn request(
    endpoint: &Endpoint,
    method: &str,
    token: Option<&str>,
    target: bool,
) -> Result<Option<Value>> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|e| Error::Provider(e.to_string()))?;
    let mut body = json!({"jsonrpc":"2.0","id":1,"method":method,"params":{}});
    if target {
        body["expectedRuntime"] = json!(endpoint.runtime_identity);
    }
    let mut request = client.post(format!("{}/rpc", endpoint.url)).json(&body);
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = match request.send().await {
        Ok(response) => response,
        Err(error) if error.is_connect() => return Ok(None),
        Err(_) => {
            return Err(Error::Provider(
                "runtime is not responding; it has not been replaced or stopped".into(),
            ));
        }
    };
    if !response.status().is_success() {
        return Err(Error::Provider(format!(
            "runtime returned HTTP {}; attach with the required credentials",
            response.status()
        )));
    }
    let response: Value = response
        .json()
        .await
        .map_err(|_| Error::Provider("invalid runtime response".into()))?;
    if let Some(error) = response.get("error") {
        return Err(Error::Provider(
            error["message"]
                .as_str()
                .unwrap_or("runtime request failed")
                .into(),
        ));
    }
    Ok(Some(response.get("result").cloned().ok_or_else(|| {
        Error::Provider("runtime returned no result".into())
    })?))
}
async fn verified(endpoint: &Endpoint, token: Option<&str>) -> Result<bool> {
    let Some(value) = request(endpoint, "runtime/initialize", token, false).await? else {
        return Ok(false);
    };
    if value["name"] != "mpai"
        || value["protocolVersion"] != protocol::PROTOCOL_VERSION
        || value["runtimeIdentity"] != json!(endpoint.runtime_identity)
    {
        return Err(Error::Invalid(
            "runtime endpoint identity changed; refuse to attach or stop another owner".into(),
        ));
    }
    Ok(true)
}
async fn startup_lock(root: PathBuf) -> Result<std::fs::File> {
    tokio::task::spawn_blocking(move || {
        let path = root.join("runtime/desktop-start.lock");
        regular(&path)?;
        let mut options = private_options();
        options.read(true).truncate(false);
        let file = options.open(path)?;
        fs2::FileExt::lock_exclusive(&file)?;
        Ok(file)
    })
    .await
    .map_err(|e| Error::Provider(e.to_string()))?
}
fn detach(command: &mut Command) {
    #[cfg(unix)]
    unsafe {
        // The child leaves the launcher's session; only async-signal-safe work is done after fork.
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    #[cfg(windows)]
    command.creation_flags(0x00000008 | 0x00000200);
}
async fn wait_ready(
    root: &Path,
    identity: &LocalStoreIdentity,
    child: &mut Child,
) -> Result<Endpoint> {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if let Some(status) = child.try_wait()? {
                return Err(Error::Provider(format!(
                    "runtime startup exited with {status}; inspect the runtime log"
                )));
            }
            if let Some(endpoint) = read(root)? {
                if endpoint.runtime_identity.identity != *identity {
                    return Err(Error::Invalid(
                        "runtime belongs to a different retained Environment".into(),
                    ));
                }
                if verified(&endpoint, None).await? {
                    return Ok(endpoint);
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .map_err(|_| Error::Timeout("runtime startup; inspect the runtime log".into()))?
}
// Cancellation during startup must not leave an unpublished owner behind.
struct StartingOwner {
    child: Child,
    armed: bool,
}
impl Drop for StartingOwner {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.child.start_kill();
        }
    }
}
pub async fn attach(root: &Path) -> Result<Endpoint> {
    // Bootstrap runs before the database is opened, so simultaneous first launches serialize too.
    for directory in [root.to_owned(), root.join("runtime")] {
        if let Ok(meta) = std::fs::symlink_metadata(&directory)
            && (!meta.is_dir() || meta.file_type().is_symlink())
        {
            return Err(Error::Storage(
                "application directory must be a real directory".into(),
            ));
        }
        std::fs::create_dir_all(&directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    let _startup = startup_lock(root.to_owned()).await?;
    let store = storage::SqliteSessionStore::new(root).await?;
    let identity = store.identity().clone();
    if let Some(endpoint) = read(root)? {
        if endpoint.runtime_identity.identity != identity {
            return Err(Error::Invalid(
                "runtime record belongs to a different Environment".into(),
            ));
        }
        if verified(&endpoint, None).await? {
            return Ok(endpoint);
        }
    }
    // Do not turn an unavailable connection into a competing execution owner.
    let path = root.join("runtime/runtime-owner.lock");
    regular(&path)?;
    let mut options = private_options();
    options.read(true).truncate(false);
    let owner = options.open(path)?;
    fs2::FileExt::try_lock_exclusive(&owner).map_err(|_| Error::Storage("an existing runtime owns this Environment; reconnect to it instead of starting another".into()))?;
    fs2::FileExt::unlock(&owner)?;
    let log = root.join("runtime/daemon.log");
    regular(&log)?;
    let mut options = private_options();
    options.append(true);
    let output = options.open(log)?;
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("--state-dir")
        .arg(root)
        .args(["serve", "--http", "127.0.0.1:0", "--workbench"])
        .arg("--git-worktrees")
        .arg(root.join("worktrees"))
        .arg("--resources-root")
        .arg(root.join("resources"))
        .env_remove("MPAI_TOKEN")
        .stdin(Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(output);
    detach(&mut command);
    let mut child = StartingOwner {
        child: command.spawn()?,
        armed: true,
    };
    let endpoint = match wait_ready(root, &identity, &mut child.child).await {
        Ok(endpoint) => endpoint,
        Err(error) => {
            let _ = child.child.kill().await;
            let _ = child.child.wait().await;
            return Err(error);
        }
    };
    child.armed = false;
    Ok(endpoint)
}
pub async fn run(root: &Path, command: RuntimeCommand) -> Result<()> {
    let (token, stop) = match command {
        RuntimeCommand::Status { token } => (token, false),
        RuntimeCommand::Stop { token } => (token, true),
    };
    let Some(endpoint) = read(root)? else {
        println!("{}", json!({"running":false}));
        return Ok(());
    };
    let store = storage::SqliteSessionStore::new(root).await?;
    if endpoint.runtime_identity.identity != *store.identity() {
        return Err(Error::Invalid(
            "runtime record belongs to a different Environment".into(),
        ));
    }
    if !verified(&endpoint, token.as_deref()).await? {
        println!(
            "{}",
            json!({"running":false,"lastRuntimeIdentity":endpoint.runtime_identity})
        );
        return Ok(());
    }
    if !stop {
        println!("{}", json!({"running":true,"endpoint":endpoint}));
        return Ok(());
    }
    request(&endpoint, "runtime/shutdown", token.as_deref(), true)
        .await?
        .ok_or(Error::Closed)?;
    tokio::time::timeout(Duration::from_secs(10), async {
        while verified(&endpoint, token.as_deref()).await? {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let path = root.join("runtime/runtime-owner.lock");
        regular(&path)?;
        let file = std::fs::File::open(path)?;
        loop {
            match fs2::FileExt::try_lock_exclusive(&file) {
                Ok(()) => {
                    fs2::FileExt::unlock(&file)?;
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(error) => return Err(error.into()),
            }
        }
        Ok::<_, Error>(())
    })
    .await
    .map_err(|_| Error::Timeout("runtime is still stopping".into()))??;
    println!(
        "{}",
        json!({"stopped":true,"runtimeIdentity":endpoint.runtime_identity})
    );
    Ok(())
}
