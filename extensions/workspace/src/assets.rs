use crate::{WorkspaceExtension, relative, required};
use base64::{Engine, engine::general_purpose::STANDARD};
use swarmie::{Error, Result};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
pub const MAX_ASSET: u64 = 8 * 1024 * 1024;
const URI_ESCAPE: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS
    .add(b' ')
    .add(b'?')
    .add(b'#')
    .add(b'%');
pub fn mime(path: &Path, bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return "image/png";
    }
    if bytes.starts_with(b"\xff\xd8\xff") {
        return "image/jpeg";
    }
    if bytes.starts_with(b"GIF8") {
        return "image/gif";
    }
    if bytes.starts_with(b"%PDF-") {
        return "application/pdf";
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        return "image/webp";
    }
    match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "html" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "text/javascript",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "txt" | "md" | "rs" | "py" | "ts" | "tsx" => "text/plain",
        _ => {
            if std::str::from_utf8(bytes).is_ok() && !bytes.contains(&0) {
                "text/plain"
            } else {
                "application/octet-stream"
            }
        }
    }
}
pub async fn resolve(extension: &WorkspaceExtension, p: &Value) -> Result<PathBuf> {
    if let Some(id) = p["assetId"].as_str() {
        if let Some(path) = extension.assets.read().await.get(id).cloned() {
            return Ok(path);
        }
        if uuid::Uuid::parse_str(id).is_err() {
            return Err(Error::Invalid("assetId must be a UUID".into()));
        }
        let bytes = tokio::fs::read(
            extension
                .storage_root
                .join("asset-index")
                .join(format!("{id}.json")),
        )
        .await?;
        if bytes.len() > 16 * 1024 {
            return Err(Error::Invalid("invalid saved asset reference".into()));
        }
        let path: PathBuf = serde_json::from_slice(&bytes)?;
        let path = tokio::fs::canonicalize(path).await?;
        let root = tokio::fs::canonicalize(extension.storage_root.join("assets")).await?;
        if !path.starts_with(root) {
            return Err(Error::Invalid("saved asset leaves private storage".into()));
        }
        extension
            .assets
            .write()
            .await
            .insert(id.into(), path.clone());
        return Ok(path);
    }
    let root = extension.project(p).await?;
    extension.path(&root, required(p, "path")?).await
}
pub async fn register(extension: &WorkspaceExtension, path: PathBuf) -> Result<Value> {
    let id = uuid::Uuid::new_v4().to_string();
    extension.ensure_storage().await?;
    let index = extension.storage_root.join("asset-index");
    tokio::fs::create_dir_all(&index).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&index, std::fs::Permissions::from_mode(0o700)).await?;
    }
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(index.join(format!("{id}.json"))).await?;
    file.write_all(&serde_json::to_vec(&path)?).await?;
    file.flush().await?;
    file.sync_all().await?;
    extension
        .assets
        .write()
        .await
        .insert(id.clone(), path.clone());
    let mut metadata = metadata(&path).await?;
    metadata["assetId"] = json!(id);
    Ok(metadata)
}
pub async fn output_path(extension: &WorkspaceExtension, name: &str) -> Result<PathBuf> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', '\0'])
        || name.contains(char::is_control)
    {
        return Err(Error::Invalid("asset name must be one file name".into()));
    }
    extension.ensure_storage().await?;
    let directory = extension
        .storage_root
        .join("assets")
        .join(uuid::Uuid::new_v4().to_string());
    tokio::fs::create_dir_all(&directory).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).await?;
    }
    Ok(directory.join(name))
}
pub async fn bytes(path: &Path) -> Result<Vec<u8>> {
    let size = tokio::fs::metadata(path).await?.len();
    if size > MAX_ASSET {
        return Err(Error::Invalid("asset exceeds 8 MiB".into()));
    }
    let mut data = vec![];
    tokio::fs::File::open(path)
        .await?
        .take(MAX_ASSET + 1)
        .read_to_end(&mut data)
        .await?;
    if data.len() as u64 > MAX_ASSET {
        return Err(Error::Invalid("asset exceeds 8 MiB".into()));
    }
    Ok(data)
}
pub async fn metadata(path: &Path) -> Result<Value> {
    let info = tokio::fs::metadata(path).await?;
    if !info.is_file() {
        return Err(Error::Invalid("asset must be a file".into()));
    }
    let mut header = vec![];
    tokio::fs::File::open(path)
        .await?
        .take(4096)
        .read_to_end(&mut header)
        .await?;
    let mime = mime(path, &header);
    let mut result = json!({"uri":format!("file://{}",percent_encoding::utf8_percent_encode(&path.to_string_lossy(),URI_ESCAPE)),"path":path,"name":path.file_name().map(|v|v.to_string_lossy()),"size":info.len(),"mime":mime,"kind":if mime.starts_with("image/"){"image"}else if mime.starts_with("video/"){"video"}else if mime.starts_with("audio/"){"audio"}else if mime=="application/pdf"{"pdf"}else if mime.starts_with("text/"){"text"}else{"binary"},"previewable":info.len()<=MAX_ASSET});
    if mime == "image/png" && header.len() >= 24 {
        result["width"] = json!(u32::from_be_bytes(header[16..20].try_into().unwrap()));
        result["height"] = json!(u32::from_be_bytes(header[20..24].try_into().unwrap()));
    }
    Ok(result)
}
pub async fn invoke(extension: &WorkspaceExtension, command: &str, p: &Value) -> Result<Value> {
    if command == "assets.write" || command == "assets.export" {
        let root = extension.project(p).await?;
        let data = p["data"]
            .as_str()
            .ok_or_else(|| Error::Invalid("data must be a base64 string".into()))?;
        if data.len() > ((MAX_ASSET as usize).div_ceil(3)) * 4 {
            return Err(Error::Invalid("asset exceeds 8 MiB".into()));
        }
        let data = STANDARD
            .decode(data)
            .map_err(|_| Error::Invalid("asset data must be base64".into()))?;
        if data.len() as u64 > MAX_ASSET {
            return Err(Error::Invalid("asset exceeds 8 MiB".into()));
        }
        let path = if command == "assets.export" {
            let destination = relative(required(p, "path")?)?;
            let name = destination
                .file_name()
                .ok_or_else(|| Error::Invalid("choose a file name".into()))?;
            let parent =
                tokio::fs::canonicalize(root.join(destination.parent().unwrap_or(Path::new(""))))
                    .await?;
            if !parent.starts_with(&root) {
                return Err(Error::Invalid("destination leaves the workspace".into()));
            }
            parent.join(name)
        } else {
            output_path(extension, required(p, "name")?).await?
        };
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&path).await?;
        file.write_all(&data).await?;
        file.flush().await?;
        file.sync_all().await?;
        drop(file);
        return if command == "assets.export" {
            metadata(&path).await
        } else {
            register(extension, path).await
        };
    }
    let path = resolve(extension, p).await?;
    let mut result = metadata(&path).await?;
    if command == "assets.read" {
        result["data"] = json!(STANDARD.encode(bytes(&path).await?));
    }
    Ok(result)
}
