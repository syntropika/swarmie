use crate::definitions::Definitions;
use futures::task::{LocalFutureObj, LocalSpawn, SpawnError};
use std::{collections::VecDeque, path::PathBuf, rc::Rc, sync::mpsc, thread};
use swarmie_runtime::{NodeId, node_orb, rpc};
use swarmie_tools::Catalog;
use tembra::Host;

pub const PRINCIPAL: &str = "desktop-user";
const MAX_FRAME: usize = 64 * 1024;
const MAX_FRAMES: usize = 32;

enum Command {
    Open(mpsc::SyncSender<u64>),
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
}

impl DesktopNode {
    pub fn open(state: PathBuf) -> Result<Self, String> {
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
                runtime.block_on(
                    tokio::task::LocalSet::new()
                        .run_until(serve(receiver, ready, state, worker_id)),
                );
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
        })
    }

    pub fn descriptor(&self) -> serde_json::Value {
        serde_json::json!({"id": self.id, "principal": PRINCIPAL})
    }

    pub fn open_channel(&self) -> Result<u64, String> {
        let (send, receive) = mpsc::sync_channel(1);
        self.sender
            .try_send(Command::Open(send))
            .map_err(|_| "native input budget exhausted")?;
        receive
            .recv_timeout(std::time::Duration::from_secs(2))
            .map_err(|_| "native channel did not open".into())
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
    }
}

async fn serve(
    receiver: mpsc::Receiver<Command>,
    ready: mpsc::SyncSender<Result<(), String>>,
    state: PathBuf,
    id: String,
) {
    let host = Host::new(Executor);
    let catalog = Catalog::default();
    let settings_catalog = catalog.clone();
    let node = match NodeId::new(id) {
        Ok(node) => node,
        Err(error) => {
            let _ = ready.send(Err(error.to_string()));
            return;
        }
    };
    let installed = host.install(vec![
        node_orb(node),
        swarmie_tools::orb_with_catalog(host.clone(), move |principal| {
            if principal == PRINCIPAL {
                Ok(catalog.clone())
            } else {
                Err(tembra::Error::invalid("desktop.principal", "unbound user"))
            }
        }),
        swarmie_mcp::settings::settings_orb(
            host.clone(),
            Rc::new(Definitions(state.join("mcp.json"))),
            move |principal| {
                if principal == PRINCIPAL {
                    Ok(settings_catalog.clone())
                } else {
                    Err(tembra::Error::invalid("desktop.principal", "unbound user"))
                }
            },
        ),
    ]);
    if let Err(error) = installed {
        let _ = ready.send(Err(error.to_string()));
        return;
    }
    if ready.send(Ok(())).is_err() {
        let _ = host.close().await;
        return;
    }
    let output = Rc::new(std::cell::RefCell::new(VecDeque::<Vec<u8>>::new()));
    let mut generation: u64 = 0;
    let mut channel = None::<rpc::Connection>;
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
                Command::Open(response) => {
                    if let Some(channel) = channel.take() {
                        channel.close();
                    }
                    generation += 1;
                    output.borrow_mut().clear();
                    let mut options = rpc::Options::new(PRINCIPAL.into(), |service, key| {
                        matches!(
                            service,
                            "swarmie.node" | "swarmie.mcp.settings" | "swarmie.tools"
                        ) && key.is_none_or(|key| key == ["main"])
                    });
                    options.max_frame = MAX_FRAME;
                    options.max_pending = 16;
                    options.max_references = 16;
                    let outgoing = output.clone();
                    channel = Some(rpc::serve(host.clone(), options, move |frame| {
                        let mut output = outgoing.borrow_mut();
                        if output.len() >= MAX_FRAMES {
                            return Err(tembra::Error::new(
                                tembra::Code::Exhausted,
                                "desktop.output",
                                "native output budget exhausted",
                            ));
                        }
                        output.push_back(frame);
                        Ok(())
                    }));
                    let _ = response.send(generation);
                }
                Command::Frame(version, frame) if version == generation => {
                    if let Some(channel) = &channel {
                        let _ = channel.receive(&frame);
                    }
                }
                Command::Poll(version, response) => {
                    let result = if version != generation
                        || channel.as_ref().is_none_or(rpc::Connection::is_closed)
                    {
                        Err("native channel ended".into())
                    } else {
                        {
                            let count = output.borrow().len().min(1);
                            Ok(output.borrow_mut().drain(..count).collect())
                        }
                    };
                    let _ = response.send(result);
                }
                Command::Close(version) if version == generation => {
                    if let Some(channel) = channel.take() {
                        channel.close();
                    }
                }
                Command::Stop => break 'running,
                _ => {}
            }
        }
    }
    if let Some(channel) = channel.take() {
        channel.close();
    }
    let _ = host.close().await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

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
}
