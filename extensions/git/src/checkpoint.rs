use crate::repository::{git, git_index, utf8};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use swarmie::{Error, Result};
use tokio::sync::Mutex;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Checkpoint {
    pub turn_id: String,
    pub base: String,
    pub end: Option<String>,
    pub path: PathBuf,
}
#[derive(Default)]
pub struct Checkpoints {
    pub pending: Mutex<BTreeMap<String, (PathBuf, String)>>,
}
fn file(root: &Path, session: &str) -> Result<PathBuf> {
    uuid::Uuid::parse_str(session)
        .map_err(|_| Error::Invalid("invalid session identifier".into()))?;
    Ok(root.join(format!("{session}.json")))
}
pub async fn load(root: &Path, session: &str) -> Result<Vec<Checkpoint>> {
    match tokio::fs::read(file(root, session)?).await {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(e.into()),
    }
}
pub async fn save(root: &Path, session: &str, checkpoints: &[Checkpoint]) -> Result<()> {
    tokio::fs::create_dir_all(root).await?;
    let path = file(root, session)?;
    let temporary = path.with_extension("tmp");
    tokio::fs::write(&temporary, serde_json::to_vec(checkpoints)?).await?;
    tokio::fs::rename(temporary, path).await?;
    Ok(())
}
/// A separate index captures the worktree without changing the user's staged files.
pub async fn capture(root: &Path, state_root: &Path) -> Result<String> {
    tokio::fs::create_dir_all(state_root).await?;
    let index = state_root.join(format!("index-{}", uuid::Uuid::new_v4()));
    let result = async {
        git_index(root, &["read-tree", "HEAD"], &index).await?;
        git_index(root, &["add", "-A", "--", "."], &index).await?;
        let tree = git_index(root, &["write-tree"], &index).await?;
        let tree = utf8(&tree)?.trim().to_owned();
        // Retain captured objects through normal Git garbage collection.
        git(
            root,
            &[
                "update-ref",
                &format!("refs/swarmie/checkpoints/{tree}"),
                &tree,
            ],
            false,
        )
        .await?;
        Ok(tree)
    }
    .await;
    let _ = tokio::fs::remove_file(index).await;
    result
}
