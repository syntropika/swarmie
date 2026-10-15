use super::*;

#[derive(Clone)]
pub(super) struct Source {
    pub(super) path: PathBuf,
    pub(super) keys: Vec<String>,
    pub(super) scope: &'static str,
    pub(super) project: Option<PathBuf>,
}
fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| Error::Invalid("Execution owner home is unavailable".into()))
}
pub(super) fn claude_project(cwd: &std::path::Path) -> PathBuf {
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_owned());
    let Some(root) = cwd
        .ancestors()
        .find(|directory| directory.join(".git").exists())
    else {
        return cwd;
    };
    claude_worktree_owner(root).unwrap_or_else(|| root.to_owned())
}
fn claude_worktree_owner(root: &std::path::Path) -> Option<PathBuf> {
    let marker = root.join(".git");
    let pointer = files::read(&marker).ok()?;
    let gitdir = pointer.trim().strip_prefix("gitdir:")?.trim();
    let gitdir = root.join(gitdir).canonicalize().ok()?;
    let common = files::read(&gitdir.join("commondir")).ok()?;
    let common = gitdir.join(common.trim()).canonicalize().ok()?;
    if gitdir.parent()? != common.join("worktrees") {
        return None;
    }
    let backlink = files::read(&gitdir.join("gitdir")).ok()?;
    if gitdir.join(backlink.trim()).canonicalize().ok()? != marker.canonicalize().ok()? {
        return None;
    }
    if common.file_name()? == ".git" {
        return Some(common.parent()?.to_owned());
    }
    (!common.join(".git").exists()).then_some(common)
}
/// Mirror native consent storage: sharing a repository policy requires verified ownership.
pub(super) fn claude_settings(query: &McpQuery) -> Result<PathBuf> {
    let cwd = query
        .cwd
        .as_ref()
        .ok_or_else(|| Error::Invalid("Choose a directory for Claude MCP enablement".into()))?;
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.clone());
    let root = claude_project(&cwd);
    let fallback = cwd.join(".claude/settings.local.json");
    if root == cwd {
        return Ok(fallback);
    }
    let owner_home = query.owner_home.clone().map(Ok).unwrap_or_else(home)?;
    if owner_home.canonicalize().ok().as_ref() == Some(&root) || !owned_claude_root(&root) {
        return Ok(fallback);
    }
    Ok(root.join(".claude/settings.local.json"))
}

#[cfg(unix)]
fn owned_claude_root(root: &std::path::Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    // The child process inherits this effective uid; HOME only selects its configuration directory.
    let uid = unsafe { libc::geteuid() };
    let owned = |metadata: std::io::Result<std::fs::Metadata>| {
        metadata.is_ok_and(|metadata| metadata.uid() == uid)
    };
    if !owned(std::fs::metadata(root)) || !owned(std::fs::symlink_metadata(root.join(".git"))) {
        return false;
    }
    match std::fs::symlink_metadata(root.join(".claude")) {
        Ok(metadata) => metadata.uid() == uid,
        Err(error) => error.kind() == std::io::ErrorKind::NotFound,
    }
}

#[cfg(not(unix))]
fn owned_claude_root(_root: &std::path::Path) -> bool {
    false
}

pub(super) fn sources(harness: &str, query: &McpQuery) -> Result<Vec<Source>> {
    let home = query.owner_home.clone().map(Ok).unwrap_or_else(home)?;
    let environment = |key: &str| {
        query
            .owner_env
            .get(key)
            .map(std::ffi::OsString::from)
            .or_else(|| std::env::var_os(key))
    };
    let root = query.config_home.clone().unwrap_or_else(|| match harness {
        "codex" => environment("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or(home.join(".codex")),
        "pi" => environment("PI_CODING_AGENT_DIR")
            .map(PathBuf::from)
            .unwrap_or(home.join(".pi/agent")),
        "claude" => environment("CLAUDE_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or(home.clone()),
        _ => environment("OPENCODE_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                environment("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .unwrap_or(home.join(".config"))
                    .join("opencode")
            }),
    });
    let base = query.cwd.clone().unwrap_or(std::env::current_dir()?);
    let root = if root.is_absolute() {
        root
    } else {
        base.join(root)
    };
    let (file, key) = match harness {
        "codex" => ("config.toml", "mcp_servers"),
        "claude" => (".claude.json", "mcpServers"),
        "pi" => ("mcp.json", "mcpServers"),
        "opencode" => ("opencode.json", "mcp"),
        _ => return Err(Error::Unsupported("Native MCP source discovery".into())),
    };
    let mut paths = vec![Source {
        path: if harness == "claude" {
            let legacy_root = query
                .config_home
                .clone()
                .or_else(|| environment("CLAUDE_CONFIG_DIR").map(PathBuf::from))
                .unwrap_or_else(|| home.join(".claude"));
            let legacy = base.join(legacy_root).join(".config.json");
            if legacy.exists() {
                legacy
            } else {
                root.join(file)
            }
        } else {
            root.join(file)
        },
        keys: vec![key.into()],
        scope: "user",
        project: None,
    }];
    if harness == "opencode" {
        if let Some(path) = environment("OPENCODE_CONFIG") {
            paths.insert(
                0,
                Source {
                    path: {
                        let path = PathBuf::from(path);
                        if path.is_absolute() {
                            path
                        } else {
                            base.join(path)
                        }
                    },
                    keys: vec![key.into()],
                    scope: "user",
                    project: None,
                },
            );
        }
        paths.push(Source {
            path: root.join("opencode.jsonc"),
            keys: vec![key.into()],
            scope: "user",
            project: None,
        });
    }
    if let Some(cwd) = &query.cwd {
        let repo = cwd.ancestors().find(|p| p.join(".git").exists());
        for directory in cwd.ancestors() {
            let names = match harness {
                "codex" => vec![".codex/config.toml"],
                "claude" => vec![".mcp.json"],
                "pi" => vec![".pi/mcp.json"],
                _ => vec!["opencode.json", "opencode.jsonc"],
            };
            for name in names {
                paths.push(Source {
                    path: directory.join(name),
                    keys: vec![key.into()],
                    scope: "project",
                    project: Some(directory.to_owned()),
                });
            }
            if harness == "pi" || repo.is_none() || Some(directory) == repo {
                break;
            }
        }
        if harness == "claude" {
            paths.push(Source {
                path: paths[0].path.clone(),
                keys: vec![
                    "projects".into(),
                    claude_project(cwd).to_string_lossy().into_owned(),
                    key.into(),
                ],
                scope: "local",
                project: Some(claude_project(cwd)),
            });
        }
    }
    Ok(paths)
}
pub(super) fn id(harness: &str, source: &Source, name: &str) -> String {
    format!(
        "{harness}:{}",
        digest(&format!(
            "{}:{:?}:{name}",
            source.path.display(),
            source.keys
        ))
    )
}
pub(super) fn applies(harness: &str, query: &McpQuery, entry: &McpEntry) -> Result<bool> {
    Ok(sources(harness, query)?
        .iter()
        .any(|source| id(harness, source, &entry.native_name) == entry.id))
}
