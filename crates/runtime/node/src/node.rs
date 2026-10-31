use futures::task::{LocalFutureObj, LocalSpawn, SpawnError};
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    thread,
};
use swarmie_runtime::{NodeId, rpc};
use tembra::Host;

pub const PRINCIPAL: &str = "desktop-user";
pub const MAX_FRAME_BYTES: usize = 64 * 1024;
const MAX_FRAME: usize = MAX_FRAME_BYTES;
const MAX_OUTPUT_FRAMES: usize = 256;
const MAX_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const MAX_POLL_FRAMES: usize = 128;
const MAX_POLL_BYTES: usize = 96 * 1024;

enum Command {
    Open {
        shared: bool,
        private: bool,
        response: mpsc::SyncSender<Result<u64, String>>,
    },
    Frame(u64, bool, zeroize::Zeroizing<Vec<u8>>),
    #[cfg(test)]
    Pause(mpsc::SyncSender<()>, mpsc::Receiver<()>),
    Stop,
}

struct Transport {
    input: mpsc::Receiver<Command>,
    output: Arc<Mutex<BTreeMap<u64, Output>>>,
}

fn require_channel(
    output: &Mutex<BTreeMap<u64, Output>>,
    generation: u64,
    private: bool,
) -> Result<(), String> {
    let output = output.lock().map_err(|_| "native output is unavailable")?;
    if output
        .get(&generation)
        .is_some_and(|channel| channel.private != private)
    {
        return Err("native channel ended".into());
    }
    Ok(())
}

fn poll_output(
    output: &Mutex<BTreeMap<u64, Output>>,
    generation: u64,
    private: bool,
) -> Result<Vec<Vec<u8>>, String> {
    output
        .lock()
        .map_err(|_| "native output is unavailable")?
        .get_mut(&generation)
        .filter(|channel| channel.private == private)
        .map(Output::drain)
        .ok_or_else(|| "native channel ended".into())
}

fn close_output(output: &Mutex<BTreeMap<u64, Output>>, generation: u64, private: bool) {
    if let Ok(mut output) = output.lock()
        && output
            .get(&generation)
            .is_some_and(|channel| channel.private == private)
    {
        output.remove(&generation);
    }
}

#[derive(Clone, Copy)]
struct Executor;
impl LocalSpawn for Executor {
    fn spawn_local_obj(&self, future: LocalFutureObj<'static, ()>) -> Result<(), SpawnError> {
        tokio::task::spawn_local(future);
        Ok(())
    }
}

/// Storage and directory policy supplied by the native host, never by an RPC caller.
/// Both profiles retain their exclusive owner locks. Directory browsing grants no
/// Git mutation authority; the embedding must authorize each selected directory.
pub struct HostConfiguration {
    pub profile: PathBuf,
    pub protected_profile: PathBuf,
    pub directory_root: Option<PathBuf>,
}

/// An embedding-owned worker. Capability calls use Tembra's existing byte protocol.
pub struct Node {
    sender: mpsc::SyncSender<Command>,
    output: Arc<Mutex<BTreeMap<u64, Output>>>,
    worker: Option<thread::JoinHandle<()>>,
    id: String,
    _lock: std::fs::File,
    git_grants: Option<Arc<crate::git::DirectoryGrants>>,
}

impl Node {
    /// Open an application-owned profile with the default local directory source.
    /// Protected execution is installed before admitted work can recover.
    pub fn open(state: PathBuf) -> Result<Self, String> {
        let root = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
        Self::with_roots(state, root.map(PathBuf::from), None)
    }

    #[cfg(test)]
    fn with_directory_root(
        state: PathBuf,
        directory_root: Option<PathBuf>,
    ) -> Result<Self, String> {
        Self::with_roots(state, directory_root, None)
    }

    /// Open the complete node composition with embedding-selected storage and I/O roots.
    /// The profile lock is held until the host and its physical work have closed.
    pub fn with_configuration(configuration: HostConfiguration) -> Result<Self, String> {
        Self::with_roots(
            configuration.profile,
            configuration.directory_root,
            Some(configuration.protected_profile),
        )
    }

