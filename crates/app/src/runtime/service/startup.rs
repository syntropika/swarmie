//! Native startup ownership and platform observations; changes never restart an active owner.
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::Output,
    time::Duration,
};
use swarmie::{Error, LocalStoreIdentity, Result};

#[derive(Subcommand)]
pub enum StartupCommand {
    /// Read automatic-startup configuration without changing the service or runtime.
    Inspect {
        #[arg(long)]
        expected_node: Option<String>,
    },
    /// Install and enable a user service without interrupting the current owner.
    Enable {
        /// On Linux, explicitly enable lingering to allow startup before login.
        #[arg(long)]
        boot: bool,
        #[arg(long)]
        expected_node: Option<String>,
    },
    /// Disable future automatic startup; keep the current service and work running.
    Disable {
        #[arg(long)]
        expected_node: Option<String>,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    root: PathBuf,
    unit_sha256: String,
}
fn record_path() -> Result<PathBuf> {
    Ok(super::config_dir()?.join("startup.json"))
}
pub(super) fn regular(path: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    return Err(Error::Invalid(
                        "startup configuration cannot be a reparse point".into(),
                    ));
                }
            }
            Ok(true)
        }
        Ok(_) => Err(Error::Invalid(
            "startup configuration must be a regular file".into(),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}
pub(super) fn check_owned(root: &Path) -> Result<()> {
    let unit = super::unit_path()?;
    let installed = regular(&unit)?;
    let record = record_path()?;
    let recorded = regular(&record)?;
    if !installed && !recorded {
        return Ok(());
    }
    if !recorded
        || std::fs::metadata(&record)?.len() > 16 * 1024
        || (installed && std::fs::metadata(&unit)?.len() > 64 * 1024)
    {
        return Err(Error::Invalid("existing service is not managed by this installation; preserve it and configure startup manually".into()));
    }
    let record: Record = serde_json::from_slice(&std::fs::read(record)?)?;
    if record.root != std::fs::canonicalize(root)?
        || (installed
            && record.unit_sha256 != format!("{:x}", Sha256::digest(std::fs::read(unit)?)))
    {
        return Err(Error::Invalid("service belongs to another data directory or was modified; preserve it and configure startup manually".into()));
    }
    Ok(())
}
pub(super) async fn remember(root: &Path, unit: &Path) -> Result<()> {
    let record = Record {
        root: root.to_owned(),
        unit_sha256: format!("{:x}", Sha256::digest(tokio::fs::read(unit).await?)),
    };
    super::private_write(&record_path()?, &serde_json::to_vec(&record)?).await
}
pub(super) async fn command(program: &str, arguments: &[&str]) -> Result<Output> {
    tokio::time::timeout(
        Duration::from_secs(if arguments.contains(&"stop") { 70 } else { 8 }),
        tokio::process::Command::new(program)
            .args(arguments)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| Error::Timeout("startup service manager".into()))?
    .map_err(Error::from)
}
async fn domain() -> Result<String> {
    let uid = command("id", &["-u"]).await?;
    if !uid.status.success() {
        return Err(Error::Provider("could not determine startup user".into()));
    }
    Ok(format!(
        "gui/{}",
        String::from_utf8_lossy(&uid.stdout).trim()
    ))
}
async fn linger() -> Option<bool> {
    let uid = command("id", &["-u"]).await.ok()?;
    if !uid.status.success() {
        return None;
    }
    let uid = String::from_utf8_lossy(&uid.stdout);
    let result = command(
        "loginctl",
        &["show-user", uid.trim(), "--property=Linger", "--value"],
    )
    .await
    .ok()?;
    if !result.status.success() {
        return None;
    }
    match String::from_utf8_lossy(&result.stdout).trim() {
        "yes" => Some(true),
        "no" => Some(false),
        _ => None,
    }
}
pub(crate) async fn inspect_startup(root: &Path) -> Result<Value> {
    let unit = super::unit_path()?;
    if cfg!(windows) {
        let mut result = match super::windows::operation("inspect", &unit).await {
            Ok(result) => result,
            Err(_) => {
                json!({"available":false,"installed":regular(&unit)?,"enabled":null,"running":null,"trigger":"login","bootReady":false,"canEnableBoot":false,"reason":"Windows Task Scheduler is unavailable. Check the task permissions and PowerShell installation.","notice":"Automatic startup uses a task for the current Windows user."})
            }
        };
        if let Err(error) = check_owned(root) {
            result["available"] = false.into();
            result["reason"] = error.to_string().into();
        }
        return Ok(result);
    }
    let installed = regular(&unit)?;
    let ownership = check_owned(root);
    let macos = cfg!(target_os = "macos");
    let supported = macos || cfg!(target_os = "linux");
    let (available, enabled, running) = if macos {
        let domain = domain().await?;
        let manager = command("launchctl", &["print", &domain]).await;
        if !manager.is_ok_and(|r| r.status.success()) {
            (false, None, None)
        } else {
            let disabled = command("launchctl", &["print-disabled", &domain]).await?;
            let enabled = if disabled.status.success() {
                let disabled = String::from_utf8_lossy(&disabled.stdout)
                    .lines()
                    .any(|line| {
                        line.contains("\"com.swarmie.daemon\"")
                            && line.split_once("=>").is_some_and(|(_, value)| {
                                matches!(value.trim(), "true" | "disabled")
                            })
                    });
                Some(installed && !disabled)
            } else {
                None
            };
            let target = format!("{domain}/com.swarmie.daemon");
            let loaded = command("launchctl", &["print", &target]).await?;
            let running = Some(
                loaded.status.success()
                    && String::from_utf8_lossy(&loaded.stdout).contains("state = running"),
            );
            (enabled.is_some(), enabled, running)
        }
    } else if supported {
        let manager = command(
            "systemctl",
            &["--user", "show", "--property=Version", "--value"],
        )
        .await;
        if !manager.is_ok_and(|r| r.status.success()) {
            (false, None, None)
        } else {
            let enabled = if installed {
                let result =
                    command("systemctl", &["--user", "is-enabled", "swarmie.service"]).await?;
                match String::from_utf8_lossy(&result.stdout).trim() {
                    "enabled" => Some(true),
                    "disabled" => Some(false),
                    _ => None,
                }
            } else {
                Some(false)
            };
            let result = command("systemctl", &["--user", "is-active", "swarmie.service"]).await?;
            let running = match String::from_utf8_lossy(&result.stdout).trim() {
                "active" => Some(true),
                "inactive" | "failed" | "unknown" => Some(false),
                _ => None,
            };
            (enabled.is_some(), enabled, running)
        }
    } else {
        (false, None, None)
    };
    let linger = if cfg!(target_os = "linux") {
        linger().await
    } else {
        None
    };
    let reason = if let Err(error) = &ownership {
        error.to_string()
    } else if !supported {
        "Automatic startup is supported on Linux and macOS.".into()
    } else if !available {
        "The user service manager is unavailable. Configure a user login session or a system service on this computer.".into()
    } else {
        String::new()
    };
    Ok(
        json!({"available":available && ownership.is_ok(),"installed":installed,"enabled":enabled,"running":running,
        "trigger":if linger == Some(true) {"boot"} else {"login"},"bootReady":linger == Some(true),
        "canEnableBoot":cfg!(target_os = "linux") && linger.is_some(),
        "reason":reason,"notice":if macos { "Starts at user login. Startup without a graphical login requires an administrator-managed system daemon." }
            else { "Startup before login and availability after logout require user lingering. Disabling startup does not disable lingering for other services." }}),
    )
}
async fn lock() -> Result<std::fs::File> {
    let path = super::config_dir()?.join("startup.lock");
    tokio::fs::create_dir_all(path.parent().ok_or(Error::Closed)?).await?;
    tokio::task::spawn_blocking(move || {
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        if let Ok(meta) = std::fs::symlink_metadata(&path)
            && (!meta.is_file() || meta.file_type().is_symlink())
        {
            return Err(Error::Invalid("invalid startup lock".into()));
        }
        let lock = options.open(path)?;
        fs2::FileExt::try_lock_exclusive(&lock)
            .map_err(|_| Error::Invalid("startup configuration is busy; check again".into()))?;
        Ok(lock)
    })
    .await
    .map_err(|e| Error::Provider(e.to_string()))?
}
pub(crate) async fn configure_startup(root: &Path, enabled: bool, boot: bool) -> Result<Value> {
    if boot && (!enabled || !cfg!(target_os = "linux")) {
        return Err(Error::Unsupported("boot configuration requires Linux user lingering; use an administrator-managed system daemon on other platforms".into()));
    }
    let _lock = lock().await?;
    let before = inspect_startup(root).await?;
    if before["available"] != true {
        return Err(Error::Unsupported(
            before["reason"]
                .as_str()
                .unwrap_or("startup unavailable")
                .into(),
        ));
    }
    if boot && before["bootReady"] != true {
        let uid = command("id", &["-u"]).await?;
        if !uid.status.success() {
            return Err(Error::Provider("could not determine startup user".into()));
        }
        let uid = String::from_utf8_lossy(&uid.stdout);
        let result = command(
            "loginctl",
            &["--no-ask-password", "enable-linger", uid.trim()],
        )
        .await?;
        if !result.status.success() || linger().await != Some(true) {
            return Err(Error::Provider("could not enable user lingering; ask the server administrator to enable it, then check startup again".into()));
        }
    }
    if enabled && !regular(&super::unit_path()?)? {
        super::install(root, 0, None, None, true, Some(root.join("resources"))).await?;
    }
    if enabled || before["installed"] == true {
        super::manager(if enabled { "enable" } else { "disable" }).await?;
    }
    let mut after = inspect_startup(root).await?;
    if after["enabled"] != enabled {
        return Err(Error::Provider(
            "startup configuration could not be verified; check it again before retrying".into(),
        ));
    }
    after["executionChanged"] = false.into();
    after["supervisionPendingRestart"] = (enabled && after["running"] != true).into();
    Ok(after)
}
async fn verify_node(root: &Path, expected: Option<String>) -> Result<()> {
    if let Some(expected) = expected {
        let expected: LocalStoreIdentity = serde_json::from_str(&expected)?;
        let record = crate::runtime_host::read(root)?.ok_or_else(|| {
            Error::Invalid("runtime identity is unavailable; check the node again".into())
        })?;
        if record.runtime_identity.identity != expected {
            return Err(Error::Invalid(
                "runtime node changed; startup configuration was not modified".into(),
            ));
        }
    }
    Ok(())
}
pub(super) async fn run(root: &Path, operation: StartupCommand) -> Result<()> {
    let value = match operation {
        StartupCommand::Inspect { expected_node } => {
            verify_node(root, expected_node).await?;
            inspect_startup(root).await?
        }
        StartupCommand::Enable {
            boot,
            expected_node,
        } => {
            verify_node(root, expected_node).await?;
            configure_startup(root, true, boot).await?
        }
        StartupCommand::Disable { expected_node } => {
            verify_node(root, expected_node).await?;
            configure_startup(root, false, false).await?
        }
    };
    println!("{value}");
    Ok(())
}
