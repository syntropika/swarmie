use swarmie::{Error, Result};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command};

const MAX_OUTPUT: u64 = 8 * 1024 * 1024;

pub async fn git(cwd: &Path, args: &[&str], diff_exit: bool) -> Result<Vec<u8>> {
    git_inner(cwd, args, diff_exit, None).await
}
pub async fn git_index(cwd: &Path, args: &[&str], index: &Path) -> Result<Vec<u8>> {
    git_inner(cwd, args, false, Some(index)).await
}
async fn git_inner(
    cwd: &Path,
    args: &[&str],
    diff_exit: bool,
    index: Option<&Path>,
) -> Result<Vec<u8>> {
    let mut command = Command::new("git");
    command
        .arg("--literal-pathspecs")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.quotePath=false",
            "-c",
            "core.fsmonitor=false",
        ])
        .args(args)
        .current_dir(cwd)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    for key in std::env::vars_os()
        .map(|(key, _)| key)
        .filter(|key| key.to_string_lossy().starts_with("GIT_"))
    {
        command.env_remove(key);
    }
    command
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0");
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    let mut child = command.spawn()?;
    let mut stdout = child.stdout.take().unwrap().take(MAX_OUTPUT + 1);
    let mut stderr = child.stderr.take().unwrap().take(MAX_OUTPUT + 1);
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        let mut out = vec![];
        let mut err = vec![];
        let (a, b) = tokio::join!(stdout.read_to_end(&mut out), stderr.read_to_end(&mut err));
        a?;
        b?;
        if out.len() as u64 > MAX_OUTPUT || err.len() as u64 > MAX_OUTPUT {
            return Err(Error::Invalid("Git output exceeds the 8 MiB limit".into()));
        }
        let status = child.wait().await?;
        if !(status.success() || diff_exit && status.code() == Some(1)) {
            return Err(Error::Provider(format!(
                "Git: {}",
                String::from_utf8_lossy(&err).trim()
            )));
        }
        Ok(out)
    })
    .await
    .map_err(|_| Error::Timeout("Git command".into()))?;
    if result.is_err() {
        let _ = child.kill().await;
    }
    result
}

pub fn utf8(bytes: &[u8]) -> Result<&str> {
    std::str::from_utf8(bytes).map_err(|_| Error::Unsupported("non-UTF-8 Git paths or text".into()))
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    pub id: String,
    pub path: PathBuf,
    pub branch: Option<String>,
    pub head: String,
    pub locked: bool,
    pub prunable: bool,
    pub is_main: bool,
}

pub async fn worktrees(root: &Path) -> Result<Vec<Worktree>> {
    let output = git(root, &["worktree", "list", "--porcelain", "-z"], false).await?;
    let mut result: Vec<Worktree> = vec![];
    for item in output.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let item = utf8(item)?;
        if let Some(path) = item.strip_prefix("worktree ") {
            result.push(Worktree {
                id: path.into(),
                path: path.into(),
                branch: None,
                head: String::new(),
                locked: false,
                prunable: false,
                is_main: result.is_empty(),
            });
        } else if let Some(w) = result.last_mut() {
            if let Some(head) = item.strip_prefix("HEAD ") {
                w.head = head.into();
            }
            if let Some(branch) = item.strip_prefix("branch ") {
                w.branch = Some(branch.strip_prefix("refs/heads/").unwrap_or(branch).into());
            }
            w.locked |= item.starts_with("locked");
            w.prunable |= item.starts_with("prunable");
        }
    }
    Ok(result)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFile {
    pub path: String,
    pub status: String,
    pub previous_path: Option<String>,
}

pub async fn status(root: &Path) -> Result<Vec<ChangedFile>> {
    let output = git(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        false,
    )
    .await?;
    let mut records = output.split(|b| *b == 0).filter(|p| !p.is_empty());
    let mut result = vec![];
    while let Some(record) = records.next() {
        if record.len() < 4 {
            return Err(Error::Provider("invalid Git status record".into()));
        }
        let status = utf8(&record[..2])?.to_owned();
        let previous_path = if status.contains('R') || status.contains('C') {
            Some(
                utf8(
                    records
                        .next()
                        .ok_or_else(|| Error::Provider("missing rename path".into()))?,
                )?
                .into(),
            )
        } else {
            None
        };
        result.push(ChangedFile {
            path: utf8(&record[3..])?.into(),
            status,
            previous_path,
        });
    }
    Ok(result)
}