    fn with_roots(
        state: PathBuf,
        directory_root: Option<PathBuf>,
        vault_root: Option<PathBuf>,
    ) -> Result<Self, String> {
        let vault_root = vault_root.unwrap_or_else(|| state.join("vault"));
        std::fs::create_dir_all(&state).map_err(|_| "native state directory is unavailable")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| "native state permissions could not be set")?;
        }
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(state.join("owner.lock"))
            .map_err(|_| "native state lock is unavailable")?;
        lock.try_lock()
            .map_err(|_| "native state is owned by another process")?;
        let identity = state.join("node-id");
        let id = match std::fs::read_to_string(&identity) {
            Ok(id) => id,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                use std::io::Write;
                let id = format!("desktop-{}", uuid::Uuid::new_v4());
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&identity)
                    .map_err(|_| "native identity could not be created")?;
                file.write_all(id.as_bytes())
                    .and_then(|()| file.sync_all())
                    .map_err(|_| "native identity could not be saved")?;
                id
            }
            Err(_) => return Err("native identity could not be read".into()),
        };
        NodeId::new(id.clone()).map_err(|_| "native identity is invalid")?;
        let oauth_host = id
            .strip_prefix("desktop-")
            .and_then(|value| uuid::Uuid::parse_str(value).ok())
            .filter(|value| !value.is_nil())
            .ok_or("native OAuth host identity is invalid")?;
        let git_grants =
            cfg!(any(unix, windows)).then(|| Arc::new(crate::git::DirectoryGrants::default()));
        let git = git_grants
            .as_ref()
            .map(|grants| {
                crate::git::service(&state, grants.clone())
                    .map(|service| (service, grants.clone() as Arc<dyn swarmie_git::Authority>))
            })
            .transpose()?;
        let (sender, receiver) = mpsc::sync_channel(64);
        let output = Arc::new(Mutex::new(BTreeMap::new()));
        let worker_output = output.clone();
        let (ready, started) = mpsc::sync_channel(1);
        let (git, agent_git_authority) = match git {
            Some((service, authority)) => (Some(service), Some(authority)),
            None => (None, None),
        };
        let configuration = swarmie_client_node::NodeConfiguration {
            project_context: Some(swarmie_client_node::ProjectContextConfiguration::default()),
            state,
            node: NodeId::new(id.clone()).map_err(|_| "native identity is invalid")?,
            oauth_host,
            principal: PRINCIPAL.into(),
            directory_root,
            git,
            agent_git_authority,
        };
        let worker = thread::Builder::new()
            .name("swarmie-node".into())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(_) => {
                        let _ = ready.send(Err("native executor could not start".to_owned()));
                        return;
                    }
                };
                runtime.block_on(tokio::task::LocalSet::new().run_until(serve(
                    Transport {
                        input: receiver,
                        output: worker_output,
                    },
                    ready,
                    configuration,
                    vault_root,
                )));
            })
            .map_err(|_| "native runtime could not start")?;
        started
            .recv()
            .map_err(|_| "native runtime did not initialize")??;
        Ok(Self {
            sender,
            output,
            worker: Some(worker),
            id,
            _lock: lock,
            git_grants,
        })
    }

    /// Open private transport only for a trusted native adapter that authenticates
    /// its operator surface. This method does not approve a pending private decision.
    /// Never forward this capability through an ordinary client RPC channel.
    pub fn private_port(&self) -> PrivatePort {
        PrivatePort {
            sender: self.sender.clone(),
            output: self.output.clone(),
        }
    }

    pub fn descriptor(&self) -> serde_json::Value {
        serde_json::json!({"id": self.id, "principal": PRINCIPAL})
    }

    /// Authorize the selected directory after an explicit embedding-owned user action.
    /// Callers must not forward an arbitrary path from an untrusted browser request.
    pub fn grant_directory(&self, path: &std::path::Path) -> std::io::Result<()> {
        self.git_grants
            .as_ref()
            .ok_or_else(|| {
                std::io::Error::other("native Git execution is not qualified on this platform")
            })?
            .grant(path)
    }

    pub fn open_channel(&self) -> Result<u64, String> {
        self.channel(false)
    }

    /// Open an independent channel without replacing other embedding-owned clients.
    pub fn open_shared_channel(&self) -> Result<u64, String> {
        self.channel(true)
    }

    fn channel(&self, shared: bool) -> Result<u64, String> {
        let (send, receive) = mpsc::sync_channel(1);
        self.sender
            .try_send(Command::Open {
                shared,
                private: false,
                response: send,
            })
            .map_err(|_| "native input budget exhausted")?;
        receive
            .recv_timeout(std::time::Duration::from_secs(2))
            .map_err(|_| "native channel did not open".to_owned())?
    }

    pub fn frame(&self, generation: u64, frame: Vec<u8>) -> Result<(), String> {
        require_channel(&self.output, generation, false)?;
        if frame.len() > MAX_FRAME {
            return Err("native frame limit exceeded".into());
        }
        self.sender
            .try_send(Command::Frame(
                generation,
                false,
                zeroize::Zeroizing::new(frame),
            ))
            .map_err(|_| "native input budget exhausted".into())
    }

    /// Drain a bounded batch without scheduling work on the capability executor.
    /// An empty batch leaves the channel open while its RPC calls are still pending.
    /// Polling does not acknowledge input delivery or RPC completion.
    pub fn poll(&self, generation: u64) -> Result<Vec<Vec<u8>>, String> {
        poll_output(&self.output, generation, false)
    }

    /// Close output now. The worker cancels the RPC connection when it next runs.
    pub fn close_channel(&self, generation: u64) {
        close_output(&self.output, generation, false);
    }

    /// Shut down this host and await its physical Tembra cleanup.
    /// Client detach does not call this method. A process that owns this node must
    /// keep its executor alive until shutdown returns.
    pub fn shutdown(mut self) -> Result<(), String> {
        self.stop_worker()
    }

    fn stop_worker(&mut self) -> Result<(), String> {
        if let Some(worker) = self.worker.take() {
            let _ = self.sender.send(Command::Stop);
            worker
                .join()
                .map_err(|_| "node worker failed during shutdown".to_owned())?;
        }
        Ok(())
    }
}

