use mpai::{Error, Result};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

/// Own the daemon and host lifetimes; a fresh port prevents competing installations from sharing sessions accidentally.
pub async fn run(root: &Path, assets: PathBuf, host: PathBuf) -> Result<()> {
    let assets = tokio::fs::canonicalize(assets).await?;
    let host = tokio::fs::canonicalize(host).await?;
    if !assets.join("index.html").is_file() || !host.is_file() {
        return Err(Error::Invalid(
            "desktop requires built frontend assets and a native host executable".into(),
        ));
    }
    if host == std::env::current_exe()? {
        return Err(Error::Invalid(
            "desktop host must differ from the daemon executable".into(),
        ));
    }
    let base = root.parent().unwrap_or(root);
    let mut daemon = Command::new(std::env::current_exe()?)
        .arg("--state-dir")
        .arg(root)
        .args([
            "serve",
            "--http",
            "127.0.0.1:0",
            "--workbench",
            "--desktop-assets",
        ])
        .arg(assets)
        .arg("--git-worktrees")
        .arg(base.join("worktrees"))
        .arg("--resources-root")
        .arg(base.join("resources"))
        .env_remove("MPAI_TOKEN")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let mut lines = BufReader::new(
        daemon
            .stderr
            .take()
            .ok_or_else(|| Error::Provider("daemon stderr is unavailable".into()))?,
    )
    .lines();
    let url = tokio::time::timeout(Duration::from_secs(20), async {
        while let Some(line) = lines.next_line().await? {
            if let Some(url) = line.strip_prefix("HTTP listening on ") {
                return Ok(url.trim_end_matches("/rpc").to_owned());
            }
            eprintln!("{line}");
        }
        Err(Error::Provider(
            "desktop daemon exited before readiness".into(),
        ))
    })
    .await
    .map_err(|_| Error::Timeout("desktop daemon readiness".into()))??;
    eprintln!("Desktop daemon ready at {url}");
    let diagnostics = tokio::spawn(async move {
        while let Ok(Some(line)) = lines.next_line().await {
            eprintln!("{line}");
        }
    });
    let result=async {
        let mut child=Command::new(host).env("MPAI_DESKTOP_URL",url).env("MPAI_DESKTOP_MANAGED","1").kill_on_drop(true).spawn()?;
        tokio::select! {
            result=child.wait()=> {
                let status=result?;
                if status.success() { Ok(()) } else { Err(Error::Provider(format!("native desktop exited with {status}"))) }
            }
            result=daemon.wait()=>Err(Error::Provider(format!("desktop daemon exited with {}",result?))),
            result=tokio::signal::ctrl_c()=> { result?; let _=child.kill().await; let _=child.wait().await; Ok(()) }
        }
    }.await;
    #[cfg(unix)]
    if let Some(pid) = daemon.id() {
        let _ = Command::new("kill")
            .args(["-INT", &pid.to_string()])
            .status()
            .await;
    }
    if tokio::time::timeout(Duration::from_secs(3), daemon.wait())
        .await
        .is_err()
    {
        let _ = daemon.kill().await;
        let _ = daemon.wait().await;
    }
    diagnostics.abort();
    result
}
