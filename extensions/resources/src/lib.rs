pub mod bridge;
mod library;
mod settings;
mod store;
mod usage;
mod workflows;

use async_trait::async_trait;
use swarmie::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use store::*;
use tokio::sync::Mutex as AsyncMutex;

#[derive(Clone)]
pub struct ResourcesExtension(pub(crate) Arc<Inner>);
pub(crate) struct Inner {
    root: PathBuf,
    state: AsyncMutex<State>,
    loaded: AtomicBool,
    credentials: AsyncMutex<BTreeMap<String, String>>,
    runtime: Mutex<Option<WeakRuntime>>,
    stopped: AtomicBool,
    started: AtomicBool,
    scheduler: Mutex<Option<tokio::task::JoinHandle<()>>>,
    flusher: Mutex<Option<tokio::task::JoinHandle<()>>>,
    jobs: Mutex<BTreeMap<String, tokio::task::JoinHandle<()>>>,
    agents: AsyncMutex<BTreeMap<String, Agent>>,
    discovered: AsyncMutex<BTreeMap<String, Vec<PathBuf>>>,
    last_error: Mutex<Option<String>>,
    text_dirty: AtomicBool,
}
impl ResourcesExtension {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let root = private_root(path.as_ref())?;
        if root.join("resources.json").exists() {
            return Err(Error::Storage("legacy resource state exists; choose a fresh development resource root; existing files were preserved".into()));
        }
        let state = State::default();
        let credentials = load(&root.join("credentials.json"))?;
        Ok(Self(Arc::new(Inner {
            root,
            state: AsyncMutex::new(state),
            loaded: AtomicBool::new(false),
            credentials: AsyncMutex::new(credentials),
            runtime: Mutex::new(None),
            stopped: AtomicBool::new(false),
            started: AtomicBool::new(false),
            scheduler: Mutex::new(None),
            flusher: Mutex::new(None),
            jobs: Mutex::new(BTreeMap::new()),
            agents: AsyncMutex::new(BTreeMap::new()),
            discovered: AsyncMutex::new(BTreeMap::new()),
            last_error: Mutex::new(None),
            text_dirty: AtomicBool::new(false),
        })))
    }
    fn runtime(&self) -> Result<Runtime> {
        if self.0.stopped.load(Ordering::SeqCst) {
            return Err(Error::Closed);
        }
        self.0
            .runtime
            .lock()
            .unwrap()
            .as_ref()
            .and_then(WeakRuntime::upgrade)
            .ok_or(Error::Closed)
    }
    fn host_runtime(&self) -> Result<Runtime> {
        self.0
            .runtime
            .lock()
            .unwrap()
            .as_ref()
            .and_then(WeakRuntime::upgrade)
            .ok_or(Error::Closed)
    }
    async fn ensure_loaded(&self) -> Result<()> {
        if self.0.loaded.load(Ordering::SeqCst) {
            return Ok(());
        }
        let mut state = self.0.state.lock().await;
        if self.0.loaded.load(Ordering::SeqCst) {
            return Ok(());
        }
        let mut restored: State = self
            .host_runtime()?
            .extension_state("resources")
            .await?
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        for schedule in restored
            .schedules
            .values_mut()
            .filter(|s| s.status == "running")
        {
            schedule.enabled = false;
            schedule.status = "interrupted".into();
            schedule.error =
                Some("Daemon stopped during delivery; inspect before enabling again".into());
        }
        for job in restored
            .jobs
            .values_mut()
            .filter(|j| j.status == "running" || j.status == "cancelling")
        {
            job.status = "interrupted".into();
            for target in job
                .targets
                .iter_mut()
                .filter(|t| matches!(t.status.as_str(), "running" | "needs_input" | "pending"))
            {
                target.status = "interrupted".into();
            }
        }

        self.host_runtime()?
            .save_extension_state("resources", serde_json::to_value(&restored)?)
            .await?;
        *state = restored;
        self.0.loaded.store(true, Ordering::SeqCst);
        Ok(())
    }
    async fn persist(&self, state: &State) -> Result<()> {
        self.host_runtime()?
            .save_extension_state("resources", serde_json::to_value(state)?)
            .await?;
        self.0.text_dirty.store(false, Ordering::SeqCst);
        Ok(())
    }
    fn record_error(&self, error: Error) {
        *self.0.last_error.lock().unwrap() = Some(error.to_string());
    }
    pub fn tool_definitions() -> Vec<Value> {
        bridge::tools()
    }
}
const COMMANDS: &[(&str, &str, bool)] = &[
    ("credentials.status", "Credential status", false),
    ("credentials.set", "Set credential", true),
    ("credentials.remove", "Remove credential", true),
    ("project.get", "Project defaults", false),
    ("project.set", "Save project defaults", true),
    ("session.create", "Create configured session", true),
    ("session.resume", "Resume configured session", true),
    (
        "text.generate",
        "Generate text with native harness configuration",
        true,
    ),
    ("account.quotas", "Native account quotas", false),
    ("templates.list", "Prompt templates", false),
    ("templates.put", "Save template", true),
    ("templates.remove", "Remove template", true),
    ("templates.render", "Render template", false),
    ("resources.list", "Native resources", false),
    ("resources.read", "Read resource", false),
    ("history.list", "Prompt history", false),
    ("schedules.list", "Scheduled prompts", false),
    ("schedules.put", "Save schedule", true),
    ("schedules.remove", "Remove schedule", true),
    ("schedules.run", "Run schedule", true),
    ("peers.list", "Peer sessions", false),
    ("peers.spawn", "Spawn peer", true),
    ("peers.read", "Read peer", false),
    ("peers.send", "Send peer prompt", true),
    ("fanout.start", "Start model comparison", true),
    ("fanout.status", "Model comparison", false),
    ("fanout.list", "Saved model comparisons", false),
    ("fanout.remove", "Remove saved comparison", true),
    ("fanout.cancel", "Cancel comparison", true),
    ("usage.dashboard", "Historical usage", false),
    ("tools.list", "Agent tool definitions", false),
];
#[async_trait]
impl RuntimeExtension for ResourcesExtension {
    fn id(&self) -> &str {
        "resources"
    }
    fn initialize(&self, runtime: WeakRuntime) {
        *self.0.runtime.lock().unwrap() = Some(runtime);
        self.start_scheduler();
    }
    async fn shutdown(&self) {
        if let Err(error) = self.ensure_loaded().await {
            self.record_error(error);
            return;
        }
        self.0.stopped.store(true, Ordering::SeqCst);
        if let Some(task) = self.0.scheduler.lock().unwrap().take() {
            task.abort();
        }
        if let Some(task) = self.0.flusher.lock().unwrap().take() {
            task.abort();
        }
        for (_, task) in std::mem::take(&mut *self.0.jobs.lock().unwrap()) {
            task.abort();
        }
        let mut state = self.0.state.lock().await;
        for job in state
            .jobs
            .values_mut()
            .filter(|j| j.status == "running" || j.status == "cancelling")
        {
            job.status = "interrupted".into();
            for target in &mut job.targets {
                if matches!(
                    target.status.as_str(),
                    "running" | "needs_input" | "pending"
                ) {
                    target.status = "interrupted".into();
                }
            }
        }
        for schedule in state
            .schedules
            .values_mut()
            .filter(|s| s.status == "running")
        {
            schedule.enabled = false;
            schedule.status = "interrupted".into();
        }
        if let Err(e) = self.persist(&state).await {
            self.record_error(e);
        }
    }
    fn commands(&self) -> Vec<ExtensionCommand> {
        COMMANDS
            .iter()
            .map(|(name, title, mutates)| ExtensionCommand {
                name: (*name).into(),
                title: (*title).into(),
                mutates: *mutates,
            })
            .collect()
    }
    fn views(&self) -> Vec<ExtensionView> {
        [
            ("resources", "Resources", "resources.list"),
            ("settings", "Project settings", "project.get"),
            ("schedules", "Scheduled prompts", "schedules.list"),
            ("usage", "Usage", "usage.dashboard"),
            ("peers", "Peers and model comparison", "peers.list"),
        ]
        .into_iter()
        .map(|(id, title, command)| ExtensionView {
            id: id.into(),
            title: title.into(),
            kind: id.into(),
            command: command.into(),
        })
        .collect()
    }
    async fn invoke(&self, command: &str, params: Value) -> Result<Value> {
        self.ensure_loaded().await?;
        if !params.is_object() {
            return Err(Error::Invalid(
                "extension arguments must be an object".into(),
            ));
        }
        match command {
            "credentials.status" | "credentials.set" | "credentials.remove" | "project.get"
            | "project.set" => self.settings_command(command, &params).await,
            "templates.list" | "templates.put" | "templates.remove" | "templates.render"
            | "resources.list" | "resources.read" | "history.list" => {
                self.library_command(command, &params).await
            }
            "session.create" | "session.resume" | "account.quotas" | "text.generate"
            | "schedules.list" | "schedules.put" | "schedules.remove" | "schedules.run"
            | "peers.list" | "peers.spawn" | "peers.read" | "peers.send" | "fanout.start"
            | "fanout.status" | "fanout.cancel" | "fanout.list" | "fanout.remove" => {
                self.workflow_command(command, &params).await
            }
            "usage.dashboard" => self.dashboard(&params).await,
            "tools.list" => Ok(json!({"tools":Self::tool_definitions()})),
            _ => Err(Error::Unsupported(format!("resource command {command}"))),
        }
    }
    async fn after_spawn(&self, snapshot: &SessionSnapshot) {
        if let Err(error) = self.ensure_loaded().await {
            self.record_error(error);
            return;
        }
        let mut state = self.0.state.lock().await;
        let previous = state.sessions.get(&snapshot.id);
        let inherited_sequence = previous
            .and_then(|record| record.inherited_sequence)
            .or_else(|| {
                snapshot
                    .metadata
                    .fork_origin
                    .as_ref()
                    .map(|_| snapshot.sequence)
            });
        state.sessions.insert(
            snapshot.id.clone(),
            SessionRecord {
                id: snapshot.id.clone(),
                harness: snapshot.options.harness.clone(),
                cwd: snapshot.options.cwd.clone(),
                model: snapshot.options.model.clone(),
                native_id: snapshot.native.id.clone(),
                forked: snapshot.metadata.fork_origin.is_some(),
                inherited_sequence,
            },
        );
        if let Err(e) = self.persist(&state).await {
            self.record_error(e);
        }
    }
    async fn on_event(&self, event: &RuntimeEvent) {
        if let Err(error) = self.ensure_loaded().await {
            self.record_error(error);
            return;
        }
        let mut state = self.0.state.lock().await;
        if self.observe(&mut state, event)
            && let Err(e) = self.persist(&state).await
        {
            self.record_error(e);
        }
    }
    async fn on_session_closed(&self, snapshot: &SessionSnapshot) {
        self.0.agents.lock().await.remove(&snapshot.id);
    }
}
