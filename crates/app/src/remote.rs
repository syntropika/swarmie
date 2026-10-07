use clap::Subcommand;
use futures_util::{SinkExt, StreamExt};
use mpai::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, time::Duration};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

#[derive(Subcommand)]
pub enum RemoteCommand {
    /// Verify and save a daemon connection; bearer credentials are stored privately.
    Pair {
        name: String,
        url: String,
        #[arg(long, env = "MPAI_TOKEN")]
        token: Option<String>,
    },
    /// Show connection metadata without credentials.
    List,
    Remove {
        name: String,
    },
    /// Send an authenticated request to a saved connection.
    Call {
        name: String,
        method: String,
        #[arg(long, default_value = "{}")]
        params: String,
    },
    /// Keep an SSH loopback tunnel open until interrupted.
    Ssh {
        name: String,
        host: String,
        #[arg(long, default_value_t = 7490)]
        local_port: u16,
        #[arg(long, default_value_t = 7480)]
        remote_port: u16,
        #[arg(long, env = "MPAI_TOKEN")]
        token: Option<String>,
        /// Recreate dropped forwards with cancellable backoff, retaining the local port.
        #[arg(long)]
        reconnect: bool,
    },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Connection {
    name: String,
    url: String,
    token: Option<String>,
    ssh_host: Option<String>,
    created_at_ms: u64,
}
impl Connection {
    fn public(&self) -> Value {
        json!({"name":self.name,"url":self.url,"sshHost":self.ssh_host,"createdAtMs":self.created_at_ms,"authenticated":self.token.is_some()})
    }
}
fn path(root: &Path) -> std::path::PathBuf {
    root.parent().unwrap_or(root).join("connections.json")
}
async fn read(root: &Path) -> Result<BTreeMap<String, Connection>> {
    match tokio::fs::read(path(root)).await {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(error) => Err(error.into()),
    }
}
async fn save(root: &Path, connections: &BTreeMap<String, Connection>) -> Result<()> {
    let path = path(root);
    tokio::fs::create_dir_all(path.parent().unwrap()).await?;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    use tokio::io::AsyncWriteExt;
    let mut file = options.open(&temporary).await?;
    file.write_all(&serde_json::to_vec_pretty(connections)?)
        .await?;
    file.sync_all().await?;
    drop(file);
    tokio::fs::rename(temporary, path).await?;
    Ok(())
}
fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 100 || name.chars().any(char::is_control) {
        Err(Error::Invalid(
            "connection name must contain 1 to 100 printable characters".into(),
        ))
    } else {
        Ok(())
    }
}
fn endpoint(url: &str) -> Result<reqwest::Url> {
    let mut parsed =
        reqwest::Url::parse(url).map_err(|_| Error::Invalid("invalid daemon URL".into()))?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(Error::Invalid(
            "daemon URL must not contain credentials".into(),
        ));
    }
    if !matches!(parsed.scheme(), "http" | "https" | "ws" | "wss") {
        return Err(Error::Invalid(
            "daemon URL requires HTTP(S) or WS(S)".into(),
        ));
    }
    if matches!(parsed.scheme(), "http" | "https") && parsed.path() == "/" {
        parsed.set_path("/rpc");
    }
    Ok(parsed)
}
pub async fn rpc(url: &str, method: &str, params: Value, token: Option<&str>) -> Result<Value> {
    let parsed = endpoint(url)?;
    let request = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
    let response = match parsed.scheme() {
        "http" | "https" => {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .map_err(|error| Error::Provider(error.to_string()))?;
            let mut request = client.post(parsed).json(&request);
            if let Some(token) = token {
                request = request.bearer_auth(token);
            }
            let response = request
                .send()
                .await
                .map_err(|error| Error::Provider(error.to_string()))?;
            if !response.status().is_success() {
                return Err(Error::Provider(format!(
                    "daemon HTTP status {}",
                    response.status()
                )));
            }
            response
                .json::<Value>()
                .await
                .map_err(|error| Error::Provider(error.to_string()))?
        }
        "ws" | "wss" => {
            let mut connection = url
                .into_client_request()
                .map_err(|error| Error::Invalid(error.to_string()))?;
            if let Some(token) = token {
                connection.headers_mut().insert(
                    "authorization",
                    format!("Bearer {token}")
                        .parse()
                        .map_err(|_| Error::Invalid("invalid bearer token".into()))?,
                );
            }
            let (mut socket, _) = tokio::time::timeout(
                Duration::from_secs(15),
                tokio_tungstenite::connect_async(connection),
            )
            .await
            .map_err(|_| Error::Timeout("remote connection".into()))?
            .map_err(|error| Error::Provider(error.to_string()))?;
            socket
                .send(Message::Text(request.to_string().into()))
                .await
                .map_err(|error| Error::Provider(error.to_string()))?;
            let response = tokio::time::timeout(Duration::from_secs(15), async {
                while let Some(frame) = socket.next().await {
                    let frame = frame.map_err(|error| Error::Provider(error.to_string()))?;
                    if let Message::Text(text) = frame {
                        let response: Value = serde_json::from_str(&text)?;
                        if response["id"] == 1 {
                            return Ok(response);
                        }
                    }
                }
                Err(Error::Closed)
            })
            .await
            .map_err(|_| Error::Timeout("remote request".into()))??;
            let _ = socket.close(None).await;
            response
        }
        _ => {
            return Err(Error::Invalid(
                "daemon URL requires HTTP(S) or WS(S)".into(),
            ));
        }
    };
    if let Some(error) = response.get("error") {
        return Err(Error::Provider(
            error["message"]
                .as_str()
                .unwrap_or("remote RPC failed")
                .into(),
        ));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| Error::Provider("daemon returned no RPC result".into()))
}
pub async fn run(root: &Path, command: RemoteCommand) -> Result<()> {
    let mut connections = read(root).await?;
    match command {
        RemoteCommand::List => println!(
            "{}",
            serde_json::to_string_pretty(
                &connections
                    .values()
                    .map(Connection::public)
                    .collect::<Vec<_>>()
            )?
        ),
        RemoteCommand::Remove { name } => {
            if connections.remove(&name).is_none() {
                return Err(Error::Invalid("unknown connection".into()));
            }
            save(root, &connections).await?;
        }
        RemoteCommand::Pair { name, url, token } => {
            validate_name(&name)?;
            let url = endpoint(&url)?.to_string();
            let initialized = rpc(&url, "runtime/initialize", json!({}), token.as_deref()).await?;
            if initialized["name"] != "mpai" {
                return Err(Error::Invalid("endpoint is not an mpai daemon".into()));
            }
            let connection = Connection {
                name: name.clone(),
                url,
                token,
                ssh_host: None,
                created_at_ms: now(),
            };
            println!("{}", connection.public());
            connections.insert(name, connection);
            save(root, &connections).await?;
        }
        RemoteCommand::Call {
            name,
            method,
            params,
        } => {
            let connection = connections
                .get(&name)
                .ok_or_else(|| Error::Invalid("unknown connection".into()))?;
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &rpc(
                        &connection.url,
                        &method,
                        serde_json::from_str(&params)?,
                        connection.token.as_deref()
                    )
                    .await?
                )?
            );
        }
        RemoteCommand::Ssh {
            name,
            host,
            local_port,
            remote_port,
            token,
            reconnect,
        } => {
            validate_name(&name)?;
            if host.is_empty()
                || host.starts_with('-')
                || host.chars().any(|c| c.is_whitespace() || c.is_control())
                || local_port == 0
                || remote_port == 0
            {
                return Err(Error::Invalid("invalid SSH host or port".into()));
            }
            supervise_ssh(root, name, host, local_port, remote_port, token, reconnect).await?;
        }
    }
    Ok(())
}
fn retry_delay(failures: u32) -> Duration {
    Duration::from_millis((500u64.saturating_mul(1u64 << failures.min(6))).min(30_000))
}
fn receipt(
    connection: &Connection,
    event: &str,
    attempt: u64,
    pid: Option<u32>,
    delay: Option<Duration>,
) {
    let mut receipt = connection.public();
    receipt["event"] = event.into();
    receipt["attempt"] = attempt.into();
    receipt["timestampMs"] = now().into();
    if let Some(pid) = pid {
        receipt["sshPid"] = pid.into();
    }
    if let Some(delay) = delay {
        receipt["retryAfterMs"] = (delay.as_millis() as u64).into();
    }
    println!("{receipt}");
}
async fn stop_ssh(child: &mut tokio::process::Child) {
    let _ = child.kill().await;
    let _ = child.wait().await;
}
async fn supervise_ssh(
    root: &Path,
    name: String,
    host: String,
    local_port: u16,
    remote_port: u16,
    token: Option<String>,
    reconnect: bool,
) -> Result<()> {
    let connection = Connection {
        name: name.clone(),
        url: format!("http://127.0.0.1:{local_port}/rpc"),
        token,
        ssh_host: Some(host.clone()),
        created_at_ms: now(),
    };
    let forward = format!("127.0.0.1:{local_port}:127.0.0.1:{remote_port}");
    let cancelled = tokio::signal::ctrl_c();
    tokio::pin!(cancelled);
    // Poll before spawning so Ctrl-C has a handler throughout initial connection and later retries.
    tokio::select! { biased; result=&mut cancelled=>{result?;return Ok(());},_=std::future::ready(())=>{} }
    let mut failures = 0u32;
    let mut attempt = 0u64;
    loop {
        // Cancellation wins even when a retry timer and signal become ready together.
        tokio::select! { biased; result=&mut cancelled=>{result?;receipt(&connection,"ssh.stopped",attempt,None,None);return Ok(());},_=std::future::ready(())=>{} }
        attempt += 1;
        let mut child = tokio::process::Command::new("ssh")
            .args([
                "-N",
                "-o",
                "ExitOnForwardFailure=yes",
                "-o",
                "ServerAliveInterval=30",
                "-o",
                "ServerAliveCountMax=3",
                "-o",
                "ControlMaster=no",
                "-o",
                "ControlPath=none",
                "-o",
                "ForkAfterAuthentication=no",
                "-L",
                &forward,
                &host,
            ])
            .kill_on_drop(true)
            .spawn()?;
        let pid = child.id();
        let verified = tokio::select! {
            biased;
            result=&mut cancelled=>{result?;stop_ssh(&mut child).await;receipt(&connection,"ssh.stopped",attempt,pid,None);return Ok(());}
            result=tokio::time::timeout(Duration::from_secs(15),async {
                loop {
                    if let Some(status)=child.try_wait()? { return Err(Error::Provider(format!("SSH tunnel exited: {status}"))); }
                    match rpc(&connection.url,"runtime/initialize",json!({}),connection.token.as_deref()).await {
                        Ok(value)=>return Ok(value),
                        Err(_)=>tokio::time::sleep(Duration::from_millis(200)).await,
                    }
                }
            })=>result.map_err(|_|Error::Timeout("SSH daemon handshake".into())).and_then(|value|value),
        };
        match verified {
            Ok(verified) => {
                if verified["name"] != "mpai" {
                    stop_ssh(&mut child).await;
                    return Err(Error::Invalid("SSH target is not an mpai daemon".into()));
                }
                // Reload on each connection so a parallel pairing/removal is not replaced by stale state.
                tokio::select! {
                    biased;
                    result=&mut cancelled=>{result?;stop_ssh(&mut child).await;receipt(&connection,"ssh.stopped",attempt,pid,None);return Ok(());}
                    result=async { let mut connections=read(root).await?;connections.insert(name.clone(),connection.clone());save(root,&connections).await }=>result?,
                }
                receipt(&connection, "ssh.connected", attempt, pid, None);
                let connected_at = tokio::time::Instant::now();
                let status = tokio::select! {
                    biased;
                    result=&mut cancelled=>{result?;stop_ssh(&mut child).await;receipt(&connection,"ssh.stopped",attempt,pid,None);return Ok(());}
                    status=child.wait()=>status?,
                };
                if connected_at.elapsed() >= Duration::from_secs(30) {
                    failures = 0;
                }
                if !reconnect {
                    return if status.success() {
                        Ok(())
                    } else {
                        Err(Error::Provider(format!("SSH tunnel exited: {status}")))
                    };
                }
                eprintln!("SSH forward disconnected ({status})");
            }
            Err(error) => {
                stop_ssh(&mut child).await;
                if !reconnect {
                    return Err(error);
                }
            }
        }
        receipt(&connection, "ssh.disconnected", attempt, pid, None);
        let delay = retry_delay(failures);
        failures = failures.saturating_add(1);
        receipt(&connection, "ssh.reconnecting", attempt, None, Some(delay));
        tokio::select! {
            biased;
            result=&mut cancelled=>{result?;receipt(&connection,"ssh.stopped",attempt,None,None);return Ok(());}
            _=tokio::time::sleep(delay)=>{},
        }
    }
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn pairing_credentials_are_private_and_redacted_from_list_metadata() {
        let root = tempfile::tempdir().unwrap();
        let mut map = BTreeMap::new();
        let connection = Connection {
            name: "fixture".into(),
            url: "http://127.0.0.1:1/rpc".into(),
            token: Some("fixture-secret".into()),
            ssh_host: None,
            created_at_ms: 1,
        };
        assert!(!connection.public().to_string().contains("fixture-secret"));
        map.insert("fixture".into(), connection);
        save(&root.path().join("sessions"), &map).await.unwrap();
        assert_eq!(
            read(&root.path().join("sessions")).await.unwrap()["fixture"]
                .token
                .as_deref(),
            Some("fixture-secret")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path(&root.path().join("sessions")))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
}
