use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use subtle::ConstantTimeEq;
use swarmie_node::{HostConfiguration, MAX_FRAME_BYTES, Node};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    task::JoinSet,
};
use uuid::Uuid;

const MAX_CLIENTS: usize = 8;
const ENDPOINT_FILE: &str = "node-endpoint.json";
const MAX_ENDPOINT_BYTES: u64 = 4096;

fn read_endpoint(path: &Path) -> std::io::Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > MAX_ENDPOINT_BYTES {
        return Err(std::io::Error::other("invalid node endpoint file"));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_ENDPOINT_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_ENDPOINT_BYTES {
        return Err(std::io::Error::other("invalid node endpoint size"));
    }
    Ok(bytes)
}

/// Owner-only local authentication. This token grants ordinary node access,
/// never private operator consent or a caller-selected principal.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Endpoint {
    version: u32,
    node: String,
    address: SocketAddr,
    token: Uuid,
    max_frame_bytes: usize,
}

fn existing_endpoint(profile: &Path) -> Result<Option<Endpoint>, String> {
    match read_endpoint(&profile.join(ENDPOINT_FILE)) {
        Ok(bytes) => {
            let previous: Endpoint = serde_json::from_slice(&bytes)
                .map_err(|_| "existing node endpoint is not recognized")?;
            if previous.version != 1 {
                return Err("existing node endpoint belongs to another owner".into());
            }
            Ok(Some(previous))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("node endpoint is unavailable".into()),
    }
}

struct Publication {
    path: PathBuf,
    token: Uuid,
}

impl Publication {
    fn publish(profile: &Path, endpoint: &Endpoint) -> Result<Self, String> {
        let path = profile.join(ENDPOINT_FILE);
        if let Some(previous) = existing_endpoint(profile)?
            && previous.node != endpoint.node
        {
            return Err("existing node endpoint belongs to another owner".into());
        }
        let temporary = profile.join(format!("node-endpoint-{}.pending", Uuid::new_v4()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|_| "node endpoint could not be created")?;
        let written = (|| {
            let bytes = serde_json::to_vec(endpoint).map_err(std::io::Error::other)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&temporary, &path)?;
            fs::File::open(profile)?.sync_all()
        })();
        if written.is_err() {
            let _ = fs::remove_file(&temporary);
            return Err("node endpoint could not be published".into());
        }
        Ok(Self {
            path,
            token: endpoint.token,
        })
    }
}

impl Drop for Publication {
    fn drop(&mut self) {
        // Remove only this owner's recognized ephemeral endpoint. The profile,
        // identity, history and protected records remain owned by the user.
        if read_endpoint(&self.path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Endpoint>(&bytes).ok())
            .is_some_and(|endpoint| endpoint.token == self.token)
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

struct Attachment {
    node: Arc<Node>,
    channel: u64,
}

impl Drop for Attachment {
    fn drop(&mut self) {
        self.node.close_channel(self.channel);
    }
}

pub fn run() -> Result<(), String> {
    let mut arguments = std::env::args_os().skip(1);
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new("--profile")) {
        return Err("usage: swarmie-node --profile <directory>".into());
    }
    let profile = arguments
        .next()
        .map(PathBuf::from)
        .ok_or("a node profile directory is required")?;
    if arguments.next().is_some() {
        return Err("usage: swarmie-node --profile <directory>".into());
    }
    // Reject malformed user-owned metadata before starting any node resources.
    // This grants no ownership: publication checks it again under the profile lock.
    let _ = existing_endpoint(&profile)?;
    let node = Node::with_configuration(HostConfiguration {
        protected_profile: profile.join("vault"),
        profile: profile.clone(),
        directory_root: None,
    })?;
    // No independent host is installed at login or launched implicitly.
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "node I/O executor could not start")?
        .block_on(serve(node, profile))
}

async fn serve(node: Node, profile: PathBuf) -> Result<(), String> {
    fs::set_permissions(&profile, fs::Permissions::from_mode(0o700))
        .map_err(|_| "node profile permissions could not be set")?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|_| "local node listener could not start")?;
    let token = Uuid::new_v4();
    let endpoint = Endpoint {
        version: 1,
        node: node.descriptor()["id"]
            .as_str()
            .ok_or("node identity is unavailable")?
            .into(),
        address: listener
            .local_addr()
            .map_err(|_| "node listener is unavailable")?,
        token,
        max_frame_bytes: MAX_FRAME_BYTES,
    };
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|_| "node shutdown signal is unavailable")?;
    let _publication = Publication::publish(&profile, &endpoint)?;
    let node = Arc::new(node);
    let slots = Arc::new(Semaphore::new(MAX_CLIENTS));
    let mut clients = JoinSet::new();
    let outcome = loop {
        tokio::select! {
            interrupted = tokio::signal::ctrl_c() => {
                break interrupted.map_err(|_| "node shutdown signal failed".to_owned());
            }
            _ = terminate.recv() => break Ok(()),
            result = clients.join_next(), if !clients.is_empty() => { let _ = result; }
            accepted = listener.accept() => {
                let (stream, peer) = match accepted {
                    Ok(value) => value,
                    Err(_) => break Err("node listener failed".into()),
                };
                if !peer.ip().is_loopback() { continue; }
                let Ok(permit) = slots.clone().try_acquire_owned() else { continue; };
                let owner = node.clone();
                clients.spawn(async move {
                    let _permit = permit;
                    let _ = exchange(stream, owner, token).await;
                });
            }
        }
    };
    clients.abort_all();
    while clients.join_next().await.is_some() {}
    let node = Arc::try_unwrap(node).map_err(|_| "node clients did not release")?;
    let shutdown = node.shutdown();
    outcome.and(shutdown)
}

