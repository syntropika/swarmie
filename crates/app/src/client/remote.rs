//! SSH readiness checks and explicit registration; runtime lifecycle remains independent.
use crate::node_connection::{ConnectionManager, public_connection, rpc};
use clap::Subcommand;
use swarmie::{Error, Result};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use storage::NodeTransport;

#[derive(Subcommand)]
pub enum RemoteCommand {
    /// Verify and register a node connection; bearer credentials are stored separately.
    Pair {
        name: String,
        url: String,
        #[arg(long, env = "SWARMIE_TOKEN")]
        token: Option<String>,
    },
    /// Show connection metadata without credentials.
    List,
    Remove { name: String },
    /// Send an authenticated request to a saved connection.
    Call {
        name: String,
        method: String,
        #[arg(long, default_value = "{}")]
        params: String,
    },
    /// Check SSH access, runtime installation and protocol readiness without changing the host.
    InspectSsh {
        host: String,
        #[arg(long, default_value = "")]
        socket: String,
        #[arg(long, env = "SWARMIE_TOKEN")]
        token: Option<String>,
    },
    /// Register an existing remote runtime through an SSH stdio proxy. The native client reconnects it.
    Ssh {
        name: String,
        host: String,
        #[arg(long, default_value = "")]
        socket: String,
        #[arg(long, env = "SWARMIE_TOKEN")]
        token: Option<String>,
    },
}

pub(crate) fn ssh_endpoint(host: &str, socket: &str) -> Result<String> {
    let transport = NodeTransport::Ssh { host: host.into(), socket_path: socket.into() };
    let endpoint = transport.endpoint();
    crate::node_channel::ssh_route(&endpoint)?;
    Ok(endpoint)
}

pub(crate) async fn inspect_ssh(host: &str, socket: &str, token: Option<&str>) -> Result<Value> {
    let endpoint = ssh_endpoint(host, socket)?;
    let mut command = crate::node_channel::ssh_command(host)?;
    command.arg("command -v swarmie >/dev/null 2>&1 || exit 127; swarmie --version")
        .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
    let output = tokio::time::timeout(Duration::from_secs(8), command.output()).await
        .map_err(|_| Error::Timeout("SSH readiness check".into()))??;
    if output.status.code() == Some(127) {
        return Ok(json!({"installed":false,"ready":false,"nextAction":"install-runtime"}));
    }
    if !output.status.success() {
        return Err(Error::Provider("SSH authentication or runtime installation check failed; configure SSH access explicitly".into()));
    }
    let version = String::from_utf8_lossy(&output.stdout);
    let version = version.trim();
    if version.len() > 256 { return Err(Error::Provider("invalid runtime version response".into())); }
    match rpc(&endpoint, "runtime/initialize", json!({}), token).await {
        Ok(value) => {
            let target = crate::node_connection::handshake(&value, None)?;
            Ok(json!({"installed":true,"ready":true,"version":version,"runtimeIdentity":target}))
        }
        Err(_) => Ok(json!({"installed":true,"ready":false,"version":version,"nextAction":"verify-runtime-service-and-credentials"})),
    }
}

pub async fn run(root: &Path, command: RemoteCommand) -> Result<()> {
    let manager = ConnectionManager::open(root).await?;
    match command {
        RemoteCommand::List => println!("{}", serde_json::to_string_pretty(&manager.list().await?.iter().map(public_connection).collect::<Vec<_>>())?),
        RemoteCommand::Remove { name } => manager.remove(&name).await?,
        RemoteCommand::Pair { name, url, token } => println!("{}", public_connection(&manager.pair(name, url, token).await?)),
        RemoteCommand::Call { name, method, params } => println!("{}", serde_json::to_string_pretty(&manager.call(&name, &method, serde_json::from_str(&params)?).await?)?),
        RemoteCommand::InspectSsh { host, socket, token } => println!("{}", inspect_ssh(&host, &socket, token.as_deref()).await?),
        RemoteCommand::Ssh { name, host, socket, token } => {
            let readiness = inspect_ssh(&host, &socket, token.as_deref()).await?;
            if readiness["ready"] != true {
                return Err(Error::Invalid(format!("remote runtime is not ready: {}", readiness["nextAction"])));
            }
            let endpoint = ssh_endpoint(&host, &socket)?;
            let target = serde_json::from_value(readiness["runtimeIdentity"].clone())?;
            println!("{}", public_connection(&manager.pair_prepared(name, endpoint, token, &target).await?));
        }
    }
    manager.close().await;
    Ok(())
}