pub async fn review(
    root: &Path,
    scope: &str,
    base: &str,
    end: Option<&str>,
    file: Option<&str>,
) -> Result<serde_json::Value> {
    let mut files = status(root).await?;
    if let Some(file) = file {
        files.retain(|f| f.path == file || f.previous_path.as_deref() == Some(file));
    }
    if scope == "staged" {
        files.retain(|f| !f.status.starts_with(' ') && f.status != "??");
    }
    if scope == "unstaged" {
        files.retain(|f| {
            f.status
                .as_bytes()
                .get(1)
                .is_some_and(|c| *c != b' ' && *c != b'?')
                || f.status == "??"
        });
    }
    let mut args = vec!["diff", "--no-color", "--no-ext-diff", "--no-textconv"];
    match scope {
        "staged" => args.push("--cached"),
        "unstaged" => {}
        _ => args.push(base),
    }
    if let Some(end) = end {
        args.push(end);
    }
    args.push("--");
    if let Some(file) = file {
        args.push(file);
    }
    if scope == "base" || scope == "turn" {
        let mut names = vec!["diff", "--name-status", "-z", base];
        if let Some(end) = end {
            names.push(end);
        }
        names.push("--");
        if let Some(file) = file {
            names.push(file);
        }
        let output = git(root, &names, false).await?;
        let mut records = output.split(|b| *b == 0).filter(|p| !p.is_empty());
        files.clear();
        while let Some(kind) = records.next() {
            let Some(path) = records.next() else {
                return Err(Error::Provider("missing diff path".into()));
            };
            let kind = utf8(kind)?.to_string();
            let (path, previous_path) = if kind.starts_with('R') || kind.starts_with('C') {
                (
                    records
                        .next()
                        .ok_or_else(|| Error::Provider("missing renamed path".into()))?,
                    Some(utf8(path)?.to_string()),
                )
            } else {
                (path, None)
            };
            files.push(ChangedFile {
                path: utf8(path)?.into(),
                status: kind,
                previous_path,
            });
        }
        if end.is_none() {
            files.extend(
                status(root)
                    .await?
                    .into_iter()
                    .filter(|f| f.status == "??" && file.is_none_or(|name| f.path == name)),
            );
        }
    }
    let mut diff = git(root, &args, false).await?;
    let mut truncated = false;
    // Keep the serialized reply within the native client's buffered fetch budget.
    const MAX_DIFF: usize = 96 * 1024;
    for file in files
        .iter()
        .filter(|f| f.status == "??" && scope != "staged" && end.is_none())
    {
        if diff.len() >= MAX_DIFF {
            truncated = true;
            break;
        }
        let path = root.join(&file.path);
        let metadata = tokio::fs::symlink_metadata(&path).await?;
        if metadata.file_type().is_symlink() {
            let target = tokio::fs::read_link(&path).await?;
            diff.extend_from_slice(format!("diff --git a/{0} b/{0}\nnew file mode 120000\n--- /dev/null\n+++ b/{0}\n@@ -0,0 +1 @@\n+{1}\n", file.path, target.display()).as_bytes());
        } else if metadata.is_file() {
            let part = git(
                root,
                &[
                    "diff",
                    "--no-index",
                    "--no-color",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--",
                    "/dev/null",
                    &file.path,
                ],
                true,
            )
            .await?;
            diff.extend_from_slice(&part);
        }
    }
    if diff.len() > MAX_DIFF {
        diff.truncate(MAX_DIFF);
        truncated = true;
    }
    let diff = String::from_utf8_lossy(&diff).into_owned();
    let files_truncated = files.len() > 128;
    let files: Vec<_> = files.into_iter().take(128).collect();
    Ok(
        serde_json::json!({"files":files,"diff":diff,"truncated":truncated,"filesTruncated":files_truncated,"base":base,"scope":scope}),
    )
}
