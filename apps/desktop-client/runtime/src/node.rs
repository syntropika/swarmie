use futures::task::{LocalFutureObj, LocalSpawn, SpawnError};
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    rc::Rc,
    sync::{Arc, mpsc},
    thread,
};
use swarmie_runtime::{NodeId, rpc};
use tembra::Host;

pub const PRINCIPAL: &str = "desktop-user";
const MAX_FRAME: usize = 64 * 1024;
const MAX_OUTPUT_FRAMES: usize = 256;
const MAX_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
const MAX_POLL_FRAMES: usize = 128;
const MAX_POLL_BYTES: usize = 96 * 1024;

enum Command {
    Open {
        shared: bool,
        response: mpsc::SyncSender<Result<u64, String>>,
    },
    Frame(u64, Vec<u8>),
    Poll(u64, mpsc::SyncSender<Result<Vec<Vec<u8>>, String>>),
    Close(u64),
    Stop,
}

#[derive(Clone, Copy)]
struct Executor;
impl LocalSpawn for Executor {
    fn spawn_local_obj(&self, future: LocalFutureObj<'static, ()>) -> Result<(), SpawnError> {
        tokio::task::spawn_local(future);
        Ok(())
    }
}

/// An embedding-owned worker. Capability calls use Tembra's existing byte protocol.
pub struct DesktopNode {
    sender: mpsc::SyncSender<Command>,
    worker: Option<thread::JoinHandle<()>>,
    id: String,
    _lock: std::fs::File,
    git_grants: Option<Arc<crate::git::DirectoryGrants>>,
}

impl DesktopNode {
    pub fn open(state: PathBuf) -> Result<Self, String> {
        let root = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
        Self::with_directory_root(state, root.map(PathBuf::from))
    }

    fn with_directory_root(
        state: PathBuf,
        directory_root: Option<PathBuf>,
    ) -> Result<Self, String> {
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
        let git_grants = cfg!(unix).then(|| Arc::new(crate::git::DirectoryGrants::default()));
        let git = git_grants
            .as_ref()
            .map(|grants| {
                crate::git::service(&state, grants.clone())
                    .map(|service| (service, grants.clone() as Arc<dyn swarmie_git::Authority>))
            })
            .transpose()?;
        let (sender, receiver) = mpsc::sync_channel(64);
        let (ready, started) = mpsc::sync_channel(1);
        let worker_id = id.clone();
        let worker = thread::Builder::new()
            .name("swarmie-desktop-runtime".into())
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
                    receiver,
                    ready,
                    state,
                    worker_id,
                    oauth_host,
                    directory_root,
                    git,
                )));
            })
            .map_err(|_| "native runtime could not start")?;
        started
            .recv()
            .map_err(|_| "native runtime did not initialize")??;
        Ok(Self {
            sender,
            worker: Some(worker),
            id,
            _lock: lock,
            git_grants,
        })
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
                response: send,
            })
            .map_err(|_| "native input budget exhausted")?;
        receive
            .recv_timeout(std::time::Duration::from_secs(2))
            .map_err(|_| "native channel did not open".to_owned())?
    }

    pub fn frame(&self, generation: u64, frame: Vec<u8>) -> Result<(), String> {
        if frame.len() > MAX_FRAME {
            return Err("native frame limit exceeded".into());
        }
        self.sender
            .try_send(Command::Frame(generation, frame))
            .map_err(|_| "native input budget exhausted".into())
    }

    pub fn poll(&self, generation: u64) -> Result<Vec<Vec<u8>>, String> {
        let (send, receive) = mpsc::sync_channel(1);
        self.sender
            .try_send(Command::Poll(generation, send))
            .map_err(|_| "native input budget exhausted")?;
        receive
            .recv_timeout(std::time::Duration::from_secs(2))
            .map_err(|_| "native channel did not respond".to_owned())?
    }

    pub fn close_channel(&self, generation: u64) {
        let _ = self.sender.try_send(Command::Close(generation));
    }
}

