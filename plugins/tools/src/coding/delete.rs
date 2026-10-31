//! The `delete` tool.

use std::path::PathBuf;
use std::sync::Arc;

use crate::ToolAnnotations;
use crate::{FilesChanged, Scope, Tool, ToolContext, ToolExecutionError, Touches, args_schema};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::coding::blocking;
use crate::coding::fs::{Previous, io_error};
use crate::coding::workspace::{Local, Stat, Workspace};

/// Deletes a file; a directory is refused. Publishes the file it deleted
/// ([`FilesChanged`]): its [resolved](Workspace) path, and how many
/// lines it had.
#[derive(Clone, Debug)]
pub struct Delete {
    /// Where the file is deleted.
    pub workspace: Arc<dyn Workspace>,
    /// The directory a relative path is deleted in; `None` is the
    /// workspace's working directory.
    pub base: Option<PathBuf>,
    /// Whether a path that resolves outside `base` is refused, before
    /// anything is deleted.
    pub confine: bool,
}

impl Default for Delete {
    /// Deletes this machine's files, from this process's working directory.
    fn default() -> Self {
        Self {
            workspace: Arc::new(Local),
            base: None,
            confine: false,
        }
    }
}

/// Arguments of [`Delete`].
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteArgs {
    /// The file to delete.
    pub path: String,
}

impl DeleteArgs {
    /// Delete `path`.
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }
}

impl Tool for Delete {
    const NAME: &'static str = "delete";
    type Args = DeleteArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Delete a file, not with `rm` in `shell`. Directories are refused.".to_owned()
    }

    fn parameters(&self) -> serde_json::Value {
        args_schema::<DeleteArgs>()
    }

    fn annotations(&self) -> ToolAnnotations {
        ToolAnnotations::default()
            .with_destructive(true)
            .with_idempotent(false)
            .with_open_world(false)
    }

    fn footprint(&self, args: &DeleteArgs) -> Touches {
        Touches::Writes(Scope::Paths(vec![
            self.workspace.resolve(self.base.as_deref(), &args.path),
        ]))
    }

    async fn call(
        &self,
        context: &mut ToolContext,
        args: DeleteArgs,
    ) -> Result<String, ToolExecutionError> {
        let resolved = self
            .workspace
            .place(self.base.as_deref(), self.confine, &args.path)?;
        let workspace = self.workspace.clone();
        let change = blocking(move || {
            let path = resolved.to_string_lossy();
            match workspace
                .stat(&resolved)
                .map_err(|error| io_error(&path, error))?
            {
                Stat::File { .. } => {}
                Stat::Missing => {
                    return Err(ToolExecutionError::not_found(format!(
                        "{path}: no such file"
                    )));
                }
                Stat::Dir | Stat::Other => {
                    return Err(ToolExecutionError::invalid_args(format!(
                        "{path} is not a regular file; `delete` removes files only"
                    )));
                }
            }
            let previous = workspace.previous(&resolved).unwrap_or_default();
            workspace
                .remove(&resolved)
                .map_err(|error| io_error(&path, error))?;
            Ok(Previous::change(Some(previous), &path, None))
        })
        .await?;
        context.insert_result(FilesChanged(vec![change]))?;
        Ok(format!("Deleted {}.", args.path))
    }
}

#[cfg(test)]
mod tests {
    use crate::{FileChange, FileState, FilesChanged, Tool, ToolContext, ToolErrorKind};

    use super::{Delete, DeleteArgs};

    /// A confined delete refuses a path outside its base and leaves the file;
    /// inside it, it removes the file and publishes it deleted, with its lines.
    /// A directory is refused.
    #[test]
    fn a_delete_stays_in_its_base_and_publishes_the_deleted_file() {
        let dir = std::env::temp_dir().join(format!("swarmie-tools-delete-{}", std::process::id()));
        let dir = {
            std::fs::create_dir_all(dir.join("base").join("sub")).ok();
            std::fs::canonicalize(&dir).unwrap_or(dir)
        };
        let base = dir.join("base");
        std::fs::write(dir.join("outside.txt"), "keep\n").ok();
        std::fs::write(base.join("a.txt"), "one\ntwo\n").ok();
        let delete = Delete {
            base: Some(base.clone()),
            confine: true,
            ..Delete::default()
        };
        let run = |path: &str| {
            let mut context = ToolContext::new();
            let output =
                futures::executor::block_on(delete.call(&mut context, DeleteArgs::new(path)));
            let changed: Option<FilesChanged> = context.result().ok().flatten();
            (output, changed)
        };

        let (refused, changed) = run("../outside.txt");
        assert_eq!(
            refused.err().map(|error| error.kind()),
            Some(ToolErrorKind::PermissionDenied)
        );
        assert_eq!(changed, None);
        assert!(dir.join("outside.txt").exists());

        let (deleted, changed) = run("a.txt");
        assert_eq!(deleted.ok().as_deref(), Some("Deleted a.txt."));
        let path = base.join("a.txt").to_string_lossy().into_owned();
        let changes = changed.map(|changed| changed.0).unwrap_or_default();
        assert_eq!(
            changes.iter().map(FileChange::unified).collect::<Vec<_>>(),
            [format!(
                "--- {path}\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-one\n-two\n"
            )]
        );
        let without_hunks = |change: FileChange| FileChange {
            hunks: Vec::new(),
            ..change
        };
        assert_eq!(
            changes.into_iter().map(without_hunks).collect::<Vec<_>>(),
            [FileChange::deleted(&path, 2)
                .with_before_text("one\ntwo\n")
                .with_before(FileState::of(b"one\ntwo\n"))]
        );
        assert!(!base.join("a.txt").exists());
        assert!(run("sub").0.is_err());
        assert!(base.join("sub").is_dir());
        std::fs::remove_dir_all(dir).ok();
    }
}
