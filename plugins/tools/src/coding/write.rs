//! The `write` tool.

use std::path::PathBuf;
use std::sync::Arc;

use crate::ToolAnnotations;
use crate::{
    ChangeKind, FileChange, FilesChanged, Scope, Tool, ToolContext, ToolExecutionError, Touches,
    args_schema,
};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::coding::fs::{Previous, io_error};
use crate::coding::workspace::{Local, Workspace};
use crate::coding::{MAX_FILE_BYTES, Seen, blocking};

/// Writes a whole file, creating its parent directories. Publishes the file
/// it changed ([`FilesChanged`]): its [resolved](Workspace) path,
/// and the diff. The text it wrote counts as [`Seen`] whole, so an edit of
/// the file starts from it; a write that failed forgets the file.
#[derive(Clone, Debug)]
pub struct Write {
    /// Where the file is written.
    pub workspace: Arc<dyn Workspace>,
    /// The directory a relative path is written in; `None` is the
    /// workspace's working directory.
    pub base: Option<PathBuf>,
    /// Whether a path that resolves outside `base` is refused, before
    /// anything is written.
    pub confine: bool,
    /// The files seen, shared with [`Read`](crate::coding::Read) and
    /// [`Edit`](crate::coding::Edit).
    pub seen: Seen,
}

impl Default for Write {
    /// Writes this machine's files, from this process's working directory.
    fn default() -> Self {
        Self {
            workspace: Arc::new(Local),
            base: None,
            confine: false,
            seen: Seen::default(),
        }
    }
}

/// Arguments of [`Write`].
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WriteArgs {
    /// The file to write.
    pub path: String,
    /// The whole new content.
    pub content: String,
}

impl WriteArgs {
    /// Write `content` to `path`.
    pub fn new(path: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            content: content.into(),
        }
    }
}

impl Tool for Write {
    const NAME: &'static str = "write";
    type Args = WriteArgs;
    type Output = String;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Write a file with the given content, replacing it if it exists and creating parent \
         directories. Use it for new files and complete rewrites only; change part of a file \
         with `edit`."
            .to_owned()
    }

    fn parameters(&self) -> serde_json::Value {
        args_schema::<WriteArgs>()
    }

    fn annotations(&self) -> ToolAnnotations {
        ToolAnnotations::default()
            .with_destructive(true)
            .with_idempotent(true)
            .with_open_world(false)
    }

    fn footprint(&self, args: &WriteArgs) -> Touches {
        Touches::Writes(Scope::Paths(vec![
            self.workspace.resolve(self.base.as_deref(), &args.path),
        ]))
    }

    async fn call(
        &self,
        context: &mut ToolContext,
        args: WriteArgs,
    ) -> Result<String, ToolExecutionError> {
        let resolved = self
            .workspace
            .place(self.base.as_deref(), self.confine, &args.path)?;
        let workspace = self.workspace.clone();
        let seen = self.seen.clone();
        let (output, change) = blocking(move || {
            let path = resolved.to_string_lossy();
            let before = workspace
                .replace(&resolved, args.content.as_bytes(), MAX_FILE_BYTES)
                .map_err(|error| {
                    seen.forget(&resolved);
                    io_error(&path, error)
                })?;
            seen.saw_whole(&resolved, &args.content);
            let before = Previous::read(before);
            let change = Previous::change(before, &path, Some(args.content.as_bytes()));
            Ok::<_, ToolExecutionError>((wrote(&args.path, &change), change))
        })
        .await?;
        context.insert_result(FilesChanged(vec![change]))?;
        Ok(output)
    }
}

/// The reply to a write of `path` that made `change`: that it created the
/// file, or replaced it and, when it was read, how many lines it had.
fn wrote(path: &str, change: &FileChange) -> String {
    let had = change.before.as_ref().and_then(|before| before.lines);
    match (change.kind, had) {
        (ChangeKind::Created, _) => format!("Created {path}."),
        (_, Some(1)) => format!("Replaced {path}, which had 1 line."),
        (_, Some(lines)) => format!("Replaced {path}, which had {lines} lines."),
        (_, None) => format!("Replaced {path}."),
    }
}

