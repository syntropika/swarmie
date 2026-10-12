//! Client lifetime, private native IPC endpoint and browser presentation gateway.
use crate::{node_gateway::NodeGateway, runtime_host::Endpoint};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use mpai::{Error, Result, SessionStore};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tokio::sync::{Mutex, broadcast};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct Host {
    nodes: NodeGateway,
    origin: String,
    token: String,
    transfers: Arc<Mutex<HashMap<String, Transfer>>>,
    observed: Arc<Mutex<broadcast::Receiver<Value>>>,
    watched: Arc<Mutex<HashSet<String>>>,
    cache: storage::SqliteSessionStore,
}
struct Transfer {
    chunks: Vec<String>,
    size: usize,
    created: std::time::Instant,
}
pub struct ClientHost {
    pub url: String,
    pub token: String,
    stop: CancellationToken,
    task: tokio::task::JoinHandle<()>,
    nodes: NodeGateway,
    monitor: tokio::task::JoinHandle<()>,
}
impl ClientHost {
    pub async fn start(
        root: &std::path::Path,
        assets: PathBuf,
        endpoint: &Endpoint,
    ) -> Result<Self> {
        let nodes = NodeGateway::attached(root, endpoint).await?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}", listener.local_addr()?);
        let token = uuid::Uuid::new_v4().to_string();
        let host = Host {
            observed: Arc::new(Mutex::new(nodes.events())),
            nodes: nodes.clone(),
            origin: url.clone(),
            token: token.clone(),
            transfers: Default::default(),
            watched: Default::default(),
            cache: storage::SqliteSessionStore::new(root.join("client")).await?,
        };
        let router = axum::Router::new()
            .route("/ipc", axum::routing::post(ipc))
            .route("/rpc", axum::routing::post(local_rpc))
            .route("/client/rpc", axum::routing::post(control))
            .route("/nodes/{node}/rpc", axum::routing::post(node_rpc))
            .route("/preview/{id}/", axum::routing::get(local_preview_root))
            .route("/preview/{id}/{*path}", axum::routing::get(local_preview))
            .route(
                "/nodes/{node}/preview/{id}/",
                axum::routing::get(node_preview_root),
            )
            .route(
                "/nodes/{node}/preview/{id}/{*path}",
                axum::routing::get(node_preview),
            )
            .fallback_service(tower_http::services::ServeDir::new(assets))
            .layer(axum::extract::DefaultBodyLimit::max(16 * 1024 * 1024))
            .with_state(host.clone());
        let stop = CancellationToken::new();
        let shutdown = stop.clone();
        let task = tokio::spawn(async move {
            if let Err(error) = axum::serve(listener, router)
                .with_graceful_shutdown(shutdown.cancelled_owned())
                .await
            {
                tracing::error!("client gateway: {error}");
            }
        });
        let monitor =
            crate::client_monitor::spawn(host.nodes.clone(), host.watched.clone(), stop.clone());
        Ok(Self {
            url,
            token,
            stop,
            task,
            nodes,
            monitor,
        })
    }
    pub async fn close(mut self) {
        self.stop.cancel();
        self.monitor.abort();
        self.nodes.close().await;
        let _ = tokio::time::timeout(Duration::from_secs(2), &mut self.task).await;
    }
}
impl Drop for ClientHost {
    fn drop(&mut self) {
        self.stop.cancel();
        self.task.abort();
        self.monitor.abort();
    }
}
fn browser_allowed(host: &Host, headers: &HeaderMap) -> bool {
    headers.get("host").and_then(|h| h.to_str().ok()) == host.origin.strip_prefix("http://")
        && headers
            .get("origin")
            .is_none_or(|h| h.to_str().ok() == Some(host.origin.as_str()))
}
async fn dispatch(host: &Host, path: &str, request: Value) -> Result<Value> {
    if request["jsonrpc"] != "2.0"
        || !request["method"].is_string()
        || !(request["id"].is_string() || request["id"].is_number())
    {
        return Err(Error::Invalid("invalid client RPC envelope".into()));
    }
    match path {
        "/rpc" => Ok(host.nodes.forward(host.nodes.local_id(), request).await),
        "/client/rpc" => {
            if request["method"] == "nodes/disconnect" {
                let node = request["params"]["environmentId"]
                    .as_str()
                    .ok_or_else(|| Error::Invalid("environmentId is required".into()))?;
                if node == host.nodes.local_id() {
                    return Err(Error::Invalid(
                        "the local node connects automatically".into(),
                    ));
                }
                host.watched.lock().await.remove(node);
                host.nodes.disconnect(node).await;
                return Ok(json!({"jsonrpc":"2.0","id":request["id"],"result":null}));
            }
            Ok(host.nodes.control(request).await)
        }
        "/client/cache" => cache_request(host, request).await,
        _ => {
            let node = path
                .strip_prefix("/nodes/")
                .and_then(|s| s.strip_suffix("/rpc"))
                .filter(|s| !s.is_empty() && !s.contains('/') && !s.contains('%'))
                .ok_or_else(|| Error::Invalid("invalid client RPC destination".into()))?;
            host.watched.lock().await.insert(node.to_owned());
            Ok(host.nodes.forward(node, request).await)
        }
    }
}
async fn cache_request(host: &Host, request: Value) -> Result<Value> {
    use sha2::{Digest, Sha256};
    let identity: mpai::LocalStoreIdentity =
        serde_json::from_value(request["params"]["node"].clone())?;
    if [
        &identity.environment_id,
        &identity.store_id,
        &identity.owner_scope_id,
    ]
    .iter()
    .any(|id| id.is_empty() || id.len() > 128 || id.chars().any(char::is_control))
    {
        return Err(Error::Invalid("invalid cache node identity".into()));
    }
    let key = format!(
        "workspace:{:x}",
        Sha256::digest(serde_json::to_vec(&identity)?)
    );
    let result = match request["method"].as_str() {
        Some("client/cacheLoad") => host
            .cache
            .extension_state(&key)
            .await?
            .unwrap_or(Value::Null),
        Some("client/cacheSave") => {
            let value = request["params"]["value"].clone();
            if value["node"] != request["params"]["node"] || !value["threads"].is_array() {
                return Err(Error::Invalid(
                    "cache envelope targets a different node".into(),
                ));
            }
            host.cache.save_extension_state(&key, value).await?;
            Value::Null
        }
        _ => return Err(Error::Invalid("unknown cache operation".into())),
    };
    Ok(json!({"jsonrpc":"2.0","id":request["id"],"result":result}))
}
async fn browser_rpc(host: Host, headers: HeaderMap, path: String, request: Value) -> Response {
    if !browser_allowed(&host, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match dispatch(&host, &path, request).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    }
}
async fn local_rpc(
    State(host): State<Host>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Response {
    browser_rpc(host, headers, "/rpc".into(), request).await
}
async fn control(
    State(host): State<Host>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Response {
    browser_rpc(host, headers, "/client/rpc".into(), request).await
}
async fn node_rpc(
    State(host): State<Host>,
    headers: HeaderMap,
    Path(node): Path<String>,
    Json(request): Json<Value>,
) -> Response {
    browser_rpc(host, headers, format!("/nodes/{node}/rpc"), request).await
}
async fn ipc(State(host): State<Host>, headers: HeaderMap, Json(payload): Json<Value>) -> Response {
    if headers.contains_key("origin") {
        return (
            StatusCode::FORBIDDEN,
            "Native IPC rejects browser-origin requests",
        )
            .into_response();
    }
    if !browser_allowed(&host, &headers) {
        return (
            StatusCode::FORBIDDEN,
            "Native IPC requires its exact local Host header",
        )
            .into_response();
    }
    if headers.get("authorization").and_then(|h| h.to_str().ok())
        != Some(format!("Bearer {}", host.token).as_str())
    {
        return (
            StatusCode::FORBIDDEN,
            "Native IPC requires the client credential",
        )
            .into_response();
    }
    match transfer(&host, payload).await {
        Ok(value) => Json(value).into_response(),
        Err(error) => (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    }
}
async fn transfer(host: &Host, payload: Value) -> Result<Value> {
    let operation = payload["operation"].as_str().unwrap_or("");
    if operation == "observe" {
        let mut receiver = host.observed.lock().await;
        let event = tokio::time::timeout(Duration::from_secs(1), receiver.recv()).await;
        let mut batch = Vec::new();
        match event {
            Ok(Ok(value)) => batch.push(value),
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => {
                batch.push(json!({"method":"client/reconcile"}))
            }
            _ => (),
        }
        while batch.len() < 128 {
            match receiver.try_recv() {
                Ok(value) => batch.push(value),
                Err(broadcast::error::TryRecvError::Lagged(_)) => {
                    batch.push(json!({"method":"client/reconcile"}))
                }
                Err(_) => break,
            }
        }
        return Ok(json!({"events":batch}));
    }
    if matches!(operation, "stage" | "read" | "release" | "dispatch") {
        let mut transfers = host.transfers.lock().await;
        transfers.retain(|_, transfer| transfer.created.elapsed() < Duration::from_secs(180));
        let id = payload["transferId"].as_str().unwrap_or("");
        if operation == "release" {
            transfers.remove(id);
            return Ok(Value::Null);
        }
        if operation == "read" {
            let index = payload["index"]
                .as_u64()
                .ok_or_else(|| Error::Invalid("chunk index is required".into()))?
                as usize;
            let chunk = transfers
                .get(id)
                .and_then(|t| t.chunks.get(index))
                .ok_or_else(|| Error::Invalid("unknown transfer chunk".into()))?;
            return Ok(json!({"chunk":chunk}));
        }
        if operation == "stage" {
            let chunk = payload["chunk"]
                .as_str()
                .filter(|s| s.len() <= 512 * 1024)
                .ok_or_else(|| Error::Invalid("invalid transfer chunk".into()))?;
            let id = if id.is_empty() {
                uuid::Uuid::new_v4().to_string()
            } else {
                id.to_owned()
            };
            let total: usize = transfers.values().map(|t| t.size).sum();
            if total + chunk.len() > 128 * 1024 * 1024
                || (transfers.len() >= 64 && !transfers.contains_key(&id))
            {
                return Err(Error::Invalid("client transfer capacity exceeded".into()));
            }
            if !transfers.contains_key(&id)
                && payload["transferId"]
                    .as_str()
                    .is_some_and(|s| !s.is_empty())
            {
                return Err(Error::Invalid("unknown transfer".into()));
            }
            let transfer = transfers.entry(id.clone()).or_insert_with(|| Transfer {
                chunks: Vec::new(),
                size: 0,
                created: std::time::Instant::now(),
            });
            if transfer.size + chunk.len() > 64 * 1024 * 1024 {
                return Err(Error::Invalid(
                    "request exceeds client transfer limit".into(),
                ));
            }
            let index = payload["index"]
                .as_u64()
                .ok_or_else(|| Error::Invalid("chunk index is required".into()))?
                as usize;
            if index != transfer.chunks.len() {
                return Err(Error::Invalid("out-of-order transfer chunk".into()));
            }
            transfer.size += chunk.len();
            transfer.chunks.push(chunk.into());
            return Ok(json!({"transferId":id}));
        }
        let transfer = transfers
            .remove(id)
            .ok_or_else(|| Error::Invalid("unknown transfer".into()))?;
        drop(transfers);
        let request = serde_json::from_str(&transfer.chunks.concat())?;
        return respond(
            host,
            dispatch(host, payload["path"].as_str().unwrap_or(""), request).await?,
        )
        .await;
    }
    if operation != "rpc" {
        return Err(Error::Invalid("unknown native client operation".into()));
    }
    respond(
        host,
        dispatch(
            host,
            payload["path"].as_str().unwrap_or(""),
            payload["request"].clone(),
        )
        .await?,
    )
    .await
}
async fn respond(host: &Host, response: Value) -> Result<Value> {
    let text = response.to_string();
    if text.len() <= 128 * 1024 {
        return Ok(json!({"response":response}));
    }
    if text.len() > 64 * 1024 * 1024 {
        return Err(Error::Invalid(
            "response exceeds client transfer limit".into(),
        ));
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + 128 * 1024).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        chunks.push(text[start..end].to_owned());
        start = end;
    }
    let count = chunks.len();
    let mut transfers = host.transfers.lock().await;
    transfers.retain(|_, t| t.created.elapsed() < Duration::from_secs(180));
    if transfers.len() >= 64
        || transfers.values().map(|t| t.size).sum::<usize>() + text.len() > 128 * 1024 * 1024
    {
        return Err(Error::Invalid(
            "client transfer capacity exceeded after dispatch; delivery may have completed".into(),
        ));
    }
    let id = uuid::Uuid::new_v4().to_string();
    transfers.insert(
        id.clone(),
        Transfer {
            chunks,
            size: text.len(),
            created: std::time::Instant::now(),
        },
    );
    Ok(json!({"transferId":id,"chunks":count}))
}
async fn preview(
    host: Host,
    headers: HeaderMap,
    node: Option<String>,
    id: String,
    path: String,
) -> Response {
    if !browser_allowed(&host, &headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if uuid::Uuid::parse_str(&id).is_err() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let destination = node.as_deref().unwrap_or(host.nodes.local_id());
    match tokio::time::timeout(
        Duration::from_secs(3),
        host.nodes.preview(destination, &id, &path),
    )
    .await
    {
        Ok(Ok(value)) => {
            crate::transport::preview_response(&host.origin, &id, node.as_deref(), value)
        }
        Ok(Err(_)) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::GATEWAY_TIMEOUT.into_response(),
    }
}
async fn local_preview_root(
    State(host): State<Host>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    preview(host, headers, None, id, String::new()).await
}
async fn local_preview(
    State(host): State<Host>,
    headers: HeaderMap,
    Path((id, path)): Path<(String, String)>,
) -> Response {
    preview(host, headers, None, id, path).await
}
async fn node_preview_root(
    State(host): State<Host>,
    headers: HeaderMap,
    Path((node, id)): Path<(String, String)>,
) -> Response {
    preview(host, headers, Some(node), id, String::new()).await
}
async fn node_preview(
    State(host): State<Host>,
    headers: HeaderMap,
    Path((node, id, path)): Path<(String, String, String)>,
) -> Response {
    preview(host, headers, Some(node), id, path).await
}

#[cfg(test)]
mod tests {
    use super::*;
    async fn fixture(root: &std::path::Path) -> (Endpoint, tokio::task::JoinHandle<Result<()>>) {
        let service = protocol::RuntimeService::new(
            mpai::RuntimeBuilder::default()
                .store(storage::SqliteSessionStore::new(root).await.unwrap())
                .build(),
        );
        let identity = service.runtime().identity().await.unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let task = tokio::spawn(crate::transport::http(
            service, address, None, None, None, None,
        ));
        let url = format!("http://{address}/rpc");
        for _ in 0..100 {
            if crate::node_connection::rpc(&url, "runtime/initialize", json!({}), None)
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        std::fs::write(root.join("index.html"), "<main>Client fixture</main>").unwrap();
        (
            Endpoint {
                url,
                runtime_identity: identity,
                desktop_assets: None,
                authenticated: false,
            },
            task,
        )
    }
    async fn invoke(host: &ClientHost, payload: Value) -> Value {
        reqwest::Client::new()
            .post(format!("{}/ipc", host.url))
            .bearer_auth(&host.token)
            .json(&payload)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }
    async fn rpc(
        host: &ClientHost,
        path: &str,
        method: &str,
        params: Value,
        target: Option<&Value>,
    ) -> Value {
        let mut request =
            json!({"jsonrpc":"2.0","id":"presentation","method":method,"params":params});
        if let Some(target) = target {
            request["expectedRuntime"] = target.clone();
        }
        invoke(
            host,
            json!({"operation":"rpc","path":path,"request":request}),
        )
        .await
    }
    #[tokio::test]
    async fn native_endpoint_guards_credentials_and_targets_and_client_exit_keeps_runtime_alive() {
        let root = tempfile::tempdir().unwrap();
        let (endpoint, runtime) = fixture(root.path()).await;
        let host = ClientHost::start(root.path(), root.path().to_owned(), &endpoint)
            .await
            .unwrap();
        assert_ne!(host.url, endpoint.url.trim_end_matches("/rpc"));
        let payload = json!({"operation":"rpc","path":"/rpc","request":{"jsonrpc":"2.0","id":1,"method":"runtime/initialize","params":{}}});
        let http = reqwest::Client::new();
        for (token, origin, host_header) in [
            ("wrong", None, None),
            (host.token.as_str(), Some("null"), None),
            (host.token.as_str(), None, Some("wrong.test")),
        ] {
            let mut call = http
                .post(format!("{}/ipc", host.url))
                .bearer_auth(token)
                .json(&payload);
            if let Some(origin) = origin {
                call = call.header("origin", origin);
            }
            if let Some(h) = host_header {
                call = call.header("host", h);
            }
            assert_eq!(call.send().await.unwrap().status(), StatusCode::FORBIDDEN);
        }
        let initialized = rpc(&host, "/rpc", "runtime/initialize", json!({}), None).await;
        assert_eq!(
            initialized["response"]["result"]["runtimeIdentity"],
            serde_json::to_value(&endpoint.runtime_identity).unwrap()
        );
        let rejected = rpc(
            &host,
            "/rpc",
            "project/create",
            json!({"name":"Wrong target"}),
            Some(&json!({"identity":endpoint.runtime_identity.identity,"incarnationId":"wrong"})),
        )
        .await;
        assert_eq!(
            rejected["response"]["error"]["data"]["effectKnowledge"],
            "none"
        );
        host.close().await;
        assert!(
            crate::node_connection::rpc(&endpoint.url, "runtime/initialize", json!({}), None)
                .await
                .is_ok()
        );
        runtime.abort();
    }
    #[tokio::test]
    async fn large_unicode_cache_transfers_survive_client_restart_and_are_scoped_to_full_identity()
    {
        let root = tempfile::tempdir().unwrap();
        let (endpoint, runtime) = fixture(root.path()).await;
        let host = ClientHost::start(root.path(), root.path().to_owned(), &endpoint)
            .await
            .unwrap();
        let node = serde_json::to_value(&endpoint.runtime_identity.identity).unwrap();
        let value = json!({"node":node,"threads":[{"id":"draft","draft":"🦀".repeat(100_000)}],"revision":1});
        let request = json!({"jsonrpc":"2.0","id":1,"method":"client/cacheSave","params":{"node":node,"value":value}}).to_string();
        let mut id = String::new();
        let mut start = 0;
        let mut index = 0;
        while start < request.len() {
            let mut end = (start + 100_000).min(request.len());
            while !request.is_char_boundary(end) {
                end -= 1;
            }
            let chunk = invoke(&host,json!({"operation":"stage","transferId":id,"index":index,"chunk":&request[start..end]})).await;
            id = chunk["transferId"].as_str().unwrap().to_owned();
            start = end;
            index += 1;
        }
        assert_eq!(
            invoke(
                &host,
                json!({"operation":"dispatch","transferId":id,"path":"/client/cache"})
            )
            .await["response"]["result"],
            Value::Null
        );
        host.close().await;
        let host = ClientHost::start(root.path(), root.path().to_owned(), &endpoint)
            .await
            .unwrap();
        let reply = rpc(
            &host,
            "/client/cache",
            "client/cacheLoad",
            json!({"node":node}),
            None,
        )
        .await;
        let mut chunks = Vec::new();
        for index in 0..reply["chunks"].as_u64().unwrap() {
            chunks.push(
                invoke(
                    &host,
                    json!({"operation":"read","transferId":reply["transferId"],"index":index}),
                )
                .await["chunk"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
            );
        }
        let response: Value = serde_json::from_str(&chunks.concat()).unwrap();
        assert_eq!(response["result"], value);
        let mut another = node.clone();
        another["storeId"] = "another-store".into();
        assert_eq!(
            rpc(
                &host,
                "/client/cache",
                "client/cacheLoad",
                json!({"node":another}),
                None
            )
            .await["response"]["result"],
            Value::Null
        );
        host.close().await;
        runtime.abort();
    }
}