/// Private endpoint on the same worker and Tembra host. It carries no independent engine.
#[derive(Clone)]
pub struct PrivatePort {
    sender: mpsc::SyncSender<Command>,
    output: Arc<Mutex<BTreeMap<u64, Output>>>,
}
impl PrivatePort {
    pub fn open(&self) -> Result<u64, String> {
        let (send, receive) = mpsc::sync_channel(1);
        self.sender
            .try_send(Command::Open {
                shared: false,
                private: true,
                response: send,
            })
            .map_err(|_| "native input budget exhausted")?;
        receive
            .recv_timeout(std::time::Duration::from_secs(2))
            .map_err(|_| "private channel did not open")?
    }
    pub fn send(&self, generation: u64, frame: &[u8]) -> Result<(), String> {
        require_channel(&self.output, generation, true)?;
        if frame.len() > 400_000 {
            return Err("private frame limit exceeded".into());
        }
        self.sender
            .try_send(Command::Frame(
                generation,
                true,
                zeroize::Zeroizing::new(frame.to_vec()),
            ))
            .map_err(|_| "private input budget exhausted".into())
    }
    pub fn poll(&self, generation: u64) -> Result<Vec<Vec<u8>>, String> {
        poll_output(&self.output, generation, true)
    }
    pub fn close(&self, generation: u64) {
        close_output(&self.output, generation, true);
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        let _ = self.stop_worker();
        // Explicit release also closes a short fork/exec inheritance window in other workers.
        let _ = self._lock.unlock();
    }
}

