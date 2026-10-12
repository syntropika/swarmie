use crate::node_connection::{
    ConnectionManager, handshake, now, public_connection, rpc, validate_name,
};
use clap::Subcommand;
use mpai::{Error, Result};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use storage::NodeTransport;

#[derive(Subcommand)]
pub enum RemoteCommand {
    /// Verify and register a node connection; bearer credentials are stored separately.
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
pub async fn run(root: &Path, command: RemoteCommand) -> Result<()> {
    let manager = ConnectionManager::open(root).await?;
    match command {
        RemoteCommand::List => println!(
            "{}",
            serde_json::to_string_pretty(
                &manager
                    .list()
                    .await?
                    .iter()
                    .map(public_connection)
                    .collect::<Vec<_>>()
            )?
        ),
        RemoteCommand::Remove { name } => manager.remove(&name).await?,
        RemoteCommand::Pair { name, url, token } => {
            let record = manager.pair(name, url, token).await?;
            println!("{}", public_connection(&record));
        }
        RemoteCommand::Call {
            name,
            method,
            params,
        } => {
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &manager
                        .call(&name, &method, serde_json::from_str(&params)?)
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
    connection: &Value,
    event: &str,
    attempt: u64,
    pid: Option<u32>,
    delay: Option<Duration>,
) {
    let mut receipt = connection.clone();
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
    let manager = ConnectionManager::open(root).await?;
    let previous = manager.find(&name).await?;
    let mut pinned = previous.as_ref().map(|c| c.identity.clone());
    let mut revision = previous.as_ref().map_or(0, |c| c.revision);
    let url = format!("http://127.0.0.1:{local_port}/rpc");
    let created_at_ms = now();
    let mut connection = json!({"name":name,"url":url,"sshHost":host,"createdAtMs":created_at_ms,"authenticated":token.is_some()});
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
                    match rpc(&url,"runtime/initialize",json!({}),token.as_deref()).await {
                        Ok(value)=>return Ok(value),
                        Err(_)=>tokio::time::sleep(Duration::from_millis(200)).await,
                    }
                }
            })=>result.map_err(|_|Error::Timeout("SSH daemon handshake".into())).and_then(|value|value),
        };
        match verified {
            Ok(verified) => {
                let target = match handshake(&verified, pinned.as_ref()) {
                    Ok(target) => target,
                    Err(error) => {
                        stop_ssh(&mut child).await;
                        return Err(error);
                    }
                };
                pinned = Some(target.identity.clone());
                connection["identity"] = serde_json::to_value(&target.identity)?;
                connection["runtimeIncarnationId"] = target.incarnation_id.clone().into();
                let saved = tokio::select! {
                    biased;
                    result=&mut cancelled=>{result?;stop_ssh(&mut child).await;receipt(&connection,"ssh.stopped",attempt,pid,None);return Ok(());}
                    result=manager.save_verified(name.clone(),NodeTransport::Ssh {host:host.clone(),local_port,remote_port},&target,token.as_deref(),revision)=>match result {
                        Ok(saved)=>saved,
                        Err(error)=>{stop_ssh(&mut child).await;return Err(error);},
                    },
                };
                revision = saved.revision;
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
