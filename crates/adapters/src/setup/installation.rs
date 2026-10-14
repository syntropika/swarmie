//! Verified, explicit installation into runtime-owned storage.
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256, Sha512};
mod packages;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use swarmie::{Error, Result};
use tokio::{io::AsyncWriteExt, sync::mpsc};

const UPDATER: &str = "https://antigravity-cli-auto-updater-974169037036.us-central1.run.app";
const MAX_DOWNLOAD: u64 = 512 * 1024 * 1024;
const MAX_EXECUTABLE: u64 = 1024 * 1024 * 1024;

struct InstallationFiles {
    staging: PathBuf,
    release: Option<PathBuf>,
    metadata: Option<PathBuf>,
    activated: bool,
}
impl Drop for InstallationFiles {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.staging);
        if let Some(metadata) = &self.metadata {
            let _ = std::fs::remove_file(metadata);
        }
        if !self.activated
            && let Some(release) = &self.release
        {
            let _ = std::fs::remove_dir_all(release);
        }
    }
}

#[derive(Deserialize)]
struct Manifest {
    version: String,
    url: String,
    sha512: String,
}
#[derive(Serialize, Deserialize)]
struct Active {
    version: String,
    executable: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sdk_worker: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    auth_worker: Option<PathBuf>,
}

pub(super) fn supported(harness: &str) -> bool {
    match harness {
        "antigravity" | "grok" => platform().is_some(),
        "cursor" => packages::cursor_release().is_some(),
        "codex" | "claude" | "opencode" | "pi" => cfg!(unix) && platform().is_some(),
        _ => false,
    }
}
pub(super) fn message(harness: &str) -> &'static str {
    match harness {
        "grok" | "antigravity" => "Install the provider's managed local runtime",
        _ => "Managed installation requires Node.js 22.19 or newer and npm on this runtime node",
    }
}
pub(super) async fn install(
    root: &Path,
    harness: &str,
    cancel: Arc<AtomicBool>,
    progress: mpsc::Sender<Value>,
) -> Result<PathBuf> {
    if !supported(harness) {
        return Err(Error::Unsupported(
            "Managed installation on this platform".into(),
        ));
    }
    if harness == "antigravity" {
        install_antigravity(root, cancel, progress).await
    } else {
        packages::install(root, harness, cancel, progress).await
    }
}
fn platform() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("linux_amd64"),
        ("linux", "aarch64") => Some("linux_arm64"),
        ("macos", "x86_64") => Some("darwin_amd64"),
        ("macos", "aarch64") => Some("darwin_arm64"),
        ("windows", "x86_64") => Some("windows_amd64"),
        ("windows", "aarch64") => Some("windows_arm64"),
        _ => None,
    }
}
fn binary_name() -> &'static str {
    if cfg!(windows) { "agy.exe" } else { "agy" }
}
fn invalid() -> Error {
    Error::Provider("The provider release could not be verified".into())
}
fn cancelled(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Acquire) {
        Err(Error::Provider("Installation cancelled".into()))
    } else {
        Ok(())
    }
}
fn valid_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 64
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}
fn download_url(manifest: &Manifest) -> Result<reqwest::Url> {
    if !valid_version(&manifest.version)
        || manifest.sha512.len() != 128
        || !manifest.sha512.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err(invalid());
    }
    let url = reqwest::Url::parse(&manifest.url).map_err(|_| invalid())?;
    if url.scheme() != "https"
        || url.host_str() != Some("storage.googleapis.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !url
            .path()
            .starts_with("/antigravity-public/antigravity-cli/")
    {
        return Err(invalid());
    }
    Ok(url)
}
pub(super) fn managed_overlay(root: &Path, harness: &str) -> swarmie::AgentToolExecutionOverlay {
    let mut overlay = swarmie::AgentToolExecutionOverlay::default();
    if !supported(harness) {
        return overlay;
    }
    let directory = root.join("managed").join(harness);
    if let Some(active) = read_active(&directory, harness) {
        overlay.config = json!({"executable":active.0});
        if let Some(path) = active.1 {
            overlay.config["sdkWorkerPath"] = json!(path);
        }
        if let Some(path) = active.2 {
            overlay.config["authWorkerPath"] = json!(path);
        }
    }
    overlay
}
fn read_active(
    directory: &Path,
    harness: &str,
) -> Option<(PathBuf, Option<PathBuf>, Option<PathBuf>)> {
    use std::io::Read;
    let file = std::fs::File::open(directory.join("active.json")).ok()?;
    let mut contents = Vec::new();
    file.take(4097).read_to_end(&mut contents).ok()?;
    if contents.len() > 4096 {
        return None;
    }
    let active: Active = serde_json::from_slice(&contents).ok()?;
    if !valid_version(&active.version) {
        return None;
    }
    let resolve = |relative: &Path| -> Option<PathBuf> {
        if relative.is_absolute()
            || relative
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return None;
        }
        let candidate = directory.join(relative);
        let metadata = std::fs::symlink_metadata(&candidate).ok()?;
        if !metadata.is_file() || metadata.len() == 0 {
            return None;
        }
        let canonical = std::fs::canonicalize(&candidate).ok()?;
        canonical
            .starts_with(std::fs::canonicalize(directory).ok()?)
            .then_some(canonical)
    };
    if harness == "antigravity" && active.executable.file_name()? != binary_name() {
        return None;
    }
    let executable = resolve(&active.executable)?;
    let sdk_worker = match active.sdk_worker.as_deref() {
        Some(path) => Some(resolve(path)?),
        None => None,
    };
    let auth_worker = match active.auth_worker.as_deref() {
        Some(path) => Some(resolve(path)?),
        None => None,
    };
    Some((executable, sdk_worker, auth_worker))
}

