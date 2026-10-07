mod assets;
mod command;
mod devices;
mod find;
mod preview;
mod projects;
mod terminal;
mod windows;

use async_trait::async_trait;
use mpai::{Error, ExtensionCommand, ExtensionView, Result, RuntimeExtension};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
    sync::Arc,
};
use tokio::sync::RwLock;

/// Optional local workbench commands; no filesystem or terminal service lives in the core.
pub struct WorkspaceExtension {
    projects: RwLock<BTreeMap<String, PathBuf>>,
    terminals: Arc<terminal::Terminals>,
    storage_root: PathBuf,
    github_executable: PathBuf,
    integrations: BTreeMap<String, PathBuf>,
    assets: RwLock<BTreeMap<String, PathBuf>>,
    previews: preview::Previews,
    streams: devices::Streams,
}
impl Default for WorkspaceExtension {
    fn default() -> Self {
        Self {
            projects: RwLock::default(),
            terminals: Arc::default(),
            storage_root: projects::storage_root(),
            github_executable: "gh".into(),
            integrations: BTreeMap::new(),
            assets: RwLock::default(),
            previews: preview::Previews::default(),
            streams: devices::Streams::default(),
        }
    }
}
fn required<'a>(p: &'a Value, key: &str) -> Result<&'a str> {
    p.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| Error::Invalid(format!("missing string argument {key}")))
}
fn relative(path: &str) -> Result<&Path> {
    let path = Path::new(path);
    if path
        .components()
        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(Error::Invalid(
            "use a relative workspace path without parent traversal".into(),
        ));
    }
    Ok(path)
}
fn dimension(p: &Value, key: &str, default: u16) -> Result<u16> {
    let n = p
        .get(key)
        .map(|v| {
            v.as_u64()
                .ok_or_else(|| Error::Invalid(format!("{key} must be a positive number")))
        })
        .transpose()?
        .unwrap_or(default as u64);
    if !(1..=1000).contains(&n) {
        return Err(Error::Invalid(format!("{key} must be between 1 and 1000")));
    }
    Ok(n as u16)
}
impl WorkspaceExtension {
    pub fn new() -> Self {
        Self::default()
    }
    async fn ensure_storage(&self) -> Result<()> {
        tokio::fs::create_dir_all(&self.storage_root).await?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(&self.storage_root, std::fs::Permissions::from_mode(0o700))
                .await?;
        }
        Ok(())
    }
    pub fn with_storage_root(mut self, path: impl Into<PathBuf>) -> Self {
        self.storage_root = path.into();
        self
    }
    pub fn with_integration_executable(
        mut self,
        name: impl Into<String>,
        path: impl Into<PathBuf>,
    ) -> Self {
        let name = name.into();
        let path = path.into();
        if name == "gh" {
            self.github_executable = path.clone();
        }
        self.integrations.insert(name, path);
        self
    }
    async fn project(&self, p: &Value) -> Result<PathBuf> {
        self.projects
            .read()
            .await
            .get(required(p, "projectId")?)
            .cloned()
            .ok_or_else(|| Error::Invalid("open the workspace first".into()))
    }
    async fn path(&self, root: &Path, name: &str) -> Result<PathBuf> {
        let path = tokio::fs::canonicalize(root.join(relative(name)?)).await?;
        if !path.starts_with(root) {
            return Err(Error::Invalid("path leaves the workspace".into()));
        }
        Ok(path)
    }
    async fn actions(&self, root: &Path) -> Result<Vec<Value>> {
        let mut actions = vec![];
        if root.join("Cargo.toml").is_file() {
            for (id, title, args) in [
                ("cargo.test", "Run Rust tests", vec!["test", "--workspace"]),
                (
                    "cargo.check",
                    "Check Rust workspace",
                    vec!["check", "--workspace"],
                ),
            ] {
                actions.push(json!({"id":id,"title":title,"program":"cargo","args":args}));
            }
        }
        if let Ok(bytes) = tokio::fs::read(root.join("package.json")).await
            && bytes.len() <= 1024 * 1024
            && let Ok(package) = serde_json::from_slice::<Value>(&bytes)
            && let Some(scripts) = package["scripts"].as_object()
        {
            let manager = if root.join("pnpm-lock.yaml").exists() {
                "pnpm"
            } else if root.join("yarn.lock").exists() {
                "yarn"
            } else if root.join("bun.lock").exists() || root.join("bun.lockb").exists() {
                "bun"
            } else {
                "npm"
            };
            for name in scripts.keys() {
                actions.push(json!({"id":format!("script.{name}"),"title":format!("Run {name}"),"program":manager,"args":["run",name]}));
            }
        }
        Ok(actions)
    }
}
#[async_trait]
impl RuntimeExtension for WorkspaceExtension {
    fn id(&self) -> &str {
        "workspace"
    }
    async fn shutdown(&self) {
        self.previews.shutdown().await;
    }
    fn views(&self) -> Vec<ExtensionView> {
        [
            ("files", "Files", "files", "files.list"),
            ("terminal", "Terminal", "terminal", "terminal.start"),
            ("actions", "Project actions", "actions", "actions.list"),
            ("preview", "Browser preview", "preview", "preview.start"),
            ("devices", "Devices", "devices", "devices.list"),
            ("windows", "System windows", "windows", "windows.list"),
        ]
        .into_iter()
        .map(|(id, title, kind, command)| ExtensionView {
            id: id.into(),
            title: title.into(),
            kind: kind.into(),
            command: command.into(),
        })
        .collect()
    }
    fn commands(&self) -> Vec<ExtensionCommand> {
        [
            ("project.open", "Open folder", false),
            ("conversation.directory", "Resolve chat directory", true),
            ("files.list", "List files", false),
            ("files.read", "Read file", false),
            ("files.write", "Save file", true),
            ("terminal.start", "Open terminal", true),
            ("terminal.read", "Read terminal", false),
            ("terminal.write", "Send terminal input", true),
            ("terminal.resize", "Resize terminal", false),
            ("terminal.close", "Close terminal", true),
            ("actions.list", "List project actions", false),
            ("actions.run", "Run project action", true),
            ("files.find", "Find workspace files", false),
            ("project.create", "Create project", true),
            ("project.scratch", "Create scratch workspace", true),
            ("project.clone", "Clone repository", true),
            ("project.publish", "Publish repository", true),
            ("assets.metadata", "File preview metadata", false),
            ("assets.read", "Read binary asset", false),
            ("assets.write", "Store uploaded asset", true),
            ("assets.export", "Save asset copy in workspace", true),
            ("preview.start", "Start local preview", true),
            ("preview.status", "Local preview status", false),
            ("preview.read", "Scoped static preview content", false),
            ("preview.stop", "Stop local preview", true),
            ("devices.list", "List simulators and devices", false),
            ("devices.open", "Open simulator or device", true),
            ("devices.capture", "Capture device screen", true),
            ("devices.stream", "Start live device view", true),
            ("devices.stream.status", "Device stream status", false),
            (
                "devices.stream.frame",
                "Read current live device frame",
                false,
            ),
            ("devices.stream.stop", "Stop live device view", true),
            (
                "platform.capabilities",
                "Detect platform integrations",
                false,
            ),
            ("windows.list", "List system windows", false),
            ("windows.capture", "Capture system window", true),
            (
                "windows.context",
                "Read window accessibility context",
                false,
            ),
        ]
        .into_iter()
        .map(|(name, title, mutates)| ExtensionCommand {
            name: name.into(),
            title: title.into(),
            mutates,
        })
        .collect()
    }
    async fn invoke(&self, command: &str, p: Value) -> Result<Value> {
        if [
            "project.create",
            "project.scratch",
            "project.clone",
            "project.publish",
        ]
        .contains(&command)
        {
            return projects::invoke(self, command, &p).await;
        }
        if command.starts_with("assets.") {
            return assets::invoke(self, command, &p).await;
        }
        if command.starts_with("preview.") {
            return self.previews.invoke(self, command, &p).await;
        }
        if command.starts_with("devices.") {
            return devices::invoke(self, command, &p).await;
        }
        if command == "platform.capabilities" || command.starts_with("windows.") {
            return windows::invoke(self, command, &p).await;
        }
        if command == "files.find" {
            return find::files(self, &p).await;
        }
        match command {
            "conversation.directory" => {
                let custom = p.get("path").and_then(Value::as_str);
                let path = match custom {
                    Some(path) => PathBuf::from(path),
                    None => dirs::document_dir()
                        .or_else(|| dirs::home_dir().map(|home| home.join("Documents")))
                        .ok_or_else(|| {
                            Error::Invalid("Choose a chat directory in Settings.".into())
                        })?,
                };
                if !path.is_absolute() {
                    return Err(Error::Invalid(
                        "The chat directory must be an absolute path.".into(),
                    ));
                }
                if custom.is_none() {
                    tokio::fs::create_dir_all(&path).await?;
                }
                let path = tokio::fs::canonicalize(path).await?;
                if !path.is_dir() {
                    return Err(Error::Invalid(
                        "The chat directory must be a folder.".into(),
                    ));
                }
                let id = path.to_string_lossy().to_string();
                self.projects.write().await.insert(id.clone(), path.clone());
                return Ok(json!({"id":id,"path":path}));
            }
            "project.open" => {
                let path = tokio::fs::canonicalize(required(&p, "path")?).await?;
                if !path.is_dir() {
                    return Err(Error::Invalid("workspace must be a directory".into()));
                }
                let id = path.to_string_lossy().to_string();
                self.projects.write().await.insert(id.clone(), path.clone());
                return Ok(json!({"id":id,"path":path}));
            }
            "terminal.read" => {
                return self.terminals.read(
                    required(&p, "terminalId")?,
                    p["after"].as_u64().unwrap_or(0),
                );
            }
            "terminal.write" => {
                return self.terminals.write(
                    required(&p, "terminalId")?,
                    p["data"]
                        .as_str()
                        .ok_or_else(|| Error::Invalid("data must be a string".into()))?,
                );
            }
            "terminal.resize" => {
                return self.terminals.resize(
                    required(&p, "terminalId")?,
                    dimension(&p, "cols", 80)?,
                    dimension(&p, "rows", 24)?,
                );
            }
            "terminal.close" => return self.terminals.close(required(&p, "terminalId")?),
            _ => {}
        }
        let root = self.project(&p).await?;
        match command {
            "files.list" => {
                let name = p["path"].as_str().unwrap_or("");
                let path = self.path(&root, name).await?;
                let mut directory = tokio::fs::read_dir(path).await?;
                let mut entries = vec![];
                while let Some(entry) = directory.next_entry().await? {
                    if entries.len() >= 2048 {
                        return Err(Error::Invalid("directory exceeds 2048 entries".into()));
                    }
                    let name = entry.file_name().to_string_lossy().to_string();
                    let kind = entry.file_type().await?;
                    let metadata = entry.metadata().await?;
                    entries.push(json!({"name":name,"path":entry.path().strip_prefix(&root).unwrap().to_string_lossy(),"kind":if kind.is_symlink(){"symlink"}else if kind.is_dir(){"directory"}else{"file"},"size":metadata.len()}));
                }
                entries.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
                Ok(json!({"path":name,"entries":entries}))
            }
            "files.read" => {
                let name = required(&p, "path")?;
                let path = self.path(&root, name).await?;
                if tokio::fs::metadata(&path).await?.len() > 1024 * 1024 {
                    return Err(Error::Invalid("file exceeds 1 MiB editor limit".into()));
                }
                Ok(json!({"path":name,"content":tokio::fs::read_to_string(path).await?}))
            }
            "files.write" => {
                let name = required(&p, "path")?;
                let content = p["content"]
                    .as_str()
                    .ok_or_else(|| Error::Invalid("content must be text".into()))?;
                if content.len() > 1024 * 1024 {
                    return Err(Error::Invalid("file exceeds 1 MiB editor limit".into()));
                }
                let path = root.join(relative(name)?);
                let parent = tokio::fs::canonicalize(
                    path.parent()
                        .ok_or_else(|| Error::Invalid("file needs a parent directory".into()))?,
                )
                .await?;
                if !parent.starts_with(&root) {
                    return Err(Error::Invalid("path leaves the workspace".into()));
                }
                if let Ok(metadata) = tokio::fs::symlink_metadata(&path).await
                    && metadata.file_type().is_symlink()
                {
                    return Err(Error::Invalid("editing symlinks is not supported".into()));
                }
                tokio::fs::write(path, content).await?;
                Ok(json!({"written":true}))
            }
            "terminal.start" => {
                let program = p["program"]
                    .as_str()
                    .map(String::from)
                    .unwrap_or_else(|| std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into()));
                let args = p["args"]
                    .as_array()
                    .map(|v| {
                        v.iter()
                            .map(|s| {
                                s.as_str()
                                    .map(String::from)
                                    .ok_or_else(|| Error::Invalid("args must be strings".into()))
                            })
                            .collect::<Result<Vec<_>>>()
                    })
                    .transpose()?
                    .unwrap_or_default();
                let terminals = self.terminals.clone();
                let cols = dimension(&p, "cols", 80)?;
                let rows = dimension(&p, "rows", 24)?;
                tokio::task::spawn_blocking(move || {
                    terminals.start(&root, &program, &args, cols, rows)
                })
                .await
                .map_err(|e| Error::Provider(e.to_string()))?
            }
            "actions.list" => Ok(json!({"actions":self.actions(&root).await?})),
            "actions.run" => {
                let id = required(&p, "actionId")?;
                let action = self
                    .actions(&root)
                    .await?
                    .into_iter()
                    .find(|a| a["id"] == id)
                    .ok_or_else(|| Error::Invalid("unknown project action".into()))?;
                let terminals = self.terminals.clone();
                let program = action["program"].as_str().unwrap().to_owned();
                let args = action["args"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|s| s.as_str().unwrap().to_owned())
                    .collect::<Vec<_>>();
                tokio::task::spawn_blocking(move || terminals.start(&root, &program, &args, 80, 24))
                    .await
                    .map_err(|e| Error::Provider(e.to_string()))?
            }
            _ => Err(Error::Unsupported(format!("workspace command {command}"))),
        }
    }
}