#[cfg(test)]
mod tests {
    use crate::{ChangeKind, FileChange, FileState, FilesChanged, Tool, ToolContext, content_hash};

    use super::{Write, WriteArgs};
    use crate::coding::{Edit, EditArgs, Replacement};

    /// Writing and editing a file publish what they did to it, by its absolute
    /// path: created with its lines, then modified with the lines added and
    /// removed, each with its hunks and the text it was made to. Each reply
    /// says only what the model could not know: that the file was created or
    /// replaced, and which lines an edit changed.
    #[test]
    fn writes_and_edits_publish_the_files_they_changed() {
        let write_tool = Write::default();
        let edit_tool = Edit::default();
        let dir =
            std::env::temp_dir().join(format!("swarmie-tools-changes-{}", std::process::id()));
        let path = crate::coding::fs::resolve(Some(&dir), "a.txt")
            .to_string_lossy()
            .into_owned();
        let changed = |context: &ToolContext| {
            context
                .result::<FilesChanged>()
                .ok()
                .flatten()
                .unwrap_or_default()
        };

        let mut context = ToolContext::new();
        let written = Tool::call(
            &write_tool,
            &mut context,
            WriteArgs::new(&path, "one\ntwo\n"),
        );
        let reply = futures::executor::block_on(written).unwrap_or_default();
        assert_eq!(reply, format!("Created {path}."));
        let created = changed(&context).0;
        assert_eq!(
            created.iter().map(FileChange::unified).collect::<Vec<_>>(),
            [format!(
                "--- /dev/null\n+++ {path}\n@@ -0,0 +1,2 @@\n+one\n+two\n"
            )]
        );
        let without_hunks = |change: FileChange| FileChange {
            hunks: Vec::new(),
            ..change
        };
        assert_eq!(
            created.into_iter().map(without_hunks).collect::<Vec<_>>(),
            [FileChange::created(&path, 2).with_after(FileState::of(b"one\ntwo\n"))]
        );

        let mut context = ToolContext::new();
        let rewritten = Tool::call(
            &write_tool,
            &mut context,
            WriteArgs::new(&path, "one\n2\nthree\n"),
        );
        let reply = futures::executor::block_on(rewritten).unwrap_or_default();
        assert_eq!(reply, format!("Replaced {path}, which had 2 lines."));
        let change = changed(&context).0.into_iter().next();
        assert_eq!(
            change.map(|change| (change.kind, change.added, change.removed)),
            Some((ChangeKind::Modified, 2, 1))
        );

        let mut context = ToolContext::new();
        let edits = [
            Replacement::new("one", "1"),
            Replacement::new("three", "3").all(),
        ];
        let edited = Tool::call(&edit_tool, &mut context, EditArgs::new(&path, edits));
        let reply = futures::executor::block_on(edited).unwrap_or_default();
        let said = reply.lines().next();
        assert_eq!(said, Some(format!("Edited {path}: lines 1, 3.").as_str()));
        let change = changed(&context).0.into_iter().next();
        assert_eq!(
            change
                .as_ref()
                .map(|change| (change.path.as_str(), change.added, change.removed)),
            Some((path.as_str(), 2, 2))
        );
        assert_eq!(
            change
                .as_ref()
                .and_then(|change| change.after.clone()?.hash),
            Some(content_hash(b"1\n2\n3\n"))
        );
        assert_eq!(
            change.as_ref().map(FileChange::unified),
            Some(format!(
                "--- {path}\n+++ {path}\n@@ -1,3 +1,3 @@\n-one\n+1\n 2\n-three\n+3\n"
            ))
        );
        assert_eq!(
            change
                .as_ref()
                .and_then(|change| change.before_text.as_deref()),
            Some("one\n2\nthree\n")
        );
        assert_eq!(
            change.and_then(|change| change.after_text()).as_deref(),
            Some("1\n2\n3\n")
        );
        assert_eq!(
            std::fs::read_to_string(&path).ok().as_deref(),
            Some("1\n2\n3\n")
        );
        std::fs::remove_dir_all(dir).ok();
    }
}