fn private_directory(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
async fn bounded_download(
    client: &reqwest::Client,
    url: reqwest::Url,
    file: &Path,
    cancel: &AtomicBool,
    progress: &mpsc::Sender<Value>,
    expected_hash: &str,
) -> Result<()> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|_| {
            Error::Provider(
                "Agent tool download failed; check the runtime node's network connection".into(),
            )
        })?
        .error_for_status()
        .map_err(|_| invalid())?;
    let length = response.content_length();
    if length.is_some_and(|length| length > MAX_DOWNLOAD) {
        return Err(invalid());
    }
    let mut options = tokio::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut output = options.open(file).await?;
    let mut chunks = response.bytes_stream();
    let mut total = 0u64;
    let mut hash = Sha512::new();
    let mut hash256 = Sha256::new();
    let mut last_progress = 0;
    while let Some(chunk) = chunks.next().await {
        cancelled(cancel)?;
        let chunk = chunk.map_err(|_| {
            Error::Provider("Agent tool download was interrupted; retry installation".into())
        })?;
        total = total.checked_add(chunk.len() as u64).ok_or_else(invalid)?;
        if total > MAX_DOWNLOAD {
            return Err(invalid());
        }
        hash.update(&chunk);
        hash256.update(&chunk);
        output.write_all(&chunk).await?;
        if let Some(length) = length.filter(|length| *length > 0) {
            let percentage = total.saturating_mul(100) / length;
            if percentage >= last_progress + 5 {
                last_progress = percentage;
                let _ = progress.try_send(
                    json!({"message":"Downloading the official provider release", "progress":percentage.min(100)}),
                );
            }
        }
    }
    output.flush().await?;
    output.sync_all().await?;
    cancelled(cancel)?;
    if total == 0
        || length.is_some_and(|length| length != total)
        || (!expected_hash.is_empty()
            && if expected_hash.len() == 64 {
                format!("{:x}", hash256.finalize())
            } else {
                format!("{:x}", hash.finalize())
            } != expected_hash.to_ascii_lowercase())
    {
        return Err(invalid());
    }
    Ok(())
}
fn extract_verified_archive(archive: &Path, executable: &Path, cancel: &AtomicBool) -> Result<()> {
    use std::io::{Read, Write};
    let file = std::fs::File::open(archive)?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let mut found = false;
    let mut expanded = 0u64;
    let mut entries = 0usize;
    for entry in archive.entries().map_err(|_| invalid())? {
        cancelled(cancel)?;
        let mut entry = entry.map_err(|_| invalid())?;
        expanded = expanded.checked_add(entry.size()).ok_or_else(invalid)?;
        entries += 1;
        if expanded > MAX_EXECUTABLE || entries > 1024 {
            return Err(invalid());
        }
        // Extract exactly the documented executable; never unpack paths or links.
        if entry.path().map_err(|_| invalid())?.as_ref() != Path::new("antigravity") {
            continue;
        }
        if found
            || !entry.header().entry_type().is_file()
            || entry.size() == 0
            || entry.size() > MAX_EXECUTABLE
        {
            return Err(invalid());
        }
        found = true;
        let mut options = std::fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o700);
        }
        let mut output = options.open(executable)?;
        let mut buffer = [0u8; 65536];
        let mut written = 0u64;
        loop {
            cancelled(cancel)?;
            let count = entry.read(&mut buffer).map_err(|_| invalid())?;
            if count == 0 {
                break;
            }
            written += count as u64;
            if written > MAX_EXECUTABLE {
                return Err(invalid());
            }
            output.write_all(&buffer[..count])?;
        }
        if written != entry.size() {
            return Err(invalid());
        }
        output.sync_all()?;
    }
    if !found {
        return Err(invalid());
    }
    Ok(())
}
pub(super) async fn install_antigravity(
    root: &Path,
    cancel: Arc<AtomicBool>,
    progress: mpsc::Sender<Value>,
) -> Result<PathBuf> {
    let platform = platform()
        .ok_or_else(|| Error::Unsupported("Antigravity installation on this platform".into()))?;
    let directory = root.join("managed/antigravity");
    private_directory(&directory)?;
    let staging = directory.join(format!("staging-{}", uuid::Uuid::new_v4()));
    private_directory(&staging)?;
    let mut cleanup = InstallationFiles {
        staging: staging.clone(),
        release: None,
        metadata: None,
        activated: false,
    };
    let operation = async {
        cancelled(&cancel)?;
        let _ = progress.try_send(json!({"message":"Checking the official Antigravity release"}));
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(300))
            .build()
            .map_err(|_| invalid())?;
        let response = client
            .get(format!("{UPDATER}/manifests/{platform}.json"))
            .send()
            .await
            .map_err(|_| {
                Error::Provider("Unable to fetch the Antigravity release manifest".into())
            })?
            .error_for_status()
            .map_err(|_| invalid())?;
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            cancelled(&cancel)?;
            let chunk = chunk.map_err(|_| invalid())?;
            if bytes.len() + chunk.len() > 16384 {
                return Err(invalid());
            }
            bytes.extend_from_slice(&chunk);
        }
        let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        let url = download_url(&manifest)?;
        let archive = staging.join("download");
        bounded_download(&client, url, &archive, &cancel, &progress, &manifest.sha512).await?;
        let _ = progress.try_send(json!({"message":"Verifying and installing Antigravity"}));
        let executable = staging.join(binary_name());
        if cfg!(windows) {
            tokio::fs::rename(&archive, &executable).await?;
        } else {
            let archive_for_extract = archive.clone();
            let executable = executable.clone();
            let cancel = cancel.clone();
            tokio::task::spawn_blocking(move || {
                extract_verified_archive(&archive_for_extract, &executable, &cancel)
            })
            .await
            .map_err(|_| invalid())??;
            tokio::fs::remove_file(&archive).await?;
        }
        cancelled(&cancel)?;
        // Each install gets its own immutable directory, preserving the active release on failure.
        let release = format!("{}-{}", manifest.version, uuid::Uuid::new_v4());
        cleanup.release = Some(directory.join(&release));
        std::fs::rename(&staging, directory.join(&release))?;
        let active = Active {
            version: manifest.version,
            executable: PathBuf::from(&release).join(binary_name()),
            sdk_worker: None,
            auth_worker: None,
        };
        let temporary = directory.join(format!("active-{}.tmp", uuid::Uuid::new_v4()));
        cleanup.metadata = Some(temporary.clone());
        let publish = async {
            let mut options = tokio::fs::OpenOptions::new();
            options.create_new(true).write(true);
            #[cfg(unix)]
            options.mode(0o600);
            let mut output = options.open(&temporary).await?;
            output.write_all(&serde_json::to_vec(&active)?).await?;
            output.sync_all().await?;
            cancelled(&cancel)?;
            // The commit has no cancellation point between activation and its receipt.
            std::fs::rename(&temporary, directory.join("active.json"))?;
            cleanup.activated = true;
            Ok::<_, Error>(directory.join(&active.executable))
        }
        .await;
        if publish.is_err() {
            let _ = tokio::fs::remove_file(&temporary).await;
            let _ = tokio::fs::remove_dir_all(directory.join(release)).await;
        }
        publish
    };
    let cancellation = async {
        while !cancel.load(Ordering::Acquire) {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    };
    let result = tokio::select! {
        biased;
        result = operation => result,
        _ = cancellation => Err(Error::Provider("Installation cancelled".into())),
    };
    if staging.exists() {
        let _ = tokio::fs::remove_dir_all(&staging).await;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn abandoned_installation_preserves_active_release_and_cleans_owned_files() {
        let root =
            std::env::temp_dir().join(format!("swarmie-install-cleanup-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("active-release")).unwrap();
        let staging = root.join("staging");
        let release = root.join("new-release");
        let metadata = root.join("metadata.tmp");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::create_dir_all(&release).unwrap();
        std::fs::write(&metadata, b"fixture").unwrap();
        drop(InstallationFiles {
            staging: staging.clone(),
            release: Some(release.clone()),
            metadata: Some(metadata.clone()),
            activated: false,
        });
        assert!(!staging.exists());
        assert!(!release.exists());
        assert!(!metadata.exists());
        assert!(root.join("active-release").is_dir());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[cfg(unix)]
    fn managed_release_supplies_private_workers_and_rejects_outside_paths() {
        let root =
            std::env::temp_dir().join(format!("swarmie-managed-overlay-{}", uuid::Uuid::new_v4()));
        let directory = root.join("managed/claude");
        private_directory(&directory.join("release")).unwrap();
        for name in ["claude", "worker.mjs"] {
            std::fs::write(directory.join("release").join(name), b"fixture").unwrap();
        }
        let mut active = Active {
            version: "2.1.300".into(),
            executable: "release/claude".into(),
            sdk_worker: Some("release/worker.mjs".into()),
            auth_worker: None,
        };
        std::fs::write(
            directory.join("active.json"),
            serde_json::to_vec(&active).unwrap(),
        )
        .unwrap();
        let overlay = managed_overlay(&root, "claude");
        assert!(
            overlay.config["executable"]
                .as_str()
                .unwrap()
                .ends_with("release/claude")
        );
        assert!(
            overlay.config["sdkWorkerPath"]
                .as_str()
                .unwrap()
                .ends_with("release/worker.mjs")
        );
        active.sdk_worker = Some("../outside.mjs".into());
        std::fs::write(
            directory.join("active.json"),
            serde_json::to_vec(&active).unwrap(),
        )
        .unwrap();
        assert!(
            managed_overlay(&root, "claude")
                .config
                .as_object()
                .unwrap()
                .is_empty()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn downloaded_payload_requires_exact_digest() {
        use tokio::io::AsyncReadExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for _ in 0..3 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0u8; 2048];
                let _ = socket.read(&mut request).await.unwrap();
                socket
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nfixture",
                    )
                    .await
                    .unwrap();
            }
        });
        let root =
            std::env::temp_dir().join(format!("swarmie-install-download-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let (progress, _) = mpsc::channel(8);
        let client = reqwest::Client::new();
        let url = reqwest::Url::parse(&format!("http://{address}/fixture")).unwrap();
        let hash = format!("{:x}", Sha512::digest(b"fixture"));
        bounded_download(
            &client,
            url.clone(),
            &root.join("valid"),
            &AtomicBool::new(false),
            &progress,
            &hash,
        )
        .await
        .unwrap();
        assert!(
            bounded_download(
                &client,
                url,
                &root.join("corrupt"),
                &AtomicBool::new(false),
                &progress,
                &"0".repeat(128)
            )
            .await
            .is_err()
        );
        bounded_download(
            &client,
            reqwest::Url::parse(&format!("http://{address}/fixture")).unwrap(),
            &root.join("sha256"),
            &AtomicBool::new(false),
            &progress,
            &format!("{:x}", Sha256::digest(b"fixture")),
        )
        .await
        .unwrap();
        server.await.unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn manifest_rejects_untrusted_locations_and_unsafe_versions() {
        let mut manifest = Manifest {
            version: "1.3.5".into(),
            url:
                "https://storage.googleapis.com/antigravity-public/antigravity-cli/1.3.5/cli.tar.gz"
                    .into(),
            sha512: "a".repeat(128),
        };
        assert!(download_url(&manifest).is_ok());
        for url in [
            "https://evil.example/file",
            "http://storage.googleapis.com/antigravity-public/antigravity-cli/file",
            "https://storage.googleapis.com/other/file",
            "https://token@storage.googleapis.com/antigravity-public/antigravity-cli/file",
            "https://storage.googleapis.com/antigravity-public/antigravity-cli/file?token=secret",
        ] {
            manifest.url = url.into();
            assert!(download_url(&manifest).is_err());
        }
        manifest.version = "../outside".into();
        assert!(download_url(&manifest).is_err());
    }
    #[test]
    fn archive_extracts_only_regular_documented_binary() {
        let root = std::env::temp_dir().join(format!("swarmie-install-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("archive.gz");
        let gzip = flate2::write::GzEncoder::new(
            std::fs::File::create(&path).unwrap(),
            flate2::Compression::fast(),
        );
        let mut builder = tar::Builder::new(gzip);
        let mut header = tar::Header::new_gnu();
        header.set_size(7);
        header.set_mode(0o700);
        header.set_cksum();
        builder
            .append_data(&mut header, "antigravity", &b"fixture"[..])
            .unwrap();
        builder.into_inner().unwrap().finish().unwrap();
        extract_verified_archive(&path, &root.join("agy"), &AtomicBool::new(false)).unwrap();
        assert_eq!(std::fs::read(root.join("agy")).unwrap(), b"fixture");
        assert!(
            extract_verified_archive(&path, &root.join("cancelled"), &AtomicBool::new(true))
                .is_err()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
