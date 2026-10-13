//! Explicit user-owned SSH preparation; inspection and proxy attachment never install or start.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use swarmie::{Error, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const SCRIPT: &str = include_str!("../../../../scripts/distribution/prepare-ssh.py");
const LIMIT: u64 = 512 * 1024 * 1024;

fn manifest() -> String {
    std::env::var("SWARMIE_RUNTIME_MANIFEST")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            option_env!("SWARMIE_RUNTIME_MANIFEST")
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        })
        .unwrap_or_else(crate::update::runtime_manifest_url)
}
fn packages() -> Option<PathBuf> {
    std::env::var_os("SWARMIE_RUNTIME_PACKAGES")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::current_exe()
                .ok()?
                .parent()
                .map(|p| p.join("runtime-packages"))
        })
}
fn local_package(platform: &str) -> Option<PathBuf> {
    packages()
        .map(|p| p.join(format!("{platform}.tar.gz")))
        .filter(|p| p.is_file())
}
async fn execute(
    host: &str,
    remote: String,
    archive: Option<&Path>,
    timeout: Duration,
) -> Result<Value> {
    let mut command = crate::node_channel::ssh_command(host)?;
    command
        .arg(remote)
        .stdin(if archive.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn()?;
    let mut output = child.stdout.take().ok_or(Error::Closed)?;
    let input = child.stdin.take();
    let operation = async {
        let writer = async {
            if let Some(path) = archive {
                let mut file = tokio::fs::File::open(path).await?;
                let mut stdin = input.ok_or(Error::Closed)?;
                let count = tokio::io::copy(&mut (&mut file).take(LIMIT + 1), &mut stdin).await?;
                if count > LIMIT {
                    return Err(Error::Invalid("runtime package exceeds 512 MiB".into()));
                }
                stdin.shutdown().await?;
            }
            Ok::<_, Error>(())
        };
        let reader = async {
            let mut bytes = Vec::new();
            (&mut output).take(8193).read_to_end(&mut bytes).await?;
            if bytes.len() > 8192 {
                return Err(Error::Invalid("invalid SSH preparation response".into()));
            }
            Ok::<_, Error>(bytes)
        };
        let (_, bytes) = tokio::try_join!(writer, reader)?;
        let status = child.wait().await?;
        let result: Value = serde_json::from_slice(&bytes).map_err(|_| {
            Error::Provider(
                "SSH preparation failed; check remote access and Python 3.9 or later".into(),
            )
        })?;
        if !status.success() || result.get("error").is_some() {
            let reason = result["error"].as_str().unwrap_or("preparation-failed");
            // Return only known bootstrap codes, never arbitrary remote output.
            let message = match reason {
                "installation-refused" => {
                    "installation refused; preserve existing files and check the managed installation"
                }
                "installation-changed" => "installation changed; check the connection again",
                "checksum-mismatch" => "transferred package checksum mismatch",
                "invalid-package" => "invalid runtime package",
                "platform-changed" | "unsupported-platform" => {
                    "unsupported or changed remote platform"
                }
                _ => "remote preparation failed",
            };
            return Err(Error::Provider(message.into()));
        }
        Ok(result)
    };
    tokio::time::timeout(timeout, operation).await.map_err(|_| Error::Timeout(
        "SSH preparation; check the connection before trying again, remote changes may have completed".into()))?
}
fn python(mode: &str) -> String {
    format!(
        "python3 -c {} {}",
        crate::node_channel::shell_quote(SCRIPT),
        mode
    )
}

pub(crate) async fn preparation(host: &str, socket: &str, install: bool) -> Value {
    if !socket.is_empty() {
        return json!({"available":false,"reason":"Prepare a custom runtime socket separately."});
    }
    if !install {
        return json!({"available":true,"action":"start"});
    }
    match execute(host, python("probe"), None, Duration::from_secs(8)).await {
        Ok(probe) => {
            let platform = probe["platform"].as_str();
            let supported = platform.is_some_and(|p| {
                matches!(
                    p,
                    "linux-x86_64" | "linux-aarch64" | "macos-x86_64" | "macos-aarch64"
                )
            });
            let ready = supported && probe["pythonReady"] == true;
            let reason = if !supported {
                "This remote platform is not supported."
            } else if probe["pythonReady"] != true {
                "Install Python 3.9 or later on the remote computer."
            } else {
                ""
            };
            json!({"available":ready,"action":"install","platform":platform,"installPath":probe["installPath"],"reason":reason})
        }
        Err(_) => {
            json!({"available":false,"reason":"Check remote access and install Python 3.9 or later on the remote computer."})
        }
    }
}
async fn verified_local(path: &Path) -> Result<(String, String)> {
    let checksum = tokio::fs::read_to_string(path.with_extension("gz.sha256")).await?;
    let checksum = checksum
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if checksum.len() != 64 || !checksum.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::Invalid("invalid runtime package checksum".into()));
    }
    let mut file = tokio::fs::File::open(path).await?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let count = file.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        size += count as u64;
        if size > LIMIT {
            return Err(Error::Invalid("runtime package exceeds 512 MiB".into()));
        }
        digest.update(&buffer[..count]);
    }
    if format!("{:x}", digest.finalize()) != checksum {
        return Err(Error::Invalid("runtime package SHA-256 mismatch".into()));
    }
    Ok((checksum, env!("CARGO_PKG_VERSION").into()))
}

