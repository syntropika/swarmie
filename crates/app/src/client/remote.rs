//! SSH readiness checks and explicit registration; runtime lifecycle remains independent.
use crate::node_connection::{ConnectionManager, public_connection, rpc};
use clap::Subcommand;
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use storage::NodeTransport;
use swarmie::{Error, Result};

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
    /// Check SSH access, runtime installation and protocol readiness without changing the host.
    InspectSsh {
        host: String,
        #[arg(long, default_value = "")]
        socket: String,
        #[arg(long, env = "SWARMIE_TOKEN")]
        token: Option<String>,
    },
    /// Explicitly install or start the default remote runtime, then verify it.
    PrepareSsh {
        host: String,
        #[arg(long, value_parser = ["install", "start"])]
        action: String,
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
    let transport = NodeTransport::Ssh {
        host: host.into(),
        socket_path: socket.into(),
    };
    let endpoint = transport.endpoint();
    crate::node_channel::ssh_route(&endpoint)?;
    Ok(endpoint)
}

fn ssh_readiness(
    status: &str,
    installed: Option<bool>,
    version: Option<&str>,
    target: Option<swarmie::RuntimeIdentity>,
) -> Value {
    let next_action = match status {
        "ready" => "connect",
        "not-installed" => "install-runtime",
        "ssh-client-unavailable" => "install-ssh-client",
        "ssh-unavailable" => "configure-ssh-access",
        "invalid-installation" | "incompatible-runtime" => "verify-runtime-installation",
        _ => "verify-runtime-service-and-credentials",
    };
    let mut value = json!({"status":status,"installed":installed,"ready":status == "ready","nextAction":next_action});
    if let Some(version) = version {
        value["version"] = version.into();
    }
    if let Some(target) = target {
        value["runtimeIdentity"] = json!(target);
    }
    value
}

fn missing_runtime(shell: super::ssh_shell::Shell, code: Option<i32>, bytes: &[u8]) -> bool {
    // A Windows default PowerShell can normalize the nested process exit to 1.
    code == Some(127)
        || (shell == super::ssh_shell::Shell::Windows
            && code.is_some_and(|code| code != 0 && code != 255)
            && bytes == b"swarmie-runtime-not-installed")
}

