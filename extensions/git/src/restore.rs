//! Explicit restoration of a retained tree into a clean, managed checkout.
use crate::{GitExtension, checkpoint, repository::git, required};
use serde_json::{Value, json};
use std::path::{Component, Path};
use swarmie::{Error, Result};

pub async fn restore(extension: &GitExtension, root: &Path, p: &Value) -> Result<Value> {
    let tree = extension.tree(root, p).await?;
    let path = tokio::fs::canonicalize(&tree.path).await?;
    if tree.is_main || path.parent() != Some(extension.worktree_root.as_path()) {
        return Err(Error::Invalid(
            "restore requires an extension-managed secondary worktree".into(),
        ));
    }
    if tree.locked {
        return Err(Error::Invalid(
            "unlock the managed worktree before restoring it".into(),
        ));
    }
    if extension
        .live_sessions
        .read()
        .await
        .values()
        .any(|cwd| cwd.starts_with(&path))
    {
        return Err(Error::Busy);
    }
    let status = git(
        &path,
        &["status", "--porcelain=v1", "--untracked-files=all"],
        false,
    )
    .await?;
    if !status.is_empty() {
        return Err(Error::Invalid(
            "commit or preserve all worktree changes before restoring a checkpoint".into(),
        ));
    }
    let points = checkpoint::load(
        &extension.worktree_root.join("checkpoints"),
        required(p, "sessionId")?,
    )
    .await?;
    let point = points
        .into_iter()
        .find(|point| Some(point.turn_id.as_str()) == p["turnId"].as_str())
        .ok_or_else(|| Error::Invalid("turn checkpoint is unavailable".into()))?;
    if tokio::fs::canonicalize(&point.path).await? != path {
        return Err(Error::Invalid(
            "checkpoint belongs to another worktree".into(),
        ));
    }
    let boundary = p["boundary"].as_str().unwrap_or("before");
    let source = match boundary {
        "before" => point.base,
        "after" => point.end.ok_or_else(|| {
            Error::Invalid("after restore requires a completed checkpoint".into())
        })?,
        _ => return Err(Error::Invalid("boundary must be before or after".into())),
    };
    // Reject invalid saved references before a mutation. Captures are tree IDs, not arbitrary refs.
    if source.len() != 40 && source.len() != 64 || !source.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::Invalid(
            "checkpoint contains an invalid tree identifier".into(),
        ));
    }
    let paths = if let Some(paths) = p.get("paths") {
        let paths = paths
            .as_array()
            .ok_or_else(|| Error::Invalid("paths must be an array".into()))?;
        if paths.is_empty() || paths.len() > 128 {
            return Err(Error::Invalid(
                "provide between one and 128 literal file paths".into(),
            ));
        }
        paths
            .iter()
            .map(|value| {
                let value = value
                    .as_str()
                    .ok_or_else(|| Error::Invalid("paths must contain strings".into()))?;
                if value.is_empty()
                    || value.contains('\0')
                    || Path::new(value)
                        .components()
                        .any(|c| !matches!(c, Component::Normal(_)))
                {
                    return Err(Error::Invalid("provide literal relative file paths".into()));
                }
                Ok(value.to_owned())
            })
            .collect::<Result<Vec<_>>>()?
    } else {
        vec![".".into()]
    };
    let source_argument = format!("--source={source}");
    let mut args = vec!["restore", &source_argument, "--staged", "--worktree", "--"];
    args.extend(paths.iter().map(String::as_str));
    git(&path, &args, false).await?;
    Ok(
        json!({"restored":true,"boundary":boundary,"base":source,"paths":paths,"branchChanged":false}),
    )
}
