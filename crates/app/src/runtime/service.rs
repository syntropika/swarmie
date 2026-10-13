mod startup;
mod windows;
use clap::Subcommand;
use serde_json::json;
pub(crate) use startup::{configure_startup, inspect_startup};
use std::path::{Path, PathBuf};
use swarmie::{Error, Result};
#[derive(Subcommand)]
pub enum ServiceCommand {
    /// Install a user service and a private bearer token; start it separately.
    Install {
        #[arg(long, default_value_t = 7480)]
        port: u16,
        #[arg(long)]
        desktop_assets: Option<PathBuf>,
        #[arg(long, requires = "desktop_assets")]
        public_origin: Option<String>,
        #[arg(long)]
        workbench: bool,
        #[arg(long)]
        resources_root: Option<PathBuf>,
    },
    /// Inspect or configure automatic runtime startup without restarting active work.
    Startup {
        #[command(subcommand)]
        command: startup::StartupCommand,
    },
    Start,
    Stop,
    /// Enable startup on user login without starting the service now.
    Enable,
    /// Disable startup on user login without stopping a running service.
    Disable,
    Status,
    Uninstall,
    /// Print the private service token for pairing a trusted client.
    Token,
}
fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| Error::Invalid("HOME is unavailable".into()))
}
fn config_dir() -> Result<PathBuf> {
    if cfg!(windows) {
        return Ok(std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or(home()?.join("AppData/Local"))
            .join("swarmie"));
    }
    Ok(std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or(home()?.join(".config"))
        .join("swarmie"))
}
fn unit_path() -> Result<PathBuf> {
    if cfg!(windows) {
        return Ok(config_dir()?.join("runtime-task.xml"));
    }
    if cfg!(target_os = "macos") {
        Ok(home()?.join("Library/LaunchAgents/com.swarmie.daemon.plist"))
    } else {
        Ok(std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or(home()?.join(".config"))
            .join("systemd/user/swarmie.service"))
    }
}
fn quote_systemd(value: &str) -> Result<String> {
    if value.chars().any(char::is_control) {
        return Err(Error::Invalid(
            "service argument contains control characters".into(),
        ));
    }
    Ok(format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
    ))
}
fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn render_unit(
    executable: &Path,
    arguments: &[String],
    token_path: &Path,
    macos: bool,
) -> Result<String> {
    if cfg!(windows) {
        return windows::render(executable, arguments, token_path);
    }
    if macos {
        let args = std::iter::once(executable.to_string_lossy().into_owned())
            .chain(arguments.iter().cloned())
            .map(|arg| format!("<string>{}</string>", xml(&arg)))
            .collect::<String>();
        // launchd cannot read EnvironmentFile; pass the private token file rather than embed its secret.
        Ok(format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>com.swarmie.daemon</string><key>ProgramArguments</key><array>{args}<string>--token-file</string><string>{}</string></array><key>RunAtLoad</key><true/><key>KeepAlive</key><true/><key>StandardErrorPath</key><string>{}</string><key>StandardOutPath</key><string>{}</string></dict></plist>\n",
            xml(&token_path.to_string_lossy()),
            xml(&token_path.with_extension("stderr.log").to_string_lossy()),
            xml(&token_path.with_extension("stdout.log").to_string_lossy())
        ))
    } else {
        let args = std::iter::once(executable.to_string_lossy().into_owned())
            .chain(arguments.iter().cloned())
            .map(|arg| quote_systemd(&arg))
            .collect::<Result<Vec<_>>>()?
            .join(" ");
        Ok(format!(
            "[Unit]\nDescription=swarmie agent daemon\nAfter=network.target\n\n[Service]\nType=simple\nExecStart={args} --token-file {}\nRestart=on-failure\nRestartSec=2\nKillSignal=SIGINT\nTimeoutStopSec=60\n\n[Install]\nWantedBy=default.target\n",
            quote_systemd(&token_path.to_string_lossy())?
        ))
    }
}
async fn private_write(path: &Path, bytes: &[u8]) -> Result<()> {
    tokio::fs::create_dir_all(
        path.parent()
            .ok_or_else(|| Error::Invalid("service path has no parent".into()))?,
    )
    .await?;
    startup::regular(path)?;
    let mut options = tokio::fs::OpenOptions::new();
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    options.mode(0o600);
    use tokio::io::AsyncWriteExt;
    let mut file = options.open(path).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .await?;
    }
    file.write_all(bytes).await?;
    file.sync_all().await?;
    Ok(())
}
async fn manager(action: &str) -> Result<()> {
    if cfg!(windows) {
        windows::operation(action, &unit_path()?).await?;
        return Ok(());
    }
    let status = if cfg!(target_os = "macos") {
        let uid = startup::command("id", &["-u"]).await?;
        if !uid.status.success() {
            return Err(Error::Provider("could not determine launchd user".into()));
        }
        let domain = format!("gui/{}", String::from_utf8_lossy(&uid.stdout).trim());
        let target = format!("{domain}/com.swarmie.daemon");
        let args: Vec<String> = match action {
            "start" => vec![
                "bootstrap".into(),
                domain,
                unit_path()?.to_string_lossy().into(),
            ],
            "stop" => vec!["bootout".into(), target],
            "status" => vec!["print".into(), target],
            "enable" => vec!["enable".into(), target],
            "disable" => vec!["disable".into(), target],
            _ => return Err(Error::Invalid("unknown service operation".into())),
        };
        {
            let output = startup::command(
                "launchctl",
                &args.iter().map(String::as_str).collect::<Vec<_>>(),
            )
            .await?;
            if action == "status" {
                print!("{}", String::from_utf8_lossy(&output.stdout));
            }
            output.status
        }
    } else {
        {
            let output =
                startup::command("systemctl", &["--user", action, "swarmie.service"]).await?;
            if action == "status" {
                print!("{}", String::from_utf8_lossy(&output.stdout));
            }
            output.status
        }
    };
    if !status.success() {
        return Err(Error::Provider(format!(
            "service {action} exited: {status}"
        )));
    }
    Ok(())
}
// A missing service is different from a manager failure: never remove its unit when
// the manager cannot establish that execution has stopped.
async fn service_loaded() -> Result<bool> {
    if cfg!(windows) {
        return Ok(windows::operation("inspect", &unit_path()?).await?["running"] == true);
    }
    let output = if cfg!(target_os = "macos") {
        let uid = startup::command("id", &["-u"]).await?;
        if !uid.status.success() {
            return Err(Error::Provider("could not determine launchd user".into()));
        }
        let target = format!(
            "gui/{}/com.swarmie.daemon",
            String::from_utf8_lossy(&uid.stdout).trim()
        );
        startup::command("launchctl", &["print", &target]).await?
    } else {
        startup::command("systemctl", &["--user", "is-active", "swarmie.service"]).await?
    };
    if output.status.success() {
        return Ok(true);
    }
    let missing = if cfg!(target_os = "macos") {
        String::from_utf8_lossy(&output.stderr).contains("Could not find service")
    } else {
        matches!(output.status.code(), Some(3 | 4))
            && matches!(
                String::from_utf8_lossy(&output.stdout).trim(),
                "inactive" | "failed" | "unknown"
            )
    };
    if missing {
        Ok(false)
    } else {
        Err(Error::Provider(
            "could not establish service state; stop it explicitly before uninstalling".into(),
        ))
    }
}
async fn install(
    root: &Path,
    port: u16,
    desktop_assets: Option<PathBuf>,
    public_origin: Option<String>,
    workbench: bool,
    resources_root: Option<PathBuf>,
) -> Result<serde_json::Value> {
    let token_path = config_dir()?.join("service.token");
    tokio::fs::create_dir_all(root).await?;
    let root = tokio::fs::canonicalize(root).await?;
    let executable = std::env::current_exe()?;
    startup::check_owned(&root)?;
    let mut args = vec![
        "--state-dir".into(),
        root.to_string_lossy().into(),
        "serve".into(),
        "--http".into(),
        format!("127.0.0.1:{port}"),
    ];
    if let Some(assets) = desktop_assets {
        let assets = tokio::fs::canonicalize(assets).await?;
        if !assets.join("index.html").is_file() {
            return Err(Error::Invalid(
                "desktop assets must contain index.html".into(),
            ));
        }
        args.extend(["--desktop-assets".into(), assets.to_string_lossy().into()]);
    }
    if let Some(origin) = public_origin {
        let parsed = reqwest::Url::parse(&origin)
            .map_err(|_| Error::Invalid("invalid public origin".into()))?;
        if parsed.scheme() != "https"
            || parsed.path() != "/"
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(Error::Invalid(
                "public origin must be an HTTPS origin without a path or credentials".into(),
            ));
        }
        args.extend([
            "--public-origin".into(),
            origin.trim_end_matches('/').into(),
        ]);
    }
    if workbench {
        args.push("--workbench".into());
    }
    if port == 0 {
        args.extend([
            "--git-worktrees".into(),
            root.join("worktrees").to_string_lossy().into(),
        ]);
    }
    if let Some(resources) = resources_root {
        tokio::fs::create_dir_all(&resources).await?;
        let resources = tokio::fs::canonicalize(resources).await?;
        args.extend([
            "--resources-root".into(),
            resources.to_string_lossy().into(),
        ]);
    }
    if !token_path.exists() {
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        private_write(&token_path, token.as_bytes()).await?;
    }
    let unit = unit_path()?;
    private_write(
        &unit,
        render_unit(&executable, &args, &token_path, cfg!(target_os = "macos"))?.as_bytes(),
    )
    .await?;
    startup::remember(&root, &unit).await?;
    if cfg!(target_os = "linux") {
        let status = startup::command("systemctl", &["--user", "daemon-reload"])
            .await?
            .status;
        if !status.success() {
            return Err(Error::Provider("systemd user daemon reload failed".into()));
        }
    }
    Ok(
        json!({"installed":true,"unit":unit,"url":format!("http://127.0.0.1:{port}/rpc"),"tokenFile":token_path,"started":false}),
    )
}