pub(crate) async fn inspect_ssh(host: &str, socket: &str, token: Option<&str>) -> Result<Value> {
    use tokio::io::AsyncReadExt;
    let endpoint = ssh_endpoint(host, socket)?;
    let mut command = crate::node_channel::ssh_command(host)?;
    let shell = match super::ssh_shell::discover(host).await {
        Ok(shell) => shell,
        Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ssh_readiness("ssh-client-unavailable", None, None, None));
        }
        Err(_) => return Ok(ssh_readiness("ssh-unavailable", None, None, None)),
    };
    command
        .arg(shell.runtime(&["--version".into()]))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ssh_readiness("ssh-client-unavailable", None, None, None));
        }
        Err(error) => return Err(error.into()),
    };
    // Bound both response size and lifetime without retaining SSH diagnostic output.
    let output = tokio::time::timeout(Duration::from_secs(8), async {
        let mut bytes = Vec::new();
        child
            .stdout
            .take()
            .ok_or(Error::Closed)?
            .take(257)
            .read_to_end(&mut bytes)
            .await?;
        if bytes.len() > 256 {
            child.kill().await?;
            return Ok::<_, Error>((None, bytes));
        }
        Ok((Some(child.wait().await?), bytes))
    })
    .await;
    let (exit, bytes) = match output {
        Ok(result) => result?,
        Err(_) => {
            let _ = child.kill().await;
            return Ok(ssh_readiness("ssh-unavailable", None, None, None));
        }
    };
    let Some(exit) = exit else {
        return Ok(ssh_readiness("invalid-installation", None, None, None));
    };
    if missing_runtime(shell, exit.code(), &bytes) {
        let mut result = ssh_readiness("not-installed", Some(false), None, None);
        result["preparation"] =
            crate::client::ssh_preparation::preparation(host, socket, true).await;
        return Ok(result);
    }
    if exit.code() == Some(255) {
        return Ok(ssh_readiness("ssh-unavailable", None, None, None));
    }
    if !exit.success() {
        return Ok(ssh_readiness("invalid-installation", None, None, None));
    }
    let version = String::from_utf8_lossy(&bytes);
    let version = version.trim();
    if version
        .strip_prefix("swarmie ")
        .is_none_or(|version| semver::Version::parse(version).is_err())
    {
        return Ok(ssh_readiness("invalid-installation", None, None, None));
    }
    // Keep the complete read-only check within the client request deadline.
    let result: Result<Value> = match tokio::time::timeout(
        Duration::from_secs(15),
        rpc(&endpoint, "runtime/initialize", json!({}), token),
    )
    .await
    {
        Ok(Ok(value)) => match crate::node_connection::handshake(&value, None) {
            Ok(target) => Ok(ssh_readiness(
                "ready",
                Some(true),
                Some(version),
                Some(target),
            )),
            Err(_) => Ok(ssh_readiness(
                "incompatible-runtime",
                Some(true),
                Some(version),
                None,
            )),
        },
        Ok(Err(Error::Invalid(_) | Error::Json(_))) => Ok(ssh_readiness(
            "incompatible-runtime",
            Some(true),
            Some(version),
            None,
        )),
        _ => Ok(ssh_readiness(
            "runtime-unavailable",
            Some(true),
            Some(version),
            None,
        )),
    };
    let mut result = result?;
    if result["status"] == "runtime-unavailable" {
        result["preparation"] =
            crate::client::ssh_preparation::preparation(host, socket, false).await;
    }
    Ok(result)
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
        RemoteCommand::Pair { name, url, token } => println!(
            "{}",
            public_connection(&manager.pair(name, url, token).await?)
        ),
        RemoteCommand::Call {
            name,
            method,
            params,
        } => println!(
            "{}",
            serde_json::to_string_pretty(
                &manager
                    .call(&name, &method, serde_json::from_str(&params)?)
                    .await?
            )?
        ),
        RemoteCommand::PrepareSsh { host, action } => println!(
            "{}",
            crate::client::ssh_preparation::prepare(&host, "", &action).await?
        ),
        RemoteCommand::InspectSsh {
            host,
            socket,
            token,
        } => println!("{}", inspect_ssh(&host, &socket, token.as_deref()).await?),
        RemoteCommand::Ssh {
            name,
            host,
            socket,
            token,
        } => {
            let readiness = inspect_ssh(&host, &socket, token.as_deref()).await?;
            if readiness["ready"] != true {
                return Err(Error::Invalid(format!(
                    "remote runtime is not ready: {}",
                    readiness["nextAction"]
                )));
            }
            let endpoint = ssh_endpoint(&host, &socket)?;
            let target = serde_json::from_value(readiness["runtimeIdentity"].clone())?;
            println!(
                "{}",
                public_connection(
                    &manager
                        .pair_prepared(name, endpoint, token, &target)
                        .await?
                )
            );
        }
    }
    manager.close().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{super::ssh_shell::Shell, missing_runtime};
    #[test]
    fn missing_runtime_requires_the_exact_windows_marker_or_posix_status() {
        assert!(missing_runtime(
            Shell::Windows,
            Some(1),
            b"swarmie-runtime-not-installed"
        ));
        assert!(missing_runtime(Shell::Posix, Some(127), b""));
        assert!(!missing_runtime(
            Shell::Posix,
            Some(1),
            b"swarmie-runtime-not-installed"
        ));
        assert!(!missing_runtime(
            Shell::Windows,
            Some(0),
            b"swarmie-runtime-not-installed"
        ));
        assert!(!missing_runtime(
            Shell::Windows,
            Some(255),
            b"swarmie-runtime-not-installed"
        ));
        assert!(!missing_runtime(
            Shell::Windows,
            Some(1),
            b"unexpected error"
        ));
    }
}