async fn serve(
    transport: Transport,
    ready: mpsc::SyncSender<Result<(), String>>,
    configuration: swarmie_client_node::NodeConfiguration,
    vault_root: PathBuf,
) {
    let Transport {
        input: receiver,
        output,
    } = transport;
    let native = match crate::NativeNode::open(vault_root) {
        Ok(native) => native,
        Err(error) => {
            let _ = ready.send(Err(error.to_string()));
            return;
        }
    };
    let storage = swarmie_storage::sqlite::factory(configuration.state.join("runtime.sqlite"));
    let node_id = configuration.node.clone();
    let native_plugin_id =
        swarmie_plugin_manager::PluginId::new("swarmie.native").expect("fixed native plugin ID");
    let node_plugin_id = swarmie_plugin_manager::PluginId::new(swarmie_runtime::NODE_SERVICE)
        .expect("fixed node plugin ID");
    let owner = native.clone();
    let services = swarmie_plugin_manager::CorePlugin::new(
        native_plugin_id.clone(),
        std::num::NonZeroU32::new(1).expect("nonzero native plugin version"),
        move |context| {
            let execution = crate::control::client::ClientExecution::new(
                owner.clone(),
                PRINCIPAL.into(),
                context.index,
            );
            let mut definitions = vec![owner.private_ui(), execution.tools()];
            definitions.extend(swarmie_client_node::orbs(
                &context.host,
                configuration.clone(),
                Some(execution),
            )?);
            Ok(definitions)
        },
    )
    .requires([node_plugin_id]);
    let catalog =
        swarmie_plugin_manager::Catalog::new([swarmie_runtime::node_plugin(node_id), services])
            .and_then(|catalog| catalog.require([native_plugin_id]));
    let runtime = match catalog {
        Ok(catalog) => swarmie_runtime::Runtime::open(Executor, storage, catalog).await,
        Err(error) => Err(tembra::Error::invalid("node.plugins", error.to_string())),
    };
    let runtime = match runtime {
        Ok(runtime) => runtime,
        Err(error) => {
            native.close();
            let _ = ready.send(Err(error.to_string()));
            return;
        }
    };
    let host = runtime.host();
    if let Err(error) = swarmie_client_node::start(host).await {
        let _ = runtime.close().await;
        native.close();
        let _ = ready.send(Err(error.to_string()));
        return;
    }
    if ready.send(Ok(())).is_err() {
        let _ = runtime.close().await;
        native.close();
        return;
    }
    let mut generation: u64 = 0;
    let mut channels = BTreeMap::<u64, Channel>::new();
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(2));
    'running: loop {
        tick.tick().await;
        channels.retain(|_, channel| channel.is_open());
        for _ in 0..64 {
            let command = match receiver.try_recv() {
                Ok(command) => command,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break 'running,
            };
            match command {
                Command::Open {
                    shared,
                    private,
                    response,
                } => {
                    if !shared {
                        channels.retain(|_, channel| {
                            if channel.private == private {
                                channel.connection.close();
                                false
                            } else {
                                true
                            }
                        });
                    }
                    channels.retain(|_, channel| channel.is_open());
                    if channels
                        .values()
                        .filter(|channel| channel.private == private)
                        .count()
                        >= if private { 1 } else { 8 }
                    {
                        let _ = response.send(Err("native client limit reached".into()));
                        continue;
                    }
                    generation += 1;
                    match Channel::new(host, generation, private, output.clone()) {
                        Ok(channel) => {
                            channels.insert(generation, channel);
                            if response.send(Ok(generation)).is_err() {
                                channels.remove(&generation);
                            }
                        }
                        Err(error) => {
                            let _ = response.send(Err(error));
                        }
                    }
                }
                Command::Frame(version, private, frame) => {
                    if let Some(channel) = channels.get(&version) {
                        if channel.private == private && channel.is_open() {
                            let received = channel.connection.receive(&frame);
                            if channel.private && received.is_err() {
                                channel.connection.close();
                            }
                        }
                        if !channel.is_open() {
                            channels.remove(&version);
                        }
                    }
                }
                #[cfg(test)]
                Command::Pause(entered, release) => {
                    let _ = entered.send(());
                    let _ = release.recv();
                }
                Command::Stop => break 'running,
            }
        }
    }
    channels.clear();
    let _ = runtime.close().await;
    native.close();
}

struct Channel {
    private: bool,
    connection: rpc::Connection,
    generation: u64,
    output: Arc<Mutex<BTreeMap<u64, Output>>>,
}

#[derive(Default)]
struct Output {
    frames: VecDeque<zeroize::Zeroizing<Vec<u8>>>,
    bytes: usize,
    private: bool,
}

impl Output {
    fn push(&mut self, frame: Vec<u8>) -> tembra::Result<()> {
        if frame.len() > if self.private { 400_000 } else { MAX_FRAME }
            || self.frames.len() >= if self.private { 8 } else { MAX_OUTPUT_FRAMES }
            || frame.len() > MAX_OUTPUT_BYTES.saturating_sub(self.bytes)
        {
            return Err(tembra::Error::new(
                tembra::Code::Exhausted,
                "desktop.output",
                "native output budget exhausted",
            ));
        }
        self.bytes += frame.len();
        self.frames.push_back(zeroize::Zeroizing::new(frame));
        Ok(())
    }

    fn drain(&mut self) -> Vec<Vec<u8>> {
        let mut frames = Vec::new();
        let mut bytes = 0;
        while frames.len() < MAX_POLL_FRAMES {
            let Some(next) = self.frames.front() else {
                break;
            };
            if next.len()
                > if self.private {
                    400_000
                } else {
                    MAX_POLL_BYTES
                } - bytes
            {
                break;
            }
            bytes += next.len();
            let mut frame = self.frames.pop_front().expect("the front frame exists");
            frames.push(std::mem::take(&mut *frame));
        }
        self.bytes -= bytes;
        frames
    }
}

impl Channel {
    fn is_open(&self) -> bool {
        !self.connection.is_closed()
            && self
                .output
                .lock()
                .is_ok_and(|output| output.contains_key(&self.generation))
    }

