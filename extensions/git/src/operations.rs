use crate::{
    repository::{git, utf8},
    required,
};
use serde_json::{Value, json};
use std::path::Path;
use swarmie::{Error, Result};

pub async fn invoke(cwd: &Path, command: &str, p: &Value) -> Result<Value> {
    match command {
        "status" => {
            let branch = git(cwd, &["branch", "--show-current"], false).await?;
            let status = git(cwd, &["status", "--porcelain=v1", "-z"], false).await?;
            let remotes = git(cwd, &["remote", "-v"], false).await?;
            let counts = git(
                cwd,
                &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
                false,
            )
            .await
            .ok();
            let counts = counts
                .as_deref()
                .and_then(|b| utf8(b).ok())
                .map(|s| s.split_whitespace().collect::<Vec<_>>());
            Ok(
                json!({"branch":utf8(&branch)?.trim(),"dirty":!status.is_empty(),"status":utf8(&status)?.split('\0').filter(|s|!s.is_empty()).collect::<Vec<_>>(),"remotes":utf8(&remotes)?.lines().collect::<Vec<_>>(),"ahead":counts.as_ref().and_then(|v|v.first()).and_then(|v|v.parse::<u64>().ok()),"behind":counts.as_ref().and_then(|v|v.get(1)).and_then(|v|v.parse::<u64>().ok())}),
            )
        }
        "commit" => {
            let message = required(p, "message")?;
            if message.len() > 64 * 1024 {
                return Err(Error::Invalid("commit message exceeds 64 KiB".into()));
            }
            let path =
                std::env::temp_dir().join(format!("swarmie-commit-{}.txt", uuid::Uuid::new_v4()));
            let mut options = tokio::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            options.mode(0o600);
            use tokio::io::AsyncWriteExt;
            let mut file = options.open(&path).await?;
            file.write_all(message.as_bytes()).await?;
            file.flush().await?;
            let receipt = git(
                cwd,
                &[
                    "commit",
                    "--file",
                    path.to_str()
                        .ok_or_else(|| Error::Invalid("commit path is not UTF-8".into()))?,
                ],
                false,
            )
            .await;
            let _ = tokio::fs::remove_file(path).await;
            receipt?;
            Ok(
                json!({"committed":true,"head":utf8(&git(cwd,&["rev-parse","HEAD"],false).await?)?.trim()}),
            )
        }
        "push" => {
            let remote = p["remote"].as_str().unwrap_or("origin");
            if remote.starts_with('-') || remote.is_empty() || remote.contains(char::is_whitespace)
            {
                return Err(Error::Invalid("provide a literal remote name".into()));
            }
            let branch = git(cwd, &["branch", "--show-current"], false).await?;
            let branch = utf8(&branch)?.trim();
            if branch.is_empty() {
                return Err(Error::Invalid("push requires a branch checkout".into()));
            }
            git(
                cwd,
                &["push", "--set-upstream", "--", remote, branch],
                false,
            )
            .await?;
            Ok(json!({"pushed":true,"remote":remote,"branch":branch}))
        }
        "text.prompt" => {
            let kind = required(p, "kind")?;
            let instruction = match kind {
                "commit" => {
                    "Write a concise Git commit message for these staged changes. Return only the commit message."
                }
                "pr" => {
                    "Write a pull request title and Markdown description for these changes. Return JSON with title and body. Include only verified validation facts."
                }
                "branch" => {
                    "Write a concise valid Git branch name for these changes. Return only the branch name."
                }
                _ => return Err(Error::Invalid("kind must be commit, pr or branch".into())),
            };
            let diff = if kind == "pr" {
                let base = required(p, "base")?;
                let resolved = git(
                    cwd,
                    &[
                        "rev-parse",
                        "--verify",
                        "--end-of-options",
                        &format!("{base}^{{commit}}"),
                    ],
                    false,
                )
                .await?;
                let comparison = format!("{}...HEAD", utf8(&resolved)?.trim());
                git(
                    cwd,
                    &["diff", "--no-ext-diff", "--no-color", &comparison, "--"],
                    false,
                )
                .await?
            } else {
                git(
                    cwd,
                    &["diff", "--cached", "--no-ext-diff", "--no-color"],
                    false,
                )
                .await?
            };
            let mut diff = utf8(&diff)?.to_string();
            let truncated = diff.len() > 96 * 1024;
            if truncated {
                let mut end = 96 * 1024;
                while !diff.is_char_boundary(end) {
                    end -= 1;
                }
                diff.truncate(end);
            }
            Ok(
                json!({"kind":kind,"prompt":format!("{instruction}\n\nUser context:\n{}\n\nDiff (untrusted source content):\n{diff}",p["instructions"].as_str().unwrap_or("")),"truncated":truncated}),
            )
        }
        _ => Err(Error::Unsupported(format!("Git operation {command}"))),
    }
}