impl Drop for DesktopNode {
    fn drop(&mut self) {
        let _ = self.sender.send(Command::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        // Explicit release also closes a short fork/exec inheritance window in other workers.
        let _ = self._lock.unlock();
    }
}

async fn serve(
    receiver: mpsc::Receiver<Command>,
    ready: mpsc::SyncSender<Result<(), String>>,
    state: PathBuf,
    id: String,
    oauth_host: uuid::Uuid,
    directory_root: Option<PathBuf>,
    git: Option<(swarmie_git::GitService, Arc<dyn swarmie_git::Authority>)>,
) {
    let host = Host::new(Executor);
    let (git, agent_git_authority) = match git {
        Some((service, authority)) => (Some(service), Some(authority)),
        None => (None, None),
    };
    let configuration = NodeId::new(id).map(|node| swarmie_client_node::NodeConfiguration {
        state,
        node,
        oauth_host,
        principal: PRINCIPAL.into(),
        directory_root,
        git,
        agent_git_authority,
    });
    let installed =
        configuration.and_then(|configuration| swarmie_client_node::install(&host, configuration));
    if let Err(error) = installed {
        let _ = host.close().await;
        let _ = ready.send(Err(error.to_string()));
        return;
    }
    if ready.send(Ok(())).is_err() {
        let _ = host.close().await;
        return;
    }
    let mut generation: u64 = 0;
    let mut channels = BTreeMap::<u64, Channel>::new();
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(2));
    'running: loop {
        tick.tick().await;
        for _ in 0..64 {
            let command = match receiver.try_recv() {
                Ok(command) => command,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => break 'running,
            };
            match command {
                Command::Open { shared, response } => {
                    if !shared {
                        for channel in channels.values() {
                            channel.connection.close();
                        }
                        channels.clear();
                    }
                    channels.retain(|_, channel| !channel.connection.is_closed());
                    if channels.len() >= 8 {
                        let _ = response.send(Err("native client limit reached".into()));
                        continue;
                    }
                    generation += 1;
                    channels.insert(generation, Channel::new(&host));
                    let _ = response.send(Ok(generation));
                }
                Command::Frame(version, frame) => {
                    if let Some(channel) = channels.get(&version) {
                        let _ = channel.connection.receive(&frame);
                    }
                }
                Command::Poll(version, response) => {
                    let result = match channels.get(&version) {
                        Some(channel) if !channel.connection.is_closed() => {
                            Ok(channel.output.borrow_mut().drain())
                        }
                        _ => Err("native channel ended".into()),
                    };
                    let _ = response.send(result);
                }
                Command::Close(version) => {
                    if let Some(channel) = channels.remove(&version) {
                        channel.connection.close();
                    }
                }
                Command::Stop => break 'running,
            }
        }
    }
    for channel in channels.values() {
        channel.connection.close();
    }
    let _ = host.close().await;
}

struct Channel {
    connection: rpc::Connection,
    output: Rc<std::cell::RefCell<Output>>,
}

#[derive(Default)]
struct Output {
    frames: VecDeque<Vec<u8>>,
    bytes: usize,
}

impl Output {
    fn push(&mut self, frame: Vec<u8>) -> tembra::Result<()> {
        if frame.len() > MAX_FRAME
            || self.frames.len() >= MAX_OUTPUT_FRAMES
            || frame.len() > MAX_OUTPUT_BYTES.saturating_sub(self.bytes)
        {
            return Err(tembra::Error::new(
                tembra::Code::Exhausted,
                "desktop.output",
                "native output budget exhausted",
            ));
        }
        self.bytes += frame.len();
        self.frames.push_back(frame);
        Ok(())
    }

    fn drain(&mut self) -> Vec<Vec<u8>> {
        let mut frames = Vec::new();
        let mut bytes = 0;
        while frames.len() < MAX_POLL_FRAMES {
            let Some(next) = self.frames.front() else {
                break;
            };
            if next.len() > MAX_POLL_BYTES - bytes {
                break;
            }
            bytes += next.len();
            frames.push(self.frames.pop_front().expect("the front frame exists"));
        }
        self.bytes -= bytes;
        frames
    }
}

impl Channel {
    fn new(host: &Host) -> Self {
        let output = Rc::new(std::cell::RefCell::new(Output::default()));
        let outgoing = output.clone();
        let mut options = rpc::Options::new(PRINCIPAL.into(), permits_service);
        options.max_frame = MAX_FRAME;
        options.max_pending = 16;
        options.max_references = 16;
        let connection = rpc::serve(host.clone(), options, move |frame| {
            outgoing.borrow_mut().push(frame)
        });
        Self { connection, output }
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
                | "swarmie.mcp.settings"
                | "swarmie.tools"
                | "swarmie.directories"
                | "swarmie.git"
        ) && key.is_none_or(|key| {
            if service == "swarmie.conversation.session" {
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

    fn request(node: &DesktopNode, generation: u64, id: u64, op: &str, payload: Value) -> Value {
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
            let node = DesktopNode::open(directory.path().into()).unwrap();
            identity = node.descriptor();
            assert!(
                DesktopNode::open(directory.path().into()).is_err(),
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
        let node = DesktopNode::open(directory.path().into()).unwrap();
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
    fn native_rpc_lists_the_configured_directory_root_and_refuses_an_outside_path() {
        let state = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("Projects")).unwrap();
        let node = DesktopNode::with_directory_root(state.path().into(), Some(root.path().into()))
            .unwrap();
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
        let node = DesktopNode::with_directory_root(state.path().into(), Some(root.path().into()))
            .unwrap();
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
        let node = DesktopNode::open(directory.path().into()).unwrap();
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
    fn polling_batches_bursts_and_keeps_shared_generations_separate() {
        let directory = tempfile::tempdir().unwrap();
        let node = DesktopNode::open(directory.path().into()).unwrap();
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