    fn new(
        host: &Host,
        generation: u64,
        private: bool,
        output: Arc<Mutex<BTreeMap<u64, Output>>>,
    ) -> Result<Self, String> {
        output
            .lock()
            .map_err(|_| "native output is unavailable")?
            .insert(
                generation,
                Output {
                    private,
                    ..Output::default()
                },
            );
        let outgoing = output.clone();
        let mut options = if private {
            rpc::Options::new(crate::control::PRINCIPAL.into(), |service, key| {
                service == crate::control::SERVICE && key.is_none_or(|key| key == ["main"])
            })
        } else {
            rpc::Options::new(PRINCIPAL.into(), permits_service)
        };
        options.max_frame = if private { 400_000 } else { MAX_FRAME };
        options.max_pending = if private { 8 } else { 16 };
        options.max_references = if private { 4 } else { 16 };
        let connection = rpc::serve(host.clone(), options, move |frame| {
            let mut output = outgoing.lock().map_err(|_| {
                tembra::Error::new(
                    tembra::Code::Unavailable,
                    "desktop.output",
                    "native output is unavailable",
                )
            })?;
            let result = output
                .get_mut(&generation)
                .ok_or_else(|| {
                    tembra::Error::new(tembra::Code::Gone, "desktop.output", "native channel ended")
                })?
                .push(frame);
            if private && result.is_err() {
                output.remove(&generation);
            }
            result
        });
        Ok(Self {
            private,
            connection,
            generation,
            output,
        })
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        self.connection.close();
        if let Ok(mut output) = self.output.lock() {
            output.remove(&self.generation);
        }
    }
}

