use mpai::{Error, Result};
use std::path::{Path, PathBuf};
use tokio::process::Command;

/// Attach a client without transferring execution ownership to its window.
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
    let endpoint = crate::runtime_host::attach(root).await?;
    let gateway = crate::client_host::ClientHost::start(root, assets, &endpoint).await?;
    eprintln!(
        "Desktop attached to runtime at {}; client at {}",
        endpoint.url, gateway.url
    );
    let mut client = Command::new(host)
        .env(
            "MPAI_DESKTOP_URL",
            format!("{}/?client=native", gateway.url),
        )
        .env("MPAI_DESKTOP_IPC_TOKEN", &gateway.token)
        .env("MPAI_DESKTOP_MANAGED", "1")
        .kill_on_drop(true)
        .spawn()?;
    let result = tokio::select! {
        result=client.wait()=> {
            let status=result?;
            if status.success() { Ok(()) } else { Err(Error::Provider(format!("native desktop exited with {status}; the runtime remains available"))) }
        },
        result=tokio::signal::ctrl_c()=> { result?; let _=client.kill().await; let _=client.wait().await; Ok(()) },
    };
    gateway.close().await;
    result
}
