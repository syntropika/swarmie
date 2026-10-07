mod checkpoint;
mod github;
mod operations;
mod repository;
mod restore;
mod setup;

use async_trait::async_trait;
use mpai::{
    Error, EventKind, ExtensionCommand, ExtensionView, Prompt, Result, RuntimeEvent,
    RuntimeExtension, SessionSnapshot,
};
use repository::{git, utf8, worktrees};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use tokio::sync::{Mutex, RwLock};

/// Git commands are an optional extension; the core has no Git dependency.
pub struct GitExtension {
    worktree_root: PathBuf,
    projects: RwLock<BTreeMap<String, PathBuf>>,
    mutations: Mutex<()>,
    checkpoints: checkpoint::Checkpoints,
    github_executable: PathBuf,
    setups: RwLock<BTreeMap<String, Value>>,
    live_sessions: RwLock<BTreeMap<String, PathBuf>>,
}

impl GitExtension {
    pub async fn new(worktree_root: impl AsRef<Path>) -> Result<Self> {
        tokio::fs::create_dir_all(worktree_root.as_ref()).await?;
        let worktree_root = tokio::fs::canonicalize(worktree_root).await?;
        let setups = match tokio::fs::read(worktree_root.join("setup.json")).await {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            worktree_root,
            projects: RwLock::default(),
            mutations: Mutex::new(()),
            checkpoints: checkpoint::Checkpoints::default(),
            github_executable: "gh".into(),
            setups: RwLock::new(setups),
            live_sessions: RwLock::default(),
        })
    }
    pub fn with_github_executable(mut self, path: impl Into<PathBuf>) -> Self {
        self.github_executable = path.into();
        self
    }
    async fn project(&self, p: &Value) -> Result<PathBuf> {
        let id = required(p, "projectId")?;
        self.projects
            .read()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| Error::Invalid("open the repository before using its projectId".into()))
    }
    async fn tree(&self, root: &Path, p: &Value) -> Result<repository::Worktree> {
        let id = required(p, "worktreeId")?;
        worktrees(root)
            .await?
            .into_iter()
            .find(|w| w.id == id && !w.prunable)
            .ok_or_else(|| Error::Invalid("worktree does not belong to this project".into()))
    }
    async fn open(&self, p: &Value) -> Result<Value> {
        let path = tokio::fs::canonicalize(required(p, "path")?).await?;
        let top = git(&path, &["rev-parse", "--show-toplevel"], false).await?;
        let root = tokio::fs::canonicalize(utf8(&top)?.trim_end_matches('\n')).await?;
        // Unborn repositories can be opened and committed; worktree creation still needs a base.
        let id = root
            .to_str()
            .ok_or_else(|| Error::Unsupported("non-UTF-8 repository path".into()))?
            .to_owned();
        self.projects.write().await.insert(id.clone(), root.clone());
        Ok(json!({"id":id,"path":root,"worktrees":worktrees(&root).await?}))
    }
    async fn create(&self, root: &Path, p: &Value) -> Result<Value> {
        let _lock = self.mutations.lock().await;
        let branch = required(p, "branch")?;
        if branch.starts_with('-') || branch == "HEAD" || branch.contains('@') {
            return Err(Error::Invalid("provide a new literal branch name".into()));
        }
        git(root, &["check-ref-format", "--branch", branch], false).await?;
        let base = p
            .get("base")
            .map(|v| {
                v.as_str()
                    .ok_or_else(|| Error::Invalid("base must be a string".into()))
            })
            .transpose()?
            .unwrap_or("HEAD");
        let commit = git(
            root,
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{base}^{{commit}}"),
            ],
            false,
        )
        .await?;
        let commit = utf8(&commit)?.trim();
        let path = self.worktree_root.join(uuid::Uuid::new_v4().to_string());
        let path_arg = path
            .to_str()
            .ok_or_else(|| Error::Unsupported("non-UTF-8 worktree path".into()))?;
        git(
            root,
            &["worktree", "add", "-b", branch, "--", path_arg, commit],
            false,
        )
        .await?;
        let tree = worktrees(root)
            .await?
            .into_iter()
            .find(|w| w.path == path)
            .ok_or_else(|| Error::Provider("Git created a worktree but did not list it".into()))?;
        let mut result = serde_json::to_value(tree)?;
        if p["setup"] == true {
            let setup_params = json!({"worktreeId":result["id"]});
            match setup::run(self, root, &setup_params).await {
                Ok(receipt) => result["setup"] = receipt,
                Err(error) => {
                    result["setup"] = json!({"completed":false,"message":error.to_string()})
                }
            }
        }
        Ok(result)
    }
    async fn remove(&self, root: &Path, p: &Value) -> Result<Value> {
        let _lock = self.mutations.lock().await;
        let tree = self.tree(root, p).await?;
        let path = tokio::fs::canonicalize(&tree.path).await?;
        if tree.is_main || tree.locked || path.parent() != Some(self.worktree_root.as_path()) {
            return Err(Error::Invalid(
                "only extension-managed secondary worktrees can be removed".into(),
            ));
        }
        if self
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
                "worktree contains uncommitted changes".into(),
            ));
        }
        let submodules = git(&path, &["submodule", "status", "--recursive"], false).await?;
        if utf8(&submodules)?
            .lines()
            .any(|line| !line.starts_with('-'))
        {
            // Native deinit refuses modified submodule worktrees. No force flag is permitted.
            git(&path, &["submodule", "deinit", "--all"], false).await?;
        }
        // Git requires force even after clean submodules have been safely deinitialized.
        // This narrow exception follows cleanliness/ownership/lock checks and never discards edits.
        let mut args = vec!["worktree", "remove"];
        if !submodules.is_empty() {
            args.push("--force");
        }
        args.extend(["--", required(p, "worktreeId")?]);
        git(root, &args, false).await?;
        Ok(json!({"removed":true,"branchRetained":true}))
    }
}

