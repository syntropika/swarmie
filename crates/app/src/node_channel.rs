//! Persistent client-owned RPC channels. A dispatched request is never replayed.
use futures_util::{SinkExt, StreamExt};
use mpai::{Error, LocalStoreIdentity, Result};
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

#[derive(Clone)]
pub struct Channel {
    commands: mpsc::Sender<Command>,
}
struct Command {
    request: Value,
    response: oneshot::Sender<Result<Value>>,
}
#[derive(Clone, Default)]
pub struct Channels {
    peers: Arc<tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<Option<Channel>>>>>>,
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
        let peer = if let Some(peer) = current.as_ref().filter(|peer| !peer.commands.is_closed()) {
            peer.clone()
        } else {
            let peer = Channel::connect(url, token, identity.clone(), events.clone()).await?;
            *current = Some(peer.clone());
            peer
        };
        drop(current);
        peer.request(request).await
    }
    pub async fn remove(&self, keys: &[String]) {
        let mut peers = self.peers.lock().await;
        for key in keys {
            peers.remove(key);
        }
    }
    pub async fn clear(&self) {
        self.peers.lock().await.clear();
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
impl Channel {
    async fn connect(
        url: &str,
        token: Option<&str>,
        identity: LocalStoreIdentity,
        events: broadcast::Sender<Value>,
    ) -> Result<Self> {
        let mut upgrade = websocket_url(url)?
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
        let (mut socket, _) = tokio::time::timeout(Duration::from_secs(5), connect_async(upgrade))
            .await
            .map_err(|_| Error::Timeout("node connection".into()))?
            .map_err(|e| Error::Provider(e.to_string()))?;
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
        let target = crate::node_connection::handshake(&initialized, Some(&identity))?;
        let (commands, mut rx) = mpsc::channel::<Command>(128);
        tokio::spawn(async move {
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
                        if socket.send(Message::Text(command.request.to_string().into())).await.is_err() { break; }
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
                            Some(Ok(Message::Ping(data))) => { if socket.send(Message::Pong(data)).await.is_err() { break; } }
                            Some(Ok(Message::Pong(_))) => (),
                            _ => break,
                        }
                    }
                    _ = heartbeat.tick() => {
                        if last_received.elapsed() > Duration::from_secs(20) { break; }
                        if socket.send(Message::Ping(Vec::new().into())).await.is_err() { break; }
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
            let _ = socket.close(None).await;
        });
        Ok(Self { commands })
    }
    async fn request(&self, request: Value) -> Result<Value> {
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
        mpai::RuntimeIdentity,
        Arc<AtomicUsize>,
        Arc<AtomicUsize>,
        tokio::task::JoinHandle<()>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let target = mpai::Runtime::default().identity().await.unwrap();
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
                                    json!({"name":"mpai","protocolVersion":protocol::PROTOCOL_VERSION,"runtimeIdentity":t})
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
}
