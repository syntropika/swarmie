use swarmie::{Error, Result};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
};

fn native_url(url: &reqwest::Url) -> bool {
    url.scheme() == "http"
        && url.username().is_empty()
        && url.password().is_none()
        && matches!(url.host_str(), Some("127.0.0.1" | "[::1]"))
}

fn packaged_host(executable: &Path) -> Option<PathBuf> {
    let directory = executable.parent()?;
    let name = executable.file_name()?.to_str()?;
    let host = directory.join(name.strip_suffix("_services").unwrap_or("swarmie-desktop"));
    host.is_file().then_some(host)
}

fn set_query(url: &mut reqwest::Url, name: &str, value: &str) {
    let existing: Vec<_> = url
        .query_pairs()
        .filter(|(key, _)| key != name)
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    url.query_pairs_mut()
        .clear()
        .extend_pairs(existing)
        .append_pair(name, value);
}

pub async fn run(
    url: String,
    project: Option<PathBuf>,
    session: Option<String>,
    host: Option<PathBuf>,
    print: bool,
) -> Result<()> {
    let mut parsed =
        reqwest::Url::parse(&url).map_err(|_| Error::Invalid("invalid application URL".into()))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(Error::Invalid("application URL requires HTTP(S)".into()));
    }
    if let Some(project) = project {
        let project = tokio::fs::canonicalize(project).await?;
        set_query(&mut parsed, "project", &project.to_string_lossy());
    }
    if let Some(session) = session {
        if uuid::Uuid::parse_str(&session).is_err() {
            return Err(Error::Invalid("session must be a runtime UUID".into()));
        }
        set_query(&mut parsed, "session", &session);
    }
    println!("{parsed}");
    if !print {
        if host.is_some() && !native_url(&parsed) {
            return Err(Error::Invalid("native desktop host requires an HTTP URL at 127.0.0.1 or [::1] without credentials".into()));
        }
        let host = if native_url(&parsed) {
            host.or_else(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|path| packaged_host(&path))
            })
        } else {
            None
        };
        if let Some(host) = host {
            let host = tokio::fs::canonicalize(host).await?;
            if !host.is_file() || host == std::env::current_exe()? {
                return Err(Error::Invalid(
                    "desktop host must be a separate native executable".into(),
                ));
            }
            // The existing service owns daemon lifetime; the opened window outlives this CLI invocation.
            tokio::process::Command::new(host)
                .env("SWARMIE_DESKTOP_URL", parsed.as_str())
                .env("SWARMIE_DESKTOP_MANAGED", "1")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            return Ok(());
        }
        let executable = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        let status = tokio::process::Command::new(executable)
            .arg(parsed.as_str())
            .status()
            .await?;
        if !status.success() {
            return Err(Error::Provider(format!(
                "application opener exited: {status}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_host_accepts_only_supported_loopback_origins() {
        for url in ["http://127.0.0.1:7480/?session=id", "http://[::1]:7480/"] {
            assert!(native_url(&reqwest::Url::parse(url).unwrap()));
        }
        for url in [
            "https://127.0.0.1/",
            "http://localhost/",
            "http://127.2.0.1/",
            "http://127.0.0.1.evil.test/",
            "http://user@127.0.0.1/",
        ] {
            assert!(!native_url(&reqwest::Url::parse(url).unwrap()));
        }
    }

    #[test]
    fn packaged_services_resolve_the_sibling_native_host() {
        let directory = tempfile::tempdir().unwrap();
        let host = directory.path().join("swarmie-desktop");
        std::fs::write(&host, []).unwrap();
        assert_eq!(
            packaged_host(&directory.path().join("swarmie-desktop_services")),
            Some(host.clone())
        );
        assert_eq!(packaged_host(&directory.path().join("swarmie")), Some(host));
        assert_eq!(packaged_host(Path::new("/missing/swarmie")), None);
    }
}