async fn exchange(mut stream: TcpStream, node: Arc<Node>, token: Uuid) -> Result<(), String> {
    let mut supplied = [0_u8; 16];
    tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut supplied))
        .await
        .map_err(|_| "local node authentication expired")?
        .map_err(|_| "local node authentication ended")?;
    if !bool::from(supplied.ct_eq(token.as_bytes())) {
        return Err("local node authentication refused".into());
    }
    let attachment = Attachment {
        channel: node.open_shared_channel()?,
        node,
    };
    stream
        .write_u8(1)
        .await
        .map_err(|_| "local node attachment ended")?;
    let (mut input, mut output) = stream.into_split();
    let receive = async {
        loop {
            let length = input
                .read_u32_le()
                .await
                .map_err(|_| "node client detached")? as usize;
            if length == 0 || length > MAX_FRAME_BYTES {
                return Err("node frame limit exceeded");
            }
            let mut frame = vec![0; length];
            input
                .read_exact(&mut frame)
                .await
                .map_err(|_| "node frame ended")?;
            attachment
                .node
                .frame(attachment.channel, frame)
                .map_err(|_| "node input budget exhausted")?;
        }
    };
    let send = async {
        let mut tick = tokio::time::interval(Duration::from_millis(2));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            for frame in attachment
                .node
                .poll(attachment.channel)
                .map_err(|_| "node channel ended")?
            {
                let length =
                    u32::try_from(frame.len()).map_err(|_| "node output frame is too large")?;
                output
                    .write_u32_le(length)
                    .await
                    .map_err(|_| "node client detached")?;
                output
                    .write_all(&frame)
                    .await
                    .map_err(|_| "node client detached")?;
            }
        }
    };
    let result: Result<(), &str> =
        tokio::select! { result = receive => result, result = send => result };
    result.map_err(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publication_revalidates_a_file_replaced_after_the_initial_check() {
        let profile = tempfile::tempdir().unwrap();
        let path = profile.path().join(ENDPOINT_FILE);
        let endpoint = Endpoint {
            version: 1,
            node: "fixture-node".into(),
            address: SocketAddr::from((Ipv4Addr::LOCALHOST, 1)),
            token: Uuid::new_v4(),
            max_frame_bytes: MAX_FRAME_BYTES,
        };
        fs::write(&path, serde_json::to_vec(&endpoint).unwrap()).unwrap();
        assert!(existing_endpoint(profile.path()).unwrap().is_some());
        fs::write(&path, b"user-owned replacement").unwrap();
        assert!(Publication::publish(profile.path(), &endpoint).is_err());
        assert_eq!(fs::read(path).unwrap(), b"user-owned replacement");
        assert_eq!(fs::read_dir(profile.path()).unwrap().count(), 1);
    }
}
