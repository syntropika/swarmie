//! Persistent client-owned RPC channels. A dispatched request is never replayed.
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc, time::Duration};
use swarmie::{Error, LocalStoreIdentity, Result};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_tungstenite::{
    client_async, connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

#[derive(Clone)]
pub struct Channel {
    commands: mpsc::Sender<Command>,
    stop: tokio_util::sync::CancellationToken,
}
struct Command {
    request: Value,
    response: oneshot::Sender<Result<Value>>,
}
type ChannelSlot = Arc<tokio::sync::Mutex<Option<Channel>>>;
#[derive(Clone, Default)]
pub struct Channels {
    peers: Arc<tokio::sync::Mutex<HashMap<String, ChannelSlot>>>,
}
impl Channels {
    pub async fn request(
        &self,
        key: String,
        url: &str,
        token: Option<&str>,
        identity: &LocalStoreIdentity,
        events: &broadcast::Sender<Value>,
        request: Value,
    ) -> Result<Value> {
        let slot = self.peers.lock().await.entry(key).or_default().clone();
        let mut current = slot.lock().await;
        let peer = if let Some(peer) = current.as_ref().filter(|peer| !peer.commands.is_closed() && !peer.stop.is_cancelled()) {
            peer.clone()
        } else {
            let peer = Channel::connect(url, token, Some(identity.clone()), events.clone()).await?;
            *current = Some(peer.clone());
            peer
        };
        drop(current);
        peer.request(request).await
    }
    pub async fn remove(&self, keys: &[String]) {
        let removed = {
            let mut peers = self.peers.lock().await;
            keys.iter().filter_map(|key| peers.remove(key)).collect::<Vec<_>>()
        };
        for slot in removed {
            if let Some(channel) = slot.lock().await.as_ref() { channel.stop.cancel(); }
        }
    }
    pub async fn clear(&self) {
        let removed = self.peers.lock().await.drain().map(|(_, peer)| peer).collect::<Vec<_>>();
        for slot in removed {
            if let Some(channel) = slot.lock().await.as_ref() { channel.stop.cancel(); }
        }
    }
}
fn websocket_url(url: &str) -> Result<String> {
    let mut parsed = reqwest::Url::parse(url).map_err(|e| Error::Invalid(e.to_string()))?;
    match parsed.scheme() {
        "http" | "https" => {
            let scheme = if parsed.scheme() == "https" {
                "wss"
            } else {
                "ws"
            };
            parsed
                .set_scheme(scheme)
                .map_err(|_| Error::Invalid("invalid WebSocket endpoint".into()))?;
            let path = parsed
                .path()
                .trim_end_matches('/')
                .trim_end_matches("/rpc")
                .to_owned();
            parsed.set_path(&format!("{path}/ws"));
        }
        "ws" | "wss" => (),
        _ => return Err(Error::Invalid("unsupported node transport".into())),
    }
    Ok(parsed.to_string())
}
trait MessageSocket:
    futures_util::Stream<Item = std::result::Result<Message, tokio_tungstenite::tungstenite::Error>>
    + futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error>
    + Unpin
    + Send
{
}
impl<T> MessageSocket for T where
    T: futures_util::Stream<
            Item = std::result::Result<Message, tokio_tungstenite::tungstenite::Error>,
        > + futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error>
        + Unpin
        + Send
{
}

pub(crate) fn socket_path(url: &str) -> Result<String> {
    let parsed =
        reqwest::Url::parse(url).map_err(|_| Error::Invalid("invalid local socket URL".into()))?;
    if parsed.scheme() != "unix"
        || parsed.host_str().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(Error::Invalid(
            "local socket URL must be unix:///absolute/path".into(),
        ));
    }
    let file = reqwest::Url::parse(&url.replacen("unix:", "file:", 1))
        .map_err(|_| Error::Invalid("invalid socket path".into()))?;
    let path = file
        .to_file_path()
        .map_err(|_| Error::Invalid("socket path must be absolute".into()))?;
    Ok(path.to_string_lossy().into_owned())
}
pub(crate) fn ssh_route(url: &str) -> Result<(String, String)> {
    let parsed =
        reqwest::Url::parse(url).map_err(|_| Error::Invalid("invalid SSH route".into()))?;
    if parsed.scheme() != "ssh"
        || parsed.host_str() != Some("runtime")
        || parsed.path() != "/"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
    {
        return Err(Error::Invalid("invalid SSH proxy route".into()));
    }
    let pairs: Vec<_> = parsed.query_pairs().collect();
    if pairs.len() != 2 || pairs[0].0 != "host" || pairs[1].0 != "socket" {
        return Err(Error::Invalid("invalid SSH proxy route fields".into()));
    }
    let (host, path) = (pairs[0].1.to_string(), pairs[1].1.to_string());
    validate_ssh_host(&host)?;
    if (!path.is_empty() && !std::path::Path::new(&path).is_absolute())
        || path.chars().any(char::is_control)
        || path.len() > 8192
    {
        return Err(Error::Invalid("remote socket path must be absolute".into()));
    }
    Ok((host, path))
}
pub(crate) fn validate_ssh_host(host: &str) -> Result<()> {
    if host.is_empty()
        || host.starts_with('-')
        || host.len() > 1024
        || host.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        Err(Error::Invalid("invalid SSH host".into()))
    } else {
        Ok(())
    }
}
pub(crate) fn ssh_command(host: &str) -> Result<tokio::process::Command> {
    validate_ssh_host(host)?;
    let mut command = tokio::process::Command::new("ssh");
    command.args([
        "-T",
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=5",
        "-o",
        "ServerAliveInterval=5",
        "-o",
        "ServerAliveCountMax=3",
        "-o",
        "ControlMaster=no",
        "-o",
        "ControlPath=none",
        "-o",
        "ForkAfterAuthentication=no",
        "--",
        host,
    ]);
    command.kill_on_drop(true);
    Ok(command)
}
pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}
// A managed runtime remains reachable without editing remote shell startup files.
pub(crate) const SSH_RUNTIME: &str = "if command -v swarmie >/dev/null 2>&1; then SWARMIE=$(command -v swarmie); elif [ -x \"$HOME/.local/bin/swarmie\" ]; then SWARMIE=\"$HOME/.local/bin/swarmie\"; else exit 127; fi; ";
async fn connect_socket(
    url: &str,
    token: Option<&str>,
) -> Result<(Box<dyn MessageSocket>, Option<tokio::process::Child>)> {
    let scheme = reqwest::Url::parse(url)
        .map_err(|e| Error::Invalid(e.to_string()))?
        .scheme()
        .to_owned();
    let local = matches!(scheme.as_str(), "unix" | "ssh");
    let mut upgrade = if local {
        "ws://localhost/ws".to_owned()
    } else {
        websocket_url(url)?
    }
    .into_client_request()
    .map_err(|e| Error::Invalid(e.to_string()))?;
    if let Some(token) = token {
        upgrade.headers_mut().insert(
            "authorization",
            format!("Bearer {token}")
                .parse()
                .map_err(|_| Error::Invalid("invalid bearer token".into()))?,
        );
    }
    tokio::time::timeout(Duration::from_secs(8), async {
        match scheme.as_str() {
            "unix" => {
                #[cfg(unix)] {
                    let stream = tokio::net::UnixStream::connect(socket_path(url)?).await?;
                    let (socket, _) = client_async(upgrade, stream).await.map_err(|e| Error::Provider(e.to_string()))?;
                    Ok((Box::new(socket) as Box<dyn MessageSocket>, None))
                }
                #[cfg(not(unix))] { Err(Error::Unsupported("Unix sockets are unavailable on this platform".into())) }
            }
            "ssh" => {
                let (host, path) = ssh_route(url)?;
                let mut command = ssh_command(&host)?;
                let remote_command = if path.is_empty() { format!("{SSH_RUNTIME}exec \"$SWARMIE\" runtime proxy") } else { format!("{SSH_RUNTIME}exec \"$SWARMIE\" runtime proxy --socket {}", shell_quote(&path)) };
                command.arg(remote_command)
                    .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
                let mut child = command.spawn()?;
                let stream = tokio::io::join(child.stdout.take().ok_or(Error::Closed)?, child.stdin.take().ok_or(Error::Closed)?);
                let (socket, _) = client_async(upgrade, stream).await.map_err(|_| Error::Provider("SSH runtime proxy unavailable; verify installation, SSH authentication, runtime readiness and socket path".into()))?;
                Ok((Box::new(socket) as Box<dyn MessageSocket>, Some(child)))
            }
            _ => {
                let (socket, _) = connect_async(upgrade).await.map_err(|e| Error::Provider(e.to_string()))?;
                Ok((Box::new(socket) as Box<dyn MessageSocket>, None))
            }
        }
    }).await.map_err(|_| Error::Timeout("node connection".into()))?
}
async fn send_or_stop(socket: &mut Box<dyn MessageSocket>, frame: Message, stop: &tokio_util::sync::CancellationToken) -> bool {
    tokio::select! {
        biased;
        _ = stop.cancelled() => false,
        result = tokio::time::timeout(Duration::from_secs(5), socket.send(frame)) => matches!(result, Ok(Ok(()))),
    }
}
impl Channel {
    pub(crate) async fn connect(
        url: &str,
        token: Option<&str>,
        identity: Option<LocalStoreIdentity>,
        events: broadcast::Sender<Value>,
    ) -> Result<Self> {
        let (mut socket, process) = connect_socket(url, token).await?;
        socket
            .send(Message::Text(
                json!({"jsonrpc":"2.0","id":0,"method":"runtime/initialize","params":{}})
                    .to_string()
                    .into(),
            ))
            .await
            .map_err(|e| Error::Provider(e.to_string()))?;
        let initialized = tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(frame) = socket.next().await {
                if let Message::Text(text) = frame.map_err(|e| Error::Provider(e.to_string()))? {
                    let value: Value = serde_json::from_str(&text)?;
                    if value["id"] == 0 {
                        return Ok(value["result"].clone());
                    }
                }
            }
            Err(Error::Closed)
        })
        .await
        .map_err(|_| Error::Timeout("node initialization".into()))??;
        let target = crate::node_connection::handshake(&initialized, identity.as_ref())?;
        let identity = target.identity.clone();
        let (commands, mut rx) = mpsc::channel::<Command>(128);
        let stop = tokio_util::sync::CancellationToken::new();
        let cancelled = stop.clone();
        tokio::spawn(async move {
            let mut process = process;
            let mut pending =
                HashMap::<u64, (Value, oneshot::Sender<Result<Value>>, tokio::time::Instant)>::new(
                );
            let mut next_id = 0u64;
            let mut heartbeat = tokio::time::interval(Duration::from_secs(5));
            heartbeat.tick().await;
            let mut last_received = tokio::time::Instant::now();
            let node_event = |method: &str, params: Value| json!({"environmentId":identity.environment_id,"method":method,"params":params});
            let _ = events.send(node_event(
                "client/connected",
                serde_json::to_value(&target).unwrap_or(Value::Null),
            ));
            loop {
                tokio::select! {
                    biased;
                    _ = cancelled.cancelled() => break,
                    command = rx.recv() => {
                        let Some(mut command) = command else { break; };
                        if command.response.is_closed() { continue; }
                        if command.request["method"] != "runtime/initialize" && command.request.get("expectedRuntime") != Some(&serde_json::to_value(&target).unwrap_or(Value::Null)) {
                            let _ = command.response.send(Ok(json!({"jsonrpc":"2.0","id":command.request["id"],"error":{"code":-32005,"message":"runtime_target_mismatch","data":{"effectKnowledge":"none"}}})));
                            continue;
                        }
                        if pending.len() >= 128 {
                            let _ = command.response.send(Err(Error::Provider("node request capacity exceeded before dispatch".into())));
                            continue;
                        }
                        next_id += 1;
                        let original = command.request["id"].clone();
                        command.request["id"] = next_id.into();
                        pending.insert(next_id, (original, command.response, tokio::time::Instant::now()));
                        if !send_or_stop(&mut socket, Message::Text(command.request.to_string().into()), &cancelled).await { break; }
                    }
                    frame = socket.next() => {
                        last_received = tokio::time::Instant::now();
                        match frame {
                            Some(Ok(Message::Text(text))) => {
                                let Ok(mut value) = serde_json::from_str::<Value>(&text) else { break; };
                                if let Some(id) = value.get("id").and_then(Value::as_u64) {
                                    if let Some((original, response, _)) = pending.remove(&id) {
                                        value["id"] = original;
                                        let valid = value["jsonrpc"] == "2.0" && (value.get("result").is_some() ^ value.get("error").is_some());
                                        let _ = response.send(if valid { Ok(value) } else { Err(Error::Provider("invalid node response; delivery is uncertain".into())) });
                                    }
                                } else if let Some(method) = value["method"].as_str() {
                                    let _ = events.send(node_event(method, value["params"].clone()));
                                } else { break; }
                            }
                            Some(Ok(Message::Ping(data))) => { if !send_or_stop(&mut socket, Message::Pong(data), &cancelled).await { break; } }
                            Some(Ok(Message::Pong(_))) => (),
                            _ => break,
                        }
                    }
                    _ = heartbeat.tick() => {
                        if last_received.elapsed() > Duration::from_secs(20) { break; }
                        if !send_or_stop(&mut socket, Message::Ping(Vec::new().into()), &cancelled).await { break; }
                        let expired: Vec<_> = pending.iter().filter(|(_, (_, response, started))| response.is_closed() || started.elapsed() > Duration::from_secs(120)).map(|(id, _)| *id).collect();
                        for id in expired { if let Some((_, response, _)) = pending.remove(&id) { let _ = response.send(Err(Error::Timeout("node request; delivery is uncertain".into()))); } }
                    }
                }
            }
            rx.close();
            for (_, (_, response, _)) in pending {
                let _ = response.send(Err(Error::Provider(
                    "node disconnected; delivery is uncertain".into(),
                )));
            }
            while let Ok(command) = rx.try_recv() {
                let _ = command.response.send(Err(Error::Closed));
            }
            let _ = events.send(node_event("client/disconnected", Value::Null));
            let _ = tokio::time::timeout(Duration::from_secs(1), socket.close()).await;
            if let Some(mut child) = process.take() {
                let _ = child.kill().await;
                let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
            }
        });
        Ok(Self { commands, stop })
    }
    pub(crate) async fn request(&self, request: Value) -> Result<Value> {
        if self.stop.is_cancelled() { return Err(Error::Closed); }
        let (response, result) = oneshot::channel();
        self.commands
            .try_send(Command { request, response })
            .map_err(|_| {
                Error::Provider("node request queue is full or closed before dispatch".into())
            })?;
        tokio::time::timeout(Duration::from_secs(125), result)
            .await
            .map_err(|_| Error::Timeout("node request; delivery is uncertain".into()))?
            .map_err(|_| Error::Provider("node disconnected; delivery is uncertain".into()))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio_tungstenite::accept_async;
    async fn fixture(
        drop_prompt: bool,
    ) -> (
        String,
        swarmie::RuntimeIdentity,
        Arc<AtomicUsize>,
        Arc<AtomicUsize>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let target = swarmie::Runtime::default().identity().await.unwrap();
        let accepts = Arc::new(AtomicUsize::new(0));
        let prompts = Arc::new(AtomicUsize::new(0));
        let (a, p, t) = (accepts.clone(), prompts.clone(), target.clone());
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                a.fetch_add(1, Ordering::SeqCst);
                let p = p.clone();
                let t = t.clone();
                tokio::spawn(async move {
                    let mut socket = accept_async(stream).await.unwrap();
                    let mut responses = Vec::new();
                    while let Some(Ok(frame)) = socket.next().await {
                        match frame {
                            Message::Text(text) => {
                                let request: Value = serde_json::from_str(&text).unwrap();
                                let value = if request["method"] == "runtime/initialize" {
                                    json!({"name":"swarmie","protocolVersion":protocol::PROTOCOL_VERSION,"runtimeIdentity":t})
                                } else {
                                    request["params"].clone()
                                };
                                if request["method"] == "session/prompt" {
                                    p.fetch_add(1, Ordering::SeqCst);
                                    if drop_prompt {
                                        let _ = socket.close(None).await;
                                        break;
                                    }
                                }
                                let response =
                                    json!({"jsonrpc":"2.0","id":request["id"],"result":value});
                                if request["method"] == "test/echo" {
                                    responses.push(response);
                                    if responses.len() == 3 {
                                        socket.send(Message::Text(json!({"jsonrpc":"2.0","method":"session/event","params":{"sequence":1}}).to_string().into())).await.unwrap();
                                        for response in responses.drain(..).rev() {
                                            socket
                                                .send(Message::Text(response.to_string().into()))
                                                .await
                                                .unwrap();
                                        }
                                    }
                                } else {
                                    socket
                                        .send(Message::Text(response.to_string().into()))
                                        .await
                                        .unwrap();
                                }
                            }
                            Message::Ping(data) => {
                                let _ = socket.send(Message::Pong(data)).await;
                            }
                            _ => break,
                        }
                    }
                });
            }
        });
        (url, target, accepts, prompts, task)
    }
    #[tokio::test]
    async fn concurrent_calls_share_one_channel_and_correlate_duplicate_caller_ids_with_events() {
        let (url, target, accepts, _, task) = fixture(false).await;
        let channels = Channels::default();
        let (events, mut observed) = broadcast::channel(32);
        let mut calls = tokio::task::JoinSet::new();
        for index in 0..3 {
            let (channels, events, url, target) = (
                channels.clone(),
                events.clone(),
                url.clone(),
                target.clone(),
            );
            calls.spawn(async move {
            let response = channels.request("route".into(), &url, None, &target.identity, &events, json!({"jsonrpc":"2.0","id":"same-ui-id","method":"test/echo","params":{"index":index},"expectedRuntime":target})).await.unwrap();
            assert_eq!(response["id"], "same-ui-id"); assert_eq!(response["result"]["index"], index);
        });
        }
        tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(result) = calls.join_next().await {
                result.unwrap();
            }
        })
        .await
        .unwrap();
        assert_eq!(accepts.load(Ordering::SeqCst), 1);
        let mut got_event = false;
        while let Ok(event) = observed.try_recv() {
            got_event |= event["method"] == "session/event";
        }
        assert!(got_event);
        let rejected = channels.request("route".into(), &url, None, &target.identity, &events, json!({"jsonrpc":"2.0","id":5,"method":"session/prompt","params":{},"expectedRuntime":{"identity":target.identity,"incarnationId":"stale"}})).await.unwrap();
        assert_eq!(rejected["error"]["data"]["effectKnowledge"], "none");
        channels.clear().await;
        task.abort();
    }
    #[tokio::test]
    async fn disconnect_after_dispatch_retains_uncertainty_and_never_replays_the_prompt() {
        let (url, target, accepts, prompts, task) = fixture(true).await;
        let channels = Channels::default();
        let (events, _) = broadcast::channel(32);
        let error = channels.request("route".into(), &url, None, &target.identity, &events, json!({"jsonrpc":"2.0","id":1,"method":"session/prompt","params":{},"expectedRuntime":target})).await.unwrap_err();
        assert!(error.to_string().contains("uncertain"));
        channels
            .request(
                "route".into(),
                &url,
                None,
                &target.identity,
                &events,
                json!({"jsonrpc":"2.0","id":2,"method":"runtime/initialize","params":{}}),
            )
            .await
            .unwrap();
        assert_eq!(accepts.load(Ordering::SeqCst), 2);
        assert_eq!(prompts.load(Ordering::SeqCst), 1);
        channels.clear().await;
        task.abort();
    }
    #[tokio::test]
    async fn explicit_disconnect_cancels_pending_channel_without_waiting_for_request_deadline() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let target = swarmie::Runtime::default().identity().await.unwrap();
        let fixture_target = target.clone();
        let (dispatched, observed_dispatch) = oneshot::channel();
        let fixture = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            let mut dispatched = Some(dispatched);
            while let Some(Ok(frame)) = socket.next().await {
                match frame {
                    Message::Text(text) => {
                        let request: Value = serde_json::from_str(&text).unwrap();
                        if request["method"] == "runtime/initialize" {
                            socket.send(Message::Text(json!({"jsonrpc":"2.0","id":request["id"],"result":{"name":"swarmie","protocolVersion":protocol::PROTOCOL_VERSION,"runtimeIdentity":fixture_target}}).to_string().into())).await.unwrap();
                        } else {
                            if let Some(dispatched) = dispatched.take() { let _ = dispatched.send(()); }
                        }
                    }
                    Message::Ping(data) => { let _ = socket.send(Message::Pong(data)).await; }
                    _ => break,
                }
            }
        });
        let channels = Channels::default();
        let call_channels = channels.clone();
        let (events, _) = broadcast::channel(16);
        let pending = tokio::spawn(async move {
            call_channels.request("pending".into(), &url, None, &target.identity, &events, json!({"jsonrpc":"2.0","id":1,"method":"test/hold","params":{},"expectedRuntime":target})).await
        });
        tokio::time::timeout(Duration::from_secs(3), observed_dispatch).await.unwrap().unwrap();
        channels.remove(&["pending".into()]).await;
        let error = tokio::time::timeout(Duration::from_secs(2), pending).await.unwrap().unwrap().unwrap_err();
        assert!(error.to_string().contains("uncertain"));
        tokio::time::timeout(Duration::from_secs(2), fixture).await.unwrap().unwrap();
    }

}
