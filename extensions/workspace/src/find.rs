use crate::WorkspaceExtension;
use serde_json::{Value, json};
use swarmie::{Error, Result};
pub async fn files(extension: &WorkspaceExtension, p: &Value) -> Result<Value> {
    let root = extension.project(p).await?;
    let query = p["query"].as_str().unwrap_or("").to_lowercase();
    if query.len() > 1024 {
        return Err(Error::Invalid("query exceeds 1024 bytes".into()));
    }
    let limit = p["limit"].as_u64().unwrap_or(50).clamp(1, 200) as usize;
    let mut directories = vec![root.clone()];
    let mut scanned = 0usize;
    let mut entries = vec![];
    let mut truncated = false;
    while let Some(path) = directories.pop() {
        let mut directory = tokio::fs::read_dir(path).await?;
        while let Some(entry) = directory.next_entry().await? {
            scanned += 1;
            if scanned > 20_000 {
                truncated = true;
                break;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let kind = entry.file_type().await?;
            if kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            let relative = path
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .to_string();
            if kind.is_dir() {
                if !name.starts_with('.')
                    && !matches!(
                        name.as_str(),
                        "node_modules" | "target" | "dist" | "build" | "vendor" | "__pycache__"
                    )
                {
                    directories.push(path);
                }
                continue;
            }
            if relative.to_lowercase().contains(&query) {
                entries.push(json!({"path":relative,"name":name,"kind":"file","size":entry.metadata().await?.len()}));
                if entries.len() == limit {
                    truncated = true;
                    break;
                }
            }
        }
        if truncated {
            break;
        }
    }
    entries.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    let paths = entries
        .iter()
        .filter_map(|entry| entry["path"].as_str())
        .collect::<Vec<_>>();
    Ok(json!({"paths":paths,"entries":entries,"truncated":truncated,"scanned":scanned}))
}