pub(crate) async fn prepare(host: &str, socket: &str, action: &str) -> Result<Value> {
    tokio::time::timeout(Duration::from_secs(600), prepare_inner(host, socket, action)).await
        .map_err(|_| Error::Timeout("SSH preparation; remote changes may have completed, check the connection before trying again".into()))?
}
async fn prepare_inner(host: &str, socket: &str, action: &str) -> Result<Value> {
    if !socket.is_empty() || !matches!(action, "install" | "start") {
        return Err(Error::Invalid(
            "preparation requires the default runtime and an explicit install or start action"
                .into(),
        ));
    }
    let readiness = crate::remote::inspect_ssh(host, socket, None).await?;
    if readiness["ready"] == true {
        return Ok(readiness);
    }
    let expected = if action == "install" {
        "not-installed"
    } else {
        "runtime-unavailable"
    };
    if readiness["status"] != expected || readiness["preparation"]["available"] != true {
        return Err(Error::Invalid(
            "remote state changed or preparation is unavailable; check the connection again".into(),
        ));
    }
    if action == "install" {
        let platform = readiness["preparation"]["platform"]
            .as_str()
            .ok_or(Error::Closed)?;
        // A private staging directory separates release verification from remote execution.
        let staging = tempfile::tempdir()?;
        let (path, checksum, version) = if let Some(path) = local_package(platform) {
            let (checksum, version) = verified_local(&path).await?;
            (path, checksum, version)
        } else {
            crate::update::stage_runtime(&manifest(), platform, staging.path()).await?
        };
        let arguments = format!(
            "install {} {} {}",
            crate::node_channel::shell_quote(&checksum),
            crate::node_channel::shell_quote(platform),
            crate::node_channel::shell_quote(&version)
        );
        execute(
            host,
            python(&arguments),
            Some(&path),
            Duration::from_secs(180),
        )
        .await?;
    }
    // Start/attach uses the owner's serialized bootstrap and never restarts an existing owner.
    execute(
        host,
        format!(
            "{}\"$SWARMIE\" runtime start >/dev/null && printf '{{\"started\":true}}'",
            crate::node_channel::SSH_RUNTIME
        ),
        None,
        Duration::from_secs(40),
    )
    .await?;
    let verified = crate::remote::inspect_ssh(host, socket, None).await?;
    if verified["ready"] != true {
        return Err(Error::Provider("runtime preparation completed but verification failed; check the connection before trying again".into()));
    }
    Ok(verified)
}
