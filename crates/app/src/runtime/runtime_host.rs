//! Local execution-owner discovery and attachment, independent of client lifetime.
#[cfg(windows)]
use crate::runtime::windows_spawn::Child;
use clap::Subcommand;
use protocol::RuntimeService;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[cfg(not(windows))]
use std::process::Stdio;
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};
use swarmie::{Error, LocalStoreIdentity, Result, RuntimeIdentity};
#[cfg(not(windows))]
use tokio::process::{Child, Command};

#[derive(Subcommand)]
pub enum RuntimeCommand {
    /// Start a detached headless owner or attach to the verified existing owner.
    Start,
    /// Transparently attach stdio to an existing runtime's local socket; never start an owner.
    Proxy {
        #[arg(long)]
        socket: Option<PathBuf>,
    },
    Status {
        #[arg(long, env = "SWARMIE_TOKEN")]
        token: Option<String>,
    },
    /// Stop the execution owner and its native processes; retained history is preserved.
    Stop {
        #[arg(long, env = "SWARMIE_TOKEN")]
        token: Option<String>,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Endpoint {
    pub url: String,
    pub local_socket: Option<PathBuf>,
    pub runtime_identity: RuntimeIdentity,
    pub desktop_assets: Option<PathBuf>,
    pub authenticated: bool,
}
/// Keep socket addresses within the smallest supported Unix address limit.
pub(crate) fn local_socket_path(root: &Path) -> Result<PathBuf> {
    let root = std::fs::canonicalize(root)?;
    #[cfg(not(windows))]
    let preferred = root.join("runtime/node.sock");
    #[cfg(unix)] {
        use std::os::unix::ffi::OsStrExt;
        if preferred.as_os_str().as_bytes().len() < 100 { return Ok(preferred); }
        use sha2::{Digest, Sha256};
        let digest = format!("{:x}", Sha256::digest(root.as_os_str().as_bytes()));
        let uid = unsafe { libc::geteuid() };
        let path = PathBuf::from("/tmp").join(format!("swarmie-{uid}")).join(format!("{}.sock", &digest[..24]));
        Ok(path)
    }
    #[cfg(windows)] {
        use sha2::{Digest, Sha256};
        let digest = format!("{:x}", Sha256::digest(root.as_os_str().to_string_lossy().as_bytes()));
        Ok(PathBuf::from(format!(r"\\.\pipe\swarmie-{}", &digest[..32])))
    }
    #[cfg(not(any(unix, windows)))] { Ok(preferred) }
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
    if let Some(socket) = &endpoint.local_socket {
        let expected = local_socket_path(root)?;
        if socket != &expected {
            return Err(Error::Invalid("runtime socket must belong to the retained Environment directory".into()));
        }
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
        local_socket: if cfg!(any(unix, windows)) { Some(local_socket_path(root)?) } else { None },
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
    #[cfg(unix)]
    if let Some(path) = &endpoint.local_socket {
        let transport = storage::NodeTransport::LocalSocket { path: path.to_string_lossy().into_owned() };
        match tokio::net::UnixStream::connect(path).await {
            Ok(stream) => drop(stream),
            Err(error) if matches!(error.kind(), std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound) => return Ok(None),
            Err(error) => return Err(error.into()),
        }
        let value = crate::node_connection::rpc_targeted(&transport.endpoint(), method, json!({}), None, if target { Some(&endpoint.runtime_identity) } else { None }).await;
        return match value {
            Ok(value) => Ok(Some(value)),
            Err(_) if !path.exists() => Ok(None),
            Err(error) => Err(error),
        };
    }
    #[cfg(windows)]
    if let Some(path) = &endpoint.local_socket {
        match crate::runtime::windows_pipe::connect(path).await {
            Ok(stream) => drop(stream),
            Err(error) if matches!(error.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused) => return Ok(None),
            Err(error) => return Err(error.into()),
        }
        let transport = storage::NodeTransport::LocalSocket { path: path.to_string_lossy().into_owned() };
        return crate::node_connection::rpc_targeted(&transport.endpoint(), method, json!({}), None,
            if target {Some(&endpoint.runtime_identity)} else {None}).await.map(Some);
    }
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
pub(super) async fn verified(endpoint: &Endpoint, token: Option<&str>) -> Result<bool> {
    let Some(value) = request(endpoint, "runtime/initialize", token, false).await? else {
        return Ok(false);
    };
    if value["name"] != "swarmie"
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
#[cfg(not(windows))]
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
fn transport_token(root: &Path) -> Result<PathBuf> {
    use std::io::Write;
    let path = root.join("runtime/transport.token");
    if !regular(&path)? {
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let mut options = private_options();
        options.create_new(true);
        let mut file = options.open(&path)?;
        file.write_all(token.as_bytes())?;
        file.sync_all()?;
    }
    if std::fs::metadata(&path)?.len() > 1024 {
        return Err(Error::Invalid(
            "runtime transport token exceeds the supported size".into(),
        ));
    }
    let token = std::fs::read_to_string(&path)?;
    if token.len() < 32 || !token.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(Error::Invalid("runtime transport token must contain at least 32 printable ASCII characters without whitespace".into()));
    }
    Ok(path)
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
    let token_file = transport_token(root)?;
    let executable = std::env::current_exe()?;
    let arguments: Vec<std::ffi::OsString> = vec![
        "--state-dir".into(),
        root.as_os_str().to_owned(),
        "serve".into(),
        "--http".into(),
        "127.0.0.1:0".into(),
        "--workbench".into(),
        "--token-file".into(),
        token_file.into_os_string(),
        "--git-worktrees".into(),
        root.join("worktrees").into_os_string(),
        "--resources-root".into(),
        root.join("resources").into_os_string(),
    ];
    #[cfg(windows)]
    let spawned = crate::runtime::windows_spawn::owner(&executable, &arguments, &output)?;
    #[cfg(not(windows))]
    let spawned = {
        let mut command = Command::new(executable);
        command
            .args(arguments)
            .env_remove("SWARMIE_TOKEN")
            .stdin(Stdio::null())
            .stdout(output.try_clone()?)
            .stderr(output);
        detach(&mut command);
        command.spawn()?
    };
    let mut child = StartingOwner {
        child: spawned,
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
    if matches!(command, RuntimeCommand::Start) {
        let endpoint = attach(root).await?;
        println!("{}", json!({"running":true,"runtimeIdentity":endpoint.runtime_identity}));
        return Ok(());
    }
    if let RuntimeCommand::Proxy { socket } = command {
        return proxy(match socket { Some(path) => path, None => local_socket_path(root)? }).await;
    }
    let (token, stop) = match command {
        RuntimeCommand::Status { token } => (token, false),
        RuntimeCommand::Stop { token } => (token, true),
        RuntimeCommand::Proxy { .. } | RuntimeCommand::Start => unreachable!(),
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
        // The shutdown acknowledgment is authoritative. The owner lock confirms
        // process teardown without probing listeners that are already closing.
        let path = root.join("runtime/runtime-owner.lock");
        regular(&path)?;
        let file = std::fs::File::open(path)?;
        loop {
            match fs2::FileExt::try_lock_exclusive(&file) {
                Ok(()) => {
                    fs2::FileExt::unlock(&file)?;
                    break;
                }
                Err(error) if error.raw_os_error() == fs2::lock_contended_error().raw_os_error() => {
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

/// Closing this byte proxy closes only its connection, not runtime-owned sessions.
async fn proxy(path: PathBuf) -> Result<()> {
    #[cfg(any(unix, windows))]
    {
        #[cfg(unix)]
        let socket = tokio::net::UnixStream::connect(&path).await.map_err(|_| {
            Error::Provider(
                "runtime socket unavailable; start the installed runtime explicitly".into(),
            )
        })?;
        #[cfg(windows)]
        let socket = crate::runtime::windows_pipe::connect(&path).await?;
        use tokio::io::AsyncWriteExt;
        let (mut reader, mut writer) = tokio::io::split(socket);
        let upstream = async {
            tokio::io::copy(&mut tokio::io::stdin(), &mut writer).await?;
            // macOS can report ENOTCONN when the peer already closed after a clean EOF.
            match writer.shutdown().await {
                Err(error) if error.kind() == std::io::ErrorKind::NotConnected => Ok(()),
                result => result,
            }
        };
        let downstream = async {
            let mut output = tokio::io::stdout();
            match tokio::io::copy(&mut reader, &mut output).await {
                // The client may close its output pipe before the peer's WebSocket close reply.
                Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
                result => {
                    result?;
                    output.shutdown().await
                }
            }
        };
        tokio::try_join!(upstream, downstream)?;
        Ok(())
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Err(Error::Unsupported(
            "runtime socket proxy is unavailable on this platform".into(),
        ))
    }
}