fn required<'a>(p: &'a Value, key: &str) -> Result<&'a str> {
    p.get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| Error::Invalid(format!("missing string argument {key}")))
}

#[async_trait]
impl RuntimeExtension for GitExtension {
    fn id(&self) -> &str {
        "git"
    }
    fn views(&self) -> Vec<ExtensionView> {
        vec![
            ExtensionView {
                id: "review".into(),
                title: "Review".into(),
                kind: "git-review".into(),
                command: "review".into(),
            },
            ExtensionView {
                id: "workflow".into(),
                title: "Git and pull requests".into(),
                kind: "git-workflow".into(),
                command: "status".into(),
            },
        ]
    }
    fn commands(&self) -> Vec<ExtensionCommand> {
        [
            ("project.open", "Open repository", false),
            ("worktree.list", "List worktrees", false),
            ("worktree.create", "Create worktree", true),
            ("worktree.remove", "Remove clean worktree", true),
            ("review", "Review changes", false),
            ("branches", "List branches", false),
            ("stage", "Stage files", true),
            ("unstage", "Unstage files", true),
            ("checkpoints", "List turn checkpoints", false),
            (
                "checkpoint.restore",
                "Restore a clean managed worktree checkpoint",
                true,
            ),
            ("status", "Repository status", false),
            ("commit", "Commit staged changes", true),
            ("push", "Push current branch", true),
            ("text.prompt", "Prepare generated Git text", false),
            ("integration.status", "Check GitHub CLI", false),
            ("pr.create", "Create draft pull request", true),
            ("pr.status", "Pull request status", false),
            ("pr.checks", "Pull request checks", false),
            ("pr.comments", "Pull request discussion", false),
            ("pr.comment", "Comment on pull request", true),
            ("pr.review", "Review pull request", true),
            ("pr.viewed", "Set viewed file status", true),
            ("pr.viewed.list", "Read viewed file status", false),
            ("worktree.setup.configure", "Configure worktree setup", true),
            ("worktree.setup.run", "Run worktree setup", true),
            ("worktree.cleanup", "Clean managed worktrees", true),
        ]
        .into_iter()
        .map(|(name, title, mutates)| ExtensionCommand {
            name: name.into(),
            title: title.into(),
            mutates,
        })
        .collect()
    }
    async fn before_prompt(&self, session: &SessionSnapshot, _prompt: &mut Prompt) -> Result<()> {
        if session.turn.is_some() {
            return Ok(());
        }
        let Ok(top) = git(
            &session.options.cwd,
            &["rev-parse", "--show-toplevel"],
            false,
        )
        .await
        else {
            return Ok(());
        };
        let path = tokio::fs::canonicalize(utf8(&top)?.trim_end_matches('\n')).await?;
        let _lock = self.mutations.lock().await;
        self.checkpoints.pending.lock().await.remove(&session.id);
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            checkpoint::capture(&path, &self.worktree_root.join("checkpoints")),
        )
        .await
        {
            Ok(Ok(base)) => {
                self.checkpoints
                    .pending
                    .lock()
                    .await
                    .insert(session.id.clone(), (path.clone(), base));
            }
            Ok(Err(error)) => eprintln!("Git checkpoint unavailable: {error}"),
            Err(_) => eprintln!("Git checkpoint unavailable: capture timed out"),
        }
        Ok(())
    }
    async fn after_spawn(&self, snapshot: &SessionSnapshot) {
        let _lock = self.mutations.lock().await;
        if let Ok(cwd) = tokio::fs::canonicalize(&snapshot.options.cwd).await {
            self.live_sessions
                .write()
                .await
                .insert(snapshot.id.clone(), cwd);
        }
    }
    async fn on_session_closed(&self, snapshot: &SessionSnapshot) {
        let _lock = self.mutations.lock().await;
        self.live_sessions.write().await.remove(&snapshot.id);
    }
    async fn on_event(&self, event: &RuntimeEvent) {
        if !matches!(
            event.kind,
            EventKind::TurnStarted | EventKind::TurnEnded { .. }
        ) {
            return;
        }
        let Some(turn) = &event.turn_id else { return };
        let _lock = self.mutations.lock().await;
        let root = self.worktree_root.join("checkpoints");
        let result: Result<()> = async {
            match &event.kind {
                EventKind::TurnStarted => {
                    if let Some((path, base)) = self
                        .checkpoints
                        .pending
                        .lock()
                        .await
                        .remove(&event.session_id)
                    {
                        let mut points = checkpoint::load(&root, &event.session_id).await?;
                        points.push(checkpoint::Checkpoint {
                            turn_id: turn.clone(),
                            base,
                            end: None,
                            path,
                        });
                        checkpoint::save(&root, &event.session_id, &points).await?;
                    }
                }
                EventKind::TurnEnded { .. } => {
                    let mut points = checkpoint::load(&root, &event.session_id).await?;
                    if let Some(point) = points.iter_mut().find(|c| &c.turn_id == turn) {
                        point.end = Some(
                            tokio::time::timeout(
                                std::time::Duration::from_secs(5),
                                checkpoint::capture(&point.path, &root),
                            )
                            .await
                            .map_err(|_| Error::Timeout("Git turn checkpoint".into()))??,
                        );
                        checkpoint::save(&root, &event.session_id, &points).await?;
                    }
                }
                _ => {}
            }
            Ok(())
        }
        .await;
        if let Err(error) = result {
            eprintln!("Git checkpoint: {error}");
        }
    }
    async fn invoke(&self, command: &str, p: Value) -> Result<Value> {
        if command == "project.open" {
            return self.open(&p).await;
        }
        if command == "checkpoints" {
            return Ok(
                json!({"checkpoints":checkpoint::load(&self.worktree_root.join("checkpoints"),required(&p,"sessionId")?).await?}),
            );
        }
        let root = self.project(&p).await?;
        if command == "integration.status" || command.starts_with("pr.") {
            let cwd = if p["worktreeId"].is_string() {
                self.tree(&root, &p).await?.path
            } else {
                root.clone()
            };
            return github::invoke(&self.github_executable, &cwd, command, &p).await;
        }
        if ["status", "commit", "push", "text.prompt"].contains(&command) {
            let _lock = self.mutations.lock().await;
            let cwd = if p["worktreeId"].is_string() {
                self.tree(&root, &p).await?.path
            } else {
                root.clone()
            };
            return operations::invoke(&cwd, command, &p).await;
        }
        match command {
            "checkpoint.restore" => {
                let _lock = self.mutations.lock().await;
                restore::restore(self, &root, &p).await
            }
            "worktree.setup.configure" => {
                let _lock = self.mutations.lock().await;
                let config = setup::validate(&p)?;
                let mut setups = self.setups.write().await;
                let mut next = setups.clone();
                next.insert(root.to_string_lossy().into(), config.clone());
                let temporary = self
                    .worktree_root
                    .join(format!(".setup-{}.json", uuid::Uuid::new_v4()));
                tokio::fs::write(&temporary, serde_json::to_vec(&next)?).await?;
                tokio::fs::rename(&temporary, self.worktree_root.join("setup.json")).await?;
                *setups = next;
                Ok(config)
            }
            "worktree.setup.run" => {
                let _lock = self.mutations.lock().await;
                setup::run(self, &root, &p).await
            }
            "worktree.cleanup" => {
                let ids = p["worktreeIds"].as_array().ok_or_else(|| {
                    Error::Invalid("worktreeIds must be an explicit array".into())
                })?;
                if ids.len() > 64 {
                    return Err(Error::Invalid("cleanup is limited to 64 worktrees".into()));
                }
                let mut results = vec![];
                for id in ids {
                    let id = id
                        .as_str()
                        .ok_or_else(|| Error::Invalid("worktreeIds must contain strings".into()))?;
                    let result = self.remove(&root, &json!({"worktreeId":id})).await;
                    results.push(match result {
                        Ok(_) => json!({"worktreeId":id,"removed":true}),
                        Err(error) => {
                            json!({"worktreeId":id,"removed":false,"message":error.to_string()})
                        }
                    });
                }
                Ok(json!({"results":results}))
            }
            "worktree.list" => Ok(serde_json::to_value(worktrees(&root).await?)?),
            "worktree.create" => self.create(&root, &p).await,
            "worktree.remove" => self.remove(&root, &p).await,
            "branches" => {
                let output = git(
                    &root,
                    &[
                        "for-each-ref",
                        "--format=%(refname:short)",
                        "refs/heads",
                        "refs/remotes",
                    ],
                    false,
                )
                .await?;
                Ok(json!({"branches":utf8(&output)?.lines().collect::<Vec<_>>() }))
            }
            "stage" | "unstage" => {
                let _lock = self.mutations.lock().await;
                let tree = self.tree(&root, &p).await?;
                let paths = p["paths"]
                    .as_array()
                    .ok_or_else(|| Error::Invalid("paths must be an array".into()))?
                    .iter()
                    .map(|path| {
                        path.as_str()
                            .ok_or_else(|| Error::Invalid("paths must contain strings".into()))
                    })
                    .collect::<Result<Vec<_>>>()?;
                if paths.is_empty()
                    || paths.iter().any(|s| {
                        s.is_empty()
                            || Path::new(s)
                                .components()
                                .any(|c| !matches!(c, std::path::Component::Normal(_)))
                    })
                {
                    return Err(Error::Invalid("provide literal relative file paths".into()));
                }
                let mut args = if command == "stage" {
                    vec!["--literal-pathspecs", "add", "--"]
                } else {
                    vec!["--literal-pathspecs", "restore", "--staged", "--"]
                };
                args.extend(paths);
                git(&tree.path, &args, false).await?;
                Ok(json!({"updated":true}))
            }
            "review" => {
                let tree = self.tree(&root, &p).await?;
                let scope = p["scope"].as_str().unwrap_or("working");
                if !["working", "staged", "unstaged", "base", "turn"].contains(&scope) {
                    return Err(Error::Invalid("unknown review scope".into()));
                }
                let file = p["file"].as_str();
                if file.is_some_and(|s| {
                    s.is_empty()
                        || Path::new(s)
                            .components()
                            .any(|c| !matches!(c, std::path::Component::Normal(_)))
                }) {
                    return Err(Error::Invalid(
                        "provide a literal relative file path".into(),
                    ));
                }
                let mut base = p["base"].as_str().unwrap_or("HEAD").to_string();
                let mut end = None;
                if scope == "turn" {
                    let checkpoints = checkpoint::load(
                        &self.worktree_root.join("checkpoints"),
                        required(&p, "sessionId")?,
                    )
                    .await?;
                    let point = checkpoints
                        .into_iter()
                        .find(|c| c.turn_id == p["turnId"].as_str().unwrap_or(""))
                        .ok_or_else(|| Error::Invalid("turn checkpoint is unavailable".into()))?;
                    if tokio::fs::canonicalize(&point.path).await?
                        != tokio::fs::canonicalize(&tree.path).await?
                    {
                        return Err(Error::Invalid(
                            "checkpoint belongs to another worktree".into(),
                        ));
                    }
                    if point.end.is_none() {
                        return Err(Error::Invalid(
                            "turn review requires a completed checkpoint".into(),
                        ));
                    }
                    base = point.base;
                    end = point.end;
                } else {
                    let revision = git(
                        &tree.path,
                        &[
                            "rev-parse",
                            "--verify",
                            "--end-of-options",
                            &format!("{base}^{{commit}}"),
                        ],
                        false,
                    )
                    .await?;
                    base = utf8(&revision)?.trim().to_owned();
                }
                repository::review(&tree.path, scope, &base, end.as_deref(), file).await
            }
            _ => Err(Error::Unsupported(format!("Git command {command}"))),
        }
    }
}