pub async fn run(root: &Path, command: ServiceCommand) -> Result<()> {
    let token_path = config_dir()?.join("service.token");
    match command {
        ServiceCommand::Install {
            port,
            desktop_assets,
            public_origin,
            workbench,
            resources_root,
        } => {
            if port == 0 {
                return Err(Error::Invalid("service port must be nonzero".into()));
            }
            println!(
                "{}",
                install(
                    root,
                    port,
                    desktop_assets,
                    public_origin,
                    workbench,
                    resources_root
                )
                .await?
            );
        }
        ServiceCommand::Startup { command } => startup::run(root, command).await?,
        ServiceCommand::Start => {
            startup::check_owned(root)?;
            manager("start").await?;
        }
        ServiceCommand::Stop => {
            startup::check_owned(root)?;
            if cfg!(windows) {
                let task = windows::operation("inspect", &unit_path()?).await?;
                if task["available"] != true {
                    return Err(Error::Invalid(
                        "the scheduled task is not owned by this configuration".into(),
                    ));
                }
                if task["running"] == true {
                    let endpoint = crate::runtime_host::read(root)?.ok_or_else(|| {
                        Error::Invalid(
                            "the service runtime is not ready; check it again before stopping"
                                .into(),
                        )
                    })?;
                    if !crate::runtime_host::verified(&endpoint, None).await? {
                        return Err(Error::Invalid("the service runtime cannot be verified; preserve the scheduled task and check execution".into()));
                    }
                    crate::runtime_host::run(
                        root,
                        crate::runtime_host::RuntimeCommand::Stop { token: None },
                    )
                    .await?;
                }
            } else {
                manager("stop").await?;
            }
        }
        ServiceCommand::Enable => {
            startup::check_owned(root)?;
            manager("enable").await?;
        }
        ServiceCommand::Disable => {
            startup::check_owned(root)?;
            manager("disable").await?;
        }
        ServiceCommand::Status => {
            if cfg!(windows) {
                println!("{}", windows::operation("inspect", &unit_path()?).await?);
            } else {
                manager("status").await?;
            }
        }
        ServiceCommand::Token => {
            let token = tokio::fs::read_to_string(token_path).await?;
            println!("{}", token.trim());
        }
        ServiceCommand::Uninstall => {
            startup::check_owned(root)?;
            if unit_path()?.exists() {
                if service_loaded().await? {
                    return Err(Error::Invalid("service is running or loaded; use service stop explicitly before uninstalling".into()));
                }
                // Disable future startup without touching runtime data or the retained token.
                manager(if cfg!(windows) {
                    "uninstall"
                } else {
                    "disable"
                })
                .await?;
                tokio::fs::remove_file(unit_path()?).await?;
            }
            if cfg!(target_os = "linux") {
                let status = startup::command("systemctl", &["--user", "daemon-reload"])
                    .await?
                    .status;
                if !status.success() {
                    return Err(Error::Provider("systemd user daemon reload failed".into()));
                }
            }
            println!(
                "{}",
                json!({"installed":false,"tokenPreserved":true,"dataPreserved":true,"stopped":false})
            );
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "writes native service unit syntax diagnostics to an explicitly supplied artifact directory"]
    fn export_native_unit_validation_artifacts() {
        let directory = PathBuf::from(
            std::env::var_os("SWARMIE_UNIT_ARTIFACT_DIR")
                .expect("supply an external artifact directory"),
        );
        std::fs::create_dir_all(&directory).unwrap();
        let arguments = vec!["serve".into(), "--http".into(), "127.0.0.1:7480".into()];
        for (name, macos) in [
            ("swarmie.service", false),
            ("com.swarmie.daemon.plist", true),
        ] {
            let unit = render_unit(
                &std::env::current_exe().unwrap(),
                &arguments,
                &directory.join("service.token"),
                macos,
            )
            .unwrap();
            std::fs::write(directory.join(name), unit).unwrap();
        }
    }
    #[test]
    #[cfg(not(windows))]
    fn service_units_quote_arguments_and_reference_private_tokens() {
        let args = vec!["serve".into(), "--http".into(), "127.0.0.1:7480".into()];
        let linux = render_unit(
            Path::new("/test app/swarmie"),
            &args,
            Path::new("/test/service.token"),
            false,
        )
        .unwrap();
        assert!(linux.contains("ExecStart=\"/test app/swarmie\""));
        assert!(linux.contains("KillSignal=SIGINT"));
        assert!(linux.contains("--token-file \"/test/service.token\""));
        let mac = render_unit(
            Path::new("/test<&/swarmie"),
            &args,
            Path::new("/test/service.token"),
            true,
        )
        .unwrap();
        assert!(mac.contains("/test&lt;&amp;/swarmie"));
        assert!(mac.contains("<string>--token-file</string>"));
        assert!(!mac.contains("secret"));
    }
}
