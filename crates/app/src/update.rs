use clap::Subcommand;
use swarmie::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::io::AsyncWriteExt;

const MANIFEST_LIMIT: usize = 64 * 1024;
const ARTIFACT_LIMIT: u64 = 512 * 1024 * 1024;
#[derive(Subcommand)]
pub enum UpdateCommand {
    /// Check the release selected by the manifest; nothing is downloaded or installed.
    Check {
        #[arg(long, env = "SWARMIE_UPDATE_MANIFEST")]
        manifest: String,
    },
    /// Download a release archive, verify SHA-256, and stage it without installing.
    Stage {
        #[arg(long, env = "SWARMIE_UPDATE_MANIFEST")]
        manifest: String,
        #[arg(long)]
        directory: Option<PathBuf>,
    },
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    version: String,
    artifacts: BTreeMap<String, Artifact>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Artifact {
    url: String,
    sha256: String,
    file_name: String,
}
/// First installation follows the client's release rather than an unrelated latest runtime.
pub(crate) fn runtime_manifest_url() -> String {
    format!(
        "https://github.com/syntropika/swarmie/releases/download/v{}/runtime-manifest.json",
        env!("CARGO_PKG_VERSION")
    )
}
fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}
fn https_url(value: &str) -> Result<reqwest::Url> {
    let url =
        reqwest::Url::parse(value).map_err(|_| Error::Invalid("invalid update URL".into()))?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::Invalid(
            "updates require an HTTPS URL without credentials or a fragment".into(),
        ));
    }
    Ok(url)
}
fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .connect_timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 {
                attempt.error("too many update redirects")
            } else if https_url(attempt.url().as_str()).is_err() {
                attempt.error("update redirect requires HTTPS")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|e| Error::Provider(e.to_string()))
}
fn validate_manifest(manifest: &Manifest) -> Result<(semver::Version, &Artifact)> {
    validate_manifest_for(manifest, &platform())
}
fn validate_manifest_for<'a>(manifest: &'a Manifest, target: &str) -> Result<(semver::Version, &'a Artifact)> {
    let version = semver::Version::parse(&manifest.version)
        .map_err(|_| Error::Invalid("release version must use semantic versioning".into()))?;
    let artifact = manifest
        .artifacts
        .get(target)
        .ok_or_else(|| Error::Unsupported(format!("release does not include {target}")))?;
    https_url(&artifact.url)?;
    if artifact.sha256.len() != 64 || !artifact.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::Invalid(
            "artifact SHA-256 must contain 64 hexadecimal characters".into(),
        ));
    }
    let name = &artifact.file_name;
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.len() > 200
        || name
            .chars()
            .any(|c| c.is_control() || c == '/' || c == '\\')
    {
        return Err(Error::Invalid(
            "artifact fileName must be a single printable file name".into(),
        ));
    }
    Ok((version, artifact))
}
async fn fetch_manifest(client: &reqwest::Client, url: &str) -> Result<Manifest> {
    let response = client
        .get(https_url(url)?)
        .send()
        .await
        .map_err(|e| Error::Provider(e.to_string()))?
        .error_for_status()
        .map_err(|e| Error::Provider(e.to_string()))?;
    read_manifest(response).await
}
async fn read_manifest(mut response: reqwest::Response) -> Result<Manifest> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| Error::Provider(e.to_string()))?
    {
        if bytes.len().saturating_add(chunk.len()) > MANIFEST_LIMIT {
            return Err(Error::Invalid("release manifest exceeds 64 KiB".into()));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(serde_json::from_slice(&bytes)?)
}
async fn stage_response(
    mut response: reqwest::Response,
    artifact: &Artifact,
    directory: &Path,
) -> Result<PathBuf> {
    if response
        .content_length()
        .is_some_and(|n| n > ARTIFACT_LIMIT)
    {
        return Err(Error::Invalid("update archive exceeds 512 MiB".into()));
    }
    tokio::fs::create_dir_all(directory).await?;
    let temporary = directory.join(format!(".{}.partial", uuid::Uuid::new_v4()));
    let result = async {
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&temporary).await?;
        let mut digest = Sha256::new();
        let mut size = 0u64;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| Error::Provider(e.to_string()))?
        {
            size = size.saturating_add(chunk.len() as u64);
            if size > ARTIFACT_LIMIT {
                return Err(Error::Invalid("update archive exceeds 512 MiB".into()));
            }
            digest.update(&chunk);
            file.write_all(&chunk).await?;
        }
        let actual: String = digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if actual != artifact.sha256.to_ascii_lowercase() {
            return Err(Error::Invalid(
                "update archive SHA-256 mismatch; download discarded".into(),
            ));
        }
        file.sync_all().await?;
        drop(file);
        let destination = directory.join(&artifact.file_name);
        // A completed existing archive is never overwritten by an unverified partial download.
        tokio::fs::rename(&temporary, &destination).await?;
        Ok(destination)
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&temporary).await;
    }
    result
}
/// Stage a runtime package for the destination, including the current release on first install.
pub(crate) async fn stage_runtime(url: &str, target: &str, directory: &Path) -> Result<(PathBuf, String, String)> {
    let client = client()?;
    let manifest = fetch_manifest(&client, url).await?;
    let (version, artifact) = validate_manifest_for(&manifest, target)?;
    let response = client.get(https_url(&artifact.url)?).send().await
        .map_err(|e| Error::Provider(e.to_string()))?.error_for_status()
        .map_err(|e| Error::Provider(e.to_string()))?;
    let path = stage_response(response, artifact, directory).await?;
    Ok((path, artifact.sha256.to_ascii_lowercase(), version.to_string()))
}
pub async fn run(root: &Path, command: UpdateCommand) -> Result<()> {
    let (url, directory) = match command {
        UpdateCommand::Check { manifest } => (manifest, None),
        UpdateCommand::Stage {
            manifest,
            directory,
        } => (manifest, Some(directory)),
    };
    let client = client()?;
    let manifest = fetch_manifest(&client, &url).await?;
    let (version, artifact) = validate_manifest(&manifest)?;
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION"))
        .map_err(|e| Error::Invalid(e.to_string()))?;
    let available = version > current;
    let mut result = json!({"currentVersion":current.to_string(),"version":version.to_string(),"platform":platform(),"available":available,"manifest":url,"installed":false});
    if let Some(directory) = directory {
        if !available {
            return Err(Error::Invalid(
                "manifest does not offer a newer release".into(),
            ));
        }
        let directory = directory
            .unwrap_or_else(|| root.parent().unwrap_or(root).join("updates"))
            .join(version.to_string());
        let response = client
            .get(https_url(&artifact.url)?)
            .send()
            .await
            .map_err(|e| Error::Provider(e.to_string()))?
            .error_for_status()
            .map_err(|e| Error::Provider(e.to_string()))?;
        let path = stage_response(response, artifact, &directory).await?;
        result["path"] = path.to_string_lossy().into();
        result["sha256"] = artifact.sha256.clone().into();
        result["status"] = "staged".into();
    } else {
        result["status"] = if available { "available" } else { "up_to_date" }.into();
    }
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_contract_rejects_downgrade_transport_and_traversal() {
        assert!(https_url("http://example.com/manifest.json").is_err());
        assert!(https_url("https://user:secret@example.com/manifest.json").is_err());
        let mut manifest = Manifest {
            version: "0.2.0".into(),
            artifacts: BTreeMap::from([(
                platform(),
                Artifact {
                    url: "https://example.com/swarmie.tar.gz".into(),
                    sha256: "00".repeat(32),
                    file_name: "../swarmie.tar.gz".into(),
                },
            )]),
        };
        assert!(validate_manifest(&manifest).is_err());
        manifest.artifacts.get_mut(&platform()).unwrap().file_name = "swarmie.tar.gz".into();
        assert!(validate_manifest(&manifest).is_ok());
    }
    #[test]
    fn first_install_selects_the_remote_platform_even_at_the_current_release() {
        let target = "remote-platform";
        let manifest = Manifest { version: env!("CARGO_PKG_VERSION").into(), artifacts: BTreeMap::from([
            (target.into(), Artifact { url: "https://example.com/runtime.tar.gz".into(), sha256: "00".repeat(32), file_name: "runtime.tar.gz".into() })
        ]) };
        let (version, artifact) = validate_manifest_for(&manifest, target).unwrap();
        assert_eq!(version.to_string(), env!("CARGO_PKG_VERSION"));
        assert_eq!(artifact.file_name, "runtime.tar.gz");
        assert!(validate_manifest(&manifest).is_err());
    }
    #[tokio::test]
    async fn downloaded_archive_is_only_published_after_verification_and_failed_partials_are_removed()
     {
        let app = axum::Router::new()
            .route(
                "/archive",
                axum::routing::get(|| async { "actual archive bytes" }),
            )
            .route(
                "/large-manifest",
                axum::routing::get(|| async { "x".repeat(MANIFEST_LIMIT + 1) }),
            )
            .route(
                "/large-archive",
                axum::routing::get(|| async {
                    axum::http::Response::builder()
                        .header("content-length", (ARTIFACT_LIMIT + 1).to_string())
                        .body(axum::body::Body::from_stream(
                            futures_util::stream::pending::<
                                std::result::Result<axum::body::Bytes, std::convert::Infallible>,
                            >(),
                        ))
                        .unwrap()
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/archive", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let directory = tempfile::tempdir().unwrap();
        let mut artifact = Artifact {
            url: url.clone(),
            sha256: "00".repeat(32),
            file_name: "swarmie.tar.gz".into(),
        };
        let response = reqwest::get(&url).await.unwrap();
        assert!(
            stage_response(response, &artifact, directory.path())
                .await
                .is_err()
        );
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
        let origin = url.trim_end_matches("/archive");
        assert!(
            read_manifest(
                reqwest::get(format!("{origin}/large-manifest"))
                    .await
                    .unwrap()
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("64 KiB")
        );
        assert!(
            stage_response(
                reqwest::get(format!("{origin}/large-archive"))
                    .await
                    .unwrap(),
                &artifact,
                directory.path()
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("512 MiB")
        );
        artifact.sha256 = Sha256::digest(b"actual archive bytes")
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let path = stage_response(
            reqwest::get(url).await.unwrap(),
            &artifact,
            directory.path(),
        )
        .await
        .unwrap();
        assert_eq!(
            tokio::fs::read(path).await.unwrap(),
            b"actual archive bytes"
        );
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        server.abort();
    }
}