fn permits_service(service: &str, key: Option<&[String]>) -> bool {
    (service == swarmie_client_node::AUTHORIZATION_SERVICE
        && key.is_none_or(|key| matches!(key.len(), 2 | 3)))
        || (matches!(
            service,
            "swarmie.node"
                | "swarmie.providers.settings"
                | "swarmie.conversation.catalog"
                | "swarmie.conversation.session"
                | "swarmie.context"
                | "swarmie.commands"
                | "swarmie.mcp.settings"
                | "swarmie.tools"
                | "swarmie.directories"
                | "swarmie.git"
        ) && key.is_none_or(|key| {
            if matches!(
                service,
                "swarmie.conversation.session" | "swarmie.context" | "swarmie.commands"
            ) {
                key.len() == 1
            } else {
                key == ["main"]
            }
        }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn authorization_allows_new_and_returning_sessions_only() {
        let service = swarmie_client_node::AUTHORIZATION_SERVICE;
        assert!(permits_service(service, None));
        for size in 0..=4 {
            let key = vec!["value".to_owned(); size];
            assert_eq!(permits_service(service, Some(&key)), matches!(size, 2 | 3));
        }
        assert!(!permits_service("swarmie.mcp.configured", None));
    }

    #[test]
    fn output_budgets_bound_each_poll_and_retain_frame_order() {
        let mut output = Output::default();
        assert!(output.push(vec![0; MAX_FRAME + 1]).is_err());
        for marker in 0..32 {
            output.push(vec![marker; MAX_FRAME]).unwrap();
        }
        assert!(output.push(vec![0]).is_err());
        for marker in 0..32 {
            assert_eq!(output.drain(), vec![vec![marker; MAX_FRAME]]);
        }
        assert_eq!(output.bytes, 0);
        for _ in 0..MAX_OUTPUT_FRAMES {
            output.push(vec![1]).unwrap();
        }
        assert!(output.push(vec![1]).is_err());
        assert_eq!(output.drain().len(), MAX_POLL_FRAMES);
        assert_eq!(output.drain().len(), MAX_POLL_FRAMES);
        assert!(output.drain().is_empty());
        assert_eq!(output.bytes, 0);
        output.push(vec![255; MAX_FRAME]).unwrap();
        output.push(vec![255; MAX_POLL_BYTES - MAX_FRAME]).unwrap();
        output.push(vec![255; MAX_FRAME]).unwrap();
        let batch = output.drain();
        assert_eq!(batch.iter().map(Vec::len).sum::<usize>(), MAX_POLL_BYTES);
        let exchange = serde_json::json!({"ok": true, "value": batch});
        assert!(serde_json::to_vec(&exchange).unwrap().len() < 512 * 1024);
        assert_eq!(output.bytes, MAX_FRAME);
        assert_eq!(output.drain(), vec![vec![255; MAX_FRAME]]);
    }

    fn request(node: &Node, generation: u64, id: u64, op: &str, payload: Value) -> Value {
        node.frame(
            generation,
            serde_json::to_vec(&json!({"id":id,"op":op,"payload":payload})).unwrap(),
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while std::time::Instant::now() < deadline {
            for bytes in node.poll(generation).unwrap() {
                let value: Value = serde_json::from_slice(&bytes).unwrap();
                if value["id"] == id {
                    return value;
                }
            }
            std::thread::yield_now();
        }
        panic!("native RPC response timed out");
    }

    #[test]
    fn native_rpc_persists_definitions_rejects_stale_channels_and_retains_identity_on_restart() {
        let directory = tempfile::tempdir().unwrap();
        let identity;
        {
            let node = Node::open(directory.path().into()).unwrap();
            identity = node.descriptor();
            assert!(
                Node::open(directory.path().into()).is_err(),
                "one physical owner writes settings"
            );
            let old = node.open_channel().unwrap();
            let current = node.open_channel().unwrap();
            assert!(node.poll(old).is_err());
            let acquired = request(
                &node,
                current,
                1,
                "get",
                json!({"service":"swarmie.mcp.settings","key":["main"]}),
            );
            let reference = acquired["ok"]["reference"].as_u64().unwrap();
            let definition = json!({"id":"documents","name":"Documents","scope":{"kind":"user"},"endpoint":{"kind":"http","url":"http://127.0.0.1:9/mcp","headers":{}},"protocol":"2026-07-28","enabled":false,"revision":1});
            let saved = request(
                &node,
                current,
                2,
                "call",
                json!({"reference":reference,"method":"save","arguments":[{"definition":definition,"expected_revision":null}]}),
            );
            assert_eq!(saved["ok"]["revision"], 1);
            let forbidden = request(
                &node,
                current,
                3,
                "get",
                json!({"service":"swarmie.mcp.configured","key":["documents","1"]}),
            );
            assert_eq!(forbidden["error"]["code"], "forbidden");
        }
        let node = Node::open(directory.path().into()).unwrap();
        assert_eq!(node.descriptor(), identity);
        let generation = node.open_channel().unwrap();
        let acquired = request(
            &node,
            generation,
            1,
            "get",
            json!({"service":"swarmie.mcp.settings","key":["main"]}),
        );
        let result = request(
            &node,
            generation,
            2,
            "call",
            json!({"reference":acquired["ok"]["reference"],"method":"list","arguments":[]}),
        );
        assert_eq!(result["ok"][0]["definition"]["name"], "Documents");
        assert_eq!(result["ok"][0]["observation"]["status"], "disconnected");
    }

    #[test]
    fn native_startup_uses_managed_identity_and_refuses_disabled_inventory_intact() {
        use swarmie_plugin_manager::{PluginChange, PluginStorage};
        let directory = tempfile::tempdir().unwrap();
        let identity;
        {
            let node = Node::open(directory.path().into()).unwrap();
            identity = node.descriptor();
        }
        let file = directory.path().join("runtime.sqlite");
        let storage = swarmie_storage::sqlite::SqliteStorage::open(&file).unwrap();
        let before = futures::executor::block_on(storage.inventory()).unwrap();
        assert_eq!(before.revision, 1);
        assert_eq!(before.plugins.len(), 2);
        let mut node = before
            .plugins
            .iter()
            .find(|plugin| plugin.id.as_str() == swarmie_runtime::NODE_SERVICE)
            .unwrap()
            .clone();
        node.enabled = false;
        futures::executor::block_on(storage.commit(before.revision, vec![PluginChange::Put(node)]))
            .unwrap();
        let expected = futures::executor::block_on(storage.inventory()).unwrap();
        futures::executor::block_on(storage.close()).unwrap();
        let accounts = directory.path().join("providers.json");
        std::fs::write(&accounts, b"[]").unwrap();
        assert!(Node::open(directory.path().into()).is_err());
        assert_eq!(
            std::fs::read_to_string(directory.path().join("node-id")).unwrap(),
            identity["id"].as_str().unwrap()
        );
        assert_eq!(std::fs::read(&accounts).unwrap(), b"[]");
        let storage = swarmie_storage::sqlite::SqliteStorage::open(&file).unwrap();
        assert_eq!(
            futures::executor::block_on(storage.inventory()).unwrap(),
            expected
        );
        futures::executor::block_on(storage.close()).unwrap();
    }

    #[test]
    fn native_rpc_lists_the_configured_directory_root_and_refuses_an_outside_path() {
        let state = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("Projects")).unwrap();
        let node =
            Node::with_directory_root(state.path().into(), Some(root.path().into())).unwrap();
        let generation = node.open_channel().unwrap();
        let acquired = request(
            &node,
            generation,
            1,
            "get",
            json!({"service":"swarmie.directories","key":["main"]}),
        );
        let reference = acquired["ok"]["reference"].as_u64().unwrap();
        let listed = request(
            &node,
            generation,
            2,
            "call",
            json!({"reference":reference,"method":"list","arguments":[{"path":null,"cursor":null}]}),
        );
        assert_eq!(
            listed["ok"]["directories"][0]["name"], "Projects",
            "{listed}"
        );
        assert_eq!(listed["ok"]["parent"], Value::Null);
        let refused = request(
            &node,
            generation,
            3,
            "call",
            json!({"reference":reference,"method":"list","arguments":[{"path":state.path().to_str().unwrap(),"cursor":null}]}),
        );
        assert_eq!(refused["error"]["code"], "forbidden");
    }

    #[test]
    #[cfg(unix)]
    fn git_shares_the_native_host_and_requires_a_separate_directory_grant() {
        let state = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("project");
        std::fs::create_dir(&repository).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(&repository)
                .status()
                .unwrap()
                .success()
        );
        let node =
            Node::with_directory_root(state.path().into(), Some(root.path().into())).unwrap();
        let channel = node.open_channel().unwrap();
        let directories = request(
            &node,
            channel,
            1,
            "get",
            json!({"service":"swarmie.directories","key":["main"]}),
        );
        let listed = request(
            &node,
            channel,
            2,
            "call",
            json!({"reference": directories["ok"]["reference"],"method":"list","arguments":[{"path":null,"cursor":null}]}),
        );
        assert_eq!(listed["ok"]["directories"][0]["name"], "project");
        let git = request(
            &node,
            channel,
            3,
            "get",
            json!({"service":"swarmie.git","key":["main"]}),
        );
        let reference = git["ok"]["reference"]
            .as_u64()
            .unwrap_or_else(|| panic!("Git acquisition failed: {git}"));
        let discover = |id| {
            request(
                &node,
                channel,
                id,
                "call",
                json!({"reference":reference,"method":"request","arguments":[{"kind":"discover","path":repository}]}),
            )
        };
        assert!(
            discover(4).get("error").is_some(),
            "listing a directory must not authorize Git"
        );
        node.grant_directory(&repository).unwrap();
        let discovered = discover(5);
        assert_eq!(discovered["ok"]["kind"], "repository", "{discovered}");
        let target = discovered["ok"]["repository"].clone();
        let status = request(
            &node,
            channel,
            6,
            "call",
            json!({"reference":reference,"method":"request","arguments":[{"kind":"status","target":target}]}),
        );
        assert_eq!(status["ok"]["kind"], "status", "{status}");
        let settings = request(
            &node,
            channel,
            7,
            "call",
            json!({"reference":reference,"method":"request","arguments":[{"kind":"settings_get","target":target,"project_id":null}]}),
        );
        assert_eq!(settings["ok"]["kind"], "settings", "{settings}");
        let providers = request(
            &node,
            channel,
            8,
            "get",
            json!({"service":"swarmie.providers.settings","key":["main"]}),
        );
        assert!(
            providers["ok"]["reference"].is_u64(),
            "Git must share the existing provider host: {providers}"
        );
    }

    #[test]
    fn corrupt_settings_are_reported_without_overwriting_the_saved_file() {
        let directory = tempfile::tempdir().unwrap();
        let contents = b"invalid existing settings";
        std::fs::write(directory.path().join("mcp.json"), contents).unwrap();
        let node = Node::open(directory.path().into()).unwrap();
        let generation = node.open_channel().unwrap();
        let acquired = request(
            &node,
            generation,
            1,
            "get",
            json!({"service":"swarmie.mcp.settings","key":["main"]}),
        );
        let result = request(
            &node,
            generation,
            2,
            "call",
            json!({"reference":acquired["ok"]["reference"],"method":"list","arguments":[]}),
        );
        assert_eq!(result["error"]["code"], "unavailable");
        assert_eq!(
            std::fs::read(directory.path().join("mcp.json")).unwrap(),
            contents
        );
    }

    #[test]
    fn private_output_overflow_ends_that_attachment_without_stranding_pending_calls() {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::open(directory.path().into()).unwrap();
        let public = node.open_shared_channel().unwrap();
        let private = node.private_port();
        let generation = private.open().unwrap();
        {
            let mut output = node.output.lock().unwrap();
            let queue = output.get_mut(&generation).unwrap();
            for _ in 0..8 {
                queue.push(b"{}".to_vec()).unwrap();
            }
        }
        private.send(generation, &serde_json::to_vec(&json!({"id":1,"op":"get","payload":{"service":crate::control::SERVICE,"key":["main"]}})).unwrap()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while node.output.lock().unwrap().contains_key(&generation) {
            assert!(
                std::time::Instant::now() < deadline,
                "private overflow must end the source rather than silently lose a reply"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(private.poll(generation).is_err());
        assert_eq!(node.poll(public), Ok(vec![]));
    }

    #[test]
    fn polling_remains_available_while_the_capability_worker_is_busy() {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::open(directory.path().into()).unwrap();
        let channel = node.open_shared_channel().unwrap();
        let private = node.private_port();
        let private_generation = private.open().unwrap();
        let (entered, started) = mpsc::sync_channel(1);
        let (release, resume) = mpsc::sync_channel(1);
        node.sender.send(Command::Pause(entered, resume)).unwrap();
        started.recv().unwrap();

        let result = node.poll(channel);
        let private_result = private.poll(private_generation);
        private.close(private_generation);
        let private_closed = private.poll(private_generation);
        release.send(()).unwrap();

        assert_eq!(result, Ok(vec![]));
        assert_eq!(private_result, Ok(vec![]));
        assert_eq!(private_closed.unwrap_err(), "native channel ended");
        assert!(node.poll(channel).unwrap().is_empty());
    }

    #[test]
    fn busy_worker_retains_rpc_output_and_accepts_close_with_a_full_input_queue() {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::open(directory.path().into()).unwrap();
        let first = node.open_shared_channel().unwrap();
        let second = node.open_shared_channel().unwrap();
        node.frame(
            first,
            serde_json::to_vec(&json!({
                "id": 1,
                "op": "get",
                "payload": {"service": "swarmie.node", "key": ["main"]}
            }))
            .unwrap(),
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while node.output.lock().unwrap()[&first].frames.is_empty() {
            assert!(
                std::time::Instant::now() < deadline,
                "RPC output did not arrive"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let (entered, started) = mpsc::sync_channel(1);
        let (release, resume) = mpsc::sync_channel(1);
        node.sender.send(Command::Pause(entered, resume)).unwrap();
        started.recv().unwrap();
        for _ in 0..64 {
            node.frame(u64::MAX, vec![]).unwrap();
        }

        let full = node.frame(u64::MAX, vec![]);
        let buffered = node.poll(first);
        let empty = node.poll(first);
        let other = node.poll(second);
        node.close_channel(first);
        let closed = node.poll(first);
        release.send(()).unwrap();

        assert_eq!(full.unwrap_err(), "native input budget exhausted");
        let frames = buffered.unwrap();
        assert_eq!(frames.len(), 1);
        let response: Value = serde_json::from_slice(&frames[0]).unwrap();
        assert_eq!(response["id"], 1);
        assert!(response["ok"]["reference"].is_number(), "{response}");
        assert_eq!(empty, Ok(vec![]));
        assert_eq!(other, Ok(vec![]));
        assert_eq!(closed.unwrap_err(), "native channel ended");
        assert!(node.poll(second).unwrap().is_empty());
    }

    #[test]
    fn malformed_input_ends_only_its_channel_before_the_next_poll() {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::open(directory.path().into()).unwrap();
        let first = node.open_shared_channel().unwrap();
        let second = node.open_shared_channel().unwrap();
        node.frame(first, b"invalid JSON".to_vec()).unwrap();
        let (entered, started) = mpsc::sync_channel(1);
        let (release, resume) = mpsc::sync_channel(1);
        node.sender.send(Command::Pause(entered, resume)).unwrap();
        started.recv().unwrap();

        let closed = node.poll(first);
        let open = node.poll(second);
        release.send(()).unwrap();

        assert_eq!(closed.unwrap_err(), "native channel ended");
        assert_eq!(open, Ok(vec![]));
    }

    #[test]
    fn polling_batches_bursts_and_keeps_shared_generations_separate() {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::open(directory.path().into()).unwrap();
        let first = node.open_shared_channel().unwrap();
        let second = node.open_shared_channel().unwrap();
        for (generation, offset) in [(first, 0), (second, 100)] {
            for id in 1..=8 {
                node.frame(
                    generation,
                    serde_json::to_vec(&json!({
                        "id": offset + id,
                        "op": "get",
                        "payload": {"service": "swarmie.node", "key": ["main"]}
                    }))
                    .unwrap(),
                )
                .unwrap();
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        for (generation, offset) in [(first, 0), (second, 100)] {
            let frames = node.poll(generation).unwrap();
            assert_eq!(frames.len(), 8, "one poll must drain this bounded burst");
            let mut ids: Vec<_> = frames
                .iter()
                .map(|frame| {
                    serde_json::from_slice::<Value>(frame).unwrap()["id"]
                        .as_u64()
                        .unwrap()
                })
                .collect();
            ids.sort_unstable();
            assert_eq!(ids, (1..=8).map(|id| offset + id).collect::<Vec<_>>());
            assert!(node.poll(generation).unwrap().is_empty());
        }
        node.close_channel(first);
        assert!(node.poll(first).is_err());
        assert!(node.poll(second).is_ok());
        let replacement = node.open_channel().unwrap();
        assert!(node.poll(second).is_err());
        assert!(node.poll(replacement).is_ok());
    }
}
