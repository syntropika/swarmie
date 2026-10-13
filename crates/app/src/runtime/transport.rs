use futures_util::{SinkExt, StreamExt};
use protocol::{RuntimeService, notification, rpc_error};
use serde_json::{Value, json};
use std::net::SocketAddr;
use swarmie::{Error, Result};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::mpsc,
};
use tokio_tungstenite::{
    accept_hdr_async, connect_async,
    tungstenite::{
        Message,
        client::IntoClientRequest,
        handshake::server::{ErrorResponse, Request, Response},
        http::{HeaderValue, StatusCode},
    },
};

async fn termination() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! { result = tokio::signal::ctrl_c() => result, _ = term.recv() => Ok(()) }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await
    }
}

fn parse(text: &str) -> std::result::Result<Value, Value> {
    serde_json::from_str(text).map_err(|_| rpc_error(Value::Null, -32700, "parse error"))
}

#[derive(Clone)]
struct HttpState {
    service: RuntimeService,
    token: Option<String>,
    desktop_origin: Option<String>,
    nodes: Option<crate::node_gateway::NodeGateway>,
    application: Option<crate::client_operations::ClientOperations>,
}

pub async fn http(
    service: RuntimeService,
    addr: SocketAddr,
    token: Option<String>,
    desktop_assets: Option<std::path::PathBuf>,
    public_origin: Option<String>,
    registration_root: Option<std::path::PathBuf>,
) -> Result<()> {
    if desktop_assets.is_some() && !addr.ip().is_loopback() {
        return Err(Error::Invalid(
            "desktop assets require a loopback listener".into(),
        ));
    }
    let public_origin = validate_public_origin(public_origin.as_deref(), token.as_deref())?;
    if let Some(path) = &desktop_assets
        && !path.join("index.html").is_file()
    {
        return Err(Error::Invalid(
            "desktop assets must contain index.html; build the frontend first".into(),
        ));
    }
    if !addr.ip().is_loopback() && token.as_ref().is_none_or(|t| t.is_empty()) {
        return Err(Error::Invalid("non-loopback HTTP requires --token".into()));
    }
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let nodes = match (&registration_root, &desktop_assets) {
        (Some(root), Some(_)) => {
            Some(crate::node_gateway::NodeGateway::new(root, service.clone()).await?)
        }
        _ => None,
    };
    let application = if let (Some(nodes), Some(root)) = (&nodes, &registration_root) {
        let cache = storage::SqliteSessionStore::new(root.join("client")).await?;
        let submissions = crate::client_submissions::ClientSubmissions::new(nodes.clone(), cache);
        Some(crate::client_operations::ClientOperations::new(
            nodes.clone(),
            submissions,
        ))
    } else {
        None
    };
    #[cfg(unix)]
    let mut local_socket_owner = if let Some(root) = registration_root.as_ref() {
        Some(
            local_socket(
                service.clone(),
                crate::runtime_host::local_socket_path(root)?,
                None,
            )
            .await?,
        )
    } else {
        None
    };
    let _registration = if let Some(root) = registration_root.filter(|_| addr.ip().is_loopback()) {
        Some(
            crate::runtime_host::register(
                &root,
                &service,
                listener.local_addr()?,
                desktop_assets.as_deref(),
                token.is_some(),
            )
            .await?,
        )
    } else {
        None
    };
    eprintln!("HTTP listening on http://{}/rpc", listener.local_addr()?);
    let local = listener.local_addr()?;
    let tool_address = if local.ip().is_unspecified() {
        std::net::SocketAddr::new(
            if local.is_ipv4() {
                std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
            } else {
                std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST)
            },
            local.port(),
        )
    } else {
        local
    };
    service
        .runtime()
        .install_host_tool_transport(swarmie::McpServer {
            name: "swarmie_host".into(),
            transport: swarmie::McpTransport::Stdio {
                command: std::env::current_exe()?,
                args: vec!["tools".into(), format!("http://{tool_address}/tools/rpc")],
                env: Default::default(),
            },
        })?;
    let origin = format!("http://{}", listener.local_addr()?);
    let service_for_shutdown = service.clone();
    let execution_owner = service.clone();
    let mut routes = axum::Router::new()
        .route("/rpc", axum::routing::post(http_rpc))
        .route("/ws", axum::routing::get(http_websocket))
        .route("/client/rpc", axum::routing::post(http_node_control))
        .route(
            "/nodes/{environment}/rpc",
            axum::routing::post(http_node_rpc),
        )
        .route(
            "/nodes/{environment}/preview/{id}/",
            axum::routing::get(http_node_preview_root),
        )
        .route(
            "/nodes/{environment}/preview/{id}/{*path}",
            axum::routing::get(http_node_preview_file),
        )
        .route("/tools/rpc", axum::routing::post(http_tools_rpc))
        .route("/preview/{id}/", axum::routing::get(http_preview_root))
        .route(
            "/preview/{id}/{*path}",
            axum::routing::get(http_preview_file),
        )
        .layer(axum::extract::DefaultBodyLimit::max(16 * 1024 * 1024))
        .with_state(HttpState {
            service,
            token,
            nodes,
            application,
            desktop_origin: desktop_assets
                .as_ref()
                .map(|_| public_origin.unwrap_or(origin)),
        });
    if let Some(path) = desktop_assets {
        routes = routes.fallback_service(tower_http::services::ServeDir::new(path));
    }
    let result = axum::serve(listener, routes)
        .with_graceful_shutdown(async move {
            tokio::select! { _ = termination() => {}, _ = service_for_shutdown.wait_for_shutdown() => {} }
        })
        .await;
    // A process signal must also notify socket listeners. Drain accepted socket
    // requests before closing the shared execution owner.
    execution_owner
        .invoke("runtime/shutdown", json!({}))
        .await?;
    #[cfg(unix)]
    if let Some(socket) = local_socket_owner.as_mut() {
        socket.finish().await?;
    }
    execution_owner.shutdown().await?;
    result?;
    Ok(())
}
fn validate_public_origin(origin: Option<&str>, token: Option<&str>) -> Result<Option<String>> {
    let Some(origin) = origin else {
        return Ok(None);
    };
    if token.is_none_or(str::is_empty) {
        return Err(Error::Invalid(
            "public browser access requires a bearer token".into(),
        ));
    }
    let parsed = reqwest::Url::parse(origin)
        .map_err(|_| Error::Invalid("browser origin must be HTTPS or loopback HTTP".into()))?;
    let loopback = parsed.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    });
    if !(parsed.scheme() == "https" || parsed.scheme() == "http" && loopback)
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.path() != "/"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(Error::Invalid(
            "browser origin must contain only HTTPS or loopback HTTP, a host and optional port"
                .into(),
        ));
    }
    Ok(Some(parsed.origin().ascii_serialization()))
}

async fn http_preview_root(
    state: axum::extract::State<HttpState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    http_preview(state.0, headers, id, String::new(), None).await
}
async fn http_preview_file(
    state: axum::extract::State<HttpState>,
    axum::extract::Path((id, path)): axum::extract::Path<(String, String)>,
    headers: axum::http::HeaderMap,
    uri: axum::http::Uri,
) -> axum::response::Response {
    // Preserve encoded path bytes: the extension performs one bounded decoding pass.
    let raw = uri
        .path()
        .strip_prefix(&format!("/preview/{id}/"))
        .unwrap_or(&path)
        .to_owned();
    http_preview(state.0, headers, id, raw, None).await
}
async fn http_preview(
    state: HttpState,
    headers: axum::http::HeaderMap,
    id: String,
    path: String,
    environment: Option<String>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if uuid::Uuid::parse_str(&id).is_err() {
        return (StatusCode::NOT_FOUND, "Unknown preview").into_response();
    }
    let host = headers
        .get("host")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let origin = state
        .desktop_origin
        .clone()
        .unwrap_or_else(|| format!("http://{host}"));
    let Ok(parsed) = reqwest::Url::parse(&origin) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if parsed.path() != "/"
        || parsed.host_str().is_none()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if state.desktop_origin.is_some()
        && host
            != origin
                .strip_prefix("http://")
                .or_else(|| origin.strip_prefix("https://"))
                .unwrap_or("")
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let origin = parsed.origin().ascii_serialization();
    let value = match tokio::time::timeout(std::time::Duration::from_secs(3), async {
        match environment.as_deref() {
            Some(node) => match &state.nodes {
                Some(gateway) => gateway.preview(node, &id, &path).await,
                None => Err(Error::Invalid("node gateway is unavailable".into())),
            },
            None => {
                state
                    .service
                    .runtime()
                    .invoke_extension(
                        "workspace",
                        "preview.read",
                        json!({"previewId":id,"path":path}),
                    )
                    .await
            }
        }
    })
    .await
    {
        Ok(Ok(value)) => value,
        Ok(Err(_)) => return (StatusCode::NOT_FOUND, "Unknown preview or file").into_response(),
        Err(_) => return StatusCode::GATEWAY_TIMEOUT.into_response(),
    };
    preview_response(&origin, &id, environment.as_deref(), value)
}
pub(crate) fn preview_response(
    origin: &str,
    id: &str,
    environment: Option<&str>,
    value: Value,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    use base64::{Engine, engine::general_purpose::STANDARD};
    let prefix = environment.map_or_else(
        || format!("/preview/{id}/"),
        |node| format!("/nodes/{node}/preview/{id}/"),
    );
    let status = value["status"]
        .as_u64()
        .and_then(|status| u16::try_from(status).ok())
        .and_then(|status| StatusCode::from_u16(status).ok())
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mime = value["mime"].as_str().unwrap_or("application/octet-stream");
    let encoded = value["data"].as_str().unwrap_or("");
    if encoded.len() > 12 * 1024 * 1024 {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }
    let mut body = match STANDARD.decode(encoded) {
        Ok(body) if body.len() <= 8 * 1024 * 1024 => body,
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    // Remote relays already scope root URLs; carry that scope through the local gateway.
    if environment.is_some()
        && matches!(
            mime,
            "text/html" | "text/css" | "text/javascript" | "application/javascript"
        )
    {
        let Ok(text) = std::str::from_utf8(&body) else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        body = text
            .replace(&format!("/preview/{id}/"), &prefix)
            .into_bytes();
        if body.len() > 8 * 1024 * 1024 {
            return StatusCode::PAYLOAD_TOO_LARGE.into_response();
        }
    }
    let scope = format!("{origin}{prefix}");
    let policy = format!(
        "default-src 'none'; script-src 'unsafe-inline' {scope}; style-src 'unsafe-inline' {scope}; img-src data: blob: {scope}; media-src blob: {scope}; font-src data: {scope}; connect-src {scope}; form-action {scope}; base-uri {scope}; frame-ancestors {origin}; object-src 'none'; frame-src 'none'; sandbox allow-scripts allow-forms"
    );
    let mut response = axum::http::Response::builder()
        .status(status)
        .header("content-type", mime)
        .header("content-security-policy", policy)
        .header("x-content-type-options", "nosniff")
        .header("cache-control", "no-store")
        .header("referrer-policy", "no-referrer")
        .header("access-control-allow-origin", "null")
        .header("access-control-allow-methods", "GET, HEAD");
    if let Some(location) = value["location"].as_str() {
        let source_prefix = format!("/preview/{id}/");
        if !location.starts_with(&source_prefix) || location.contains(['\r', '\n']) {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        response = response.header(
            "location",
            format!("{prefix}{}", &location[source_prefix.len()..]),
        );
    }
    response
        .body(axum::body::Body::from(body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn http_tools_rpc(
    axum::extract::State(state): axum::extract::State<HttpState>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if headers.contains_key("origin") {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(channel) = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
    else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if !state.service.runtime().host_tool_channel(channel) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let request: Value = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(_) => return axum::Json(rpc_error(Value::Null, -32700, "parse error")).into_response(),
    };
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    if request["jsonrpc"] != "2.0"
        || !request["method"].is_string()
        || !(id.is_string() || id.is_number())
    {
        return axum::Json(rpc_error(Value::Null, -32600, "invalid request")).into_response();
    }
    let response = match state
        .service
        .runtime()
        .invoke_host_tool_channel(
            channel,
            request["method"].as_str().unwrap(),
            request.get("params").cloned().unwrap_or_else(|| json!({})),
        )
        .await
    {
        Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
        Err(error) => rpc_error(
            id,
            if matches!(error, Error::Cancelled(_)) {
                -32800
            } else {
                -32001
            },
            &error.to_string(),
        ),
    };
    axum::Json(response).into_response()
}

fn authorize_http(
    state: &HttpState,
    headers: &axum::http::HeaderMap,
) -> std::result::Result<(), (StatusCode, &'static str)> {
    // Desktop web content is allowed only from the explicitly enabled local frontend.
    if let Some(origin) = headers.get("origin")
        && origin.to_str().ok() != state.desktop_origin.as_deref()
    {
        return Err((StatusCode::FORBIDDEN, "Origin is not allowed"));
    }
    if let Some(origin) = &state.desktop_origin
        && headers.get("host").and_then(|h| h.to_str().ok())
            != origin
                .strip_prefix("http://")
                .or_else(|| origin.strip_prefix("https://"))
    {
        return Err((StatusCode::FORBIDDEN, "Host is not allowed"));
    }
    if let Some(token) = &state.token
        && headers.get("authorization").and_then(|h| h.to_str().ok())
            != Some(format!("Bearer {token}").as_str())
    {
        return Err((StatusCode::UNAUTHORIZED, "Unauthorized"));
    }
    Ok(())
}
async fn http_websocket(
    axum::extract::State(state): axum::extract::State<HttpState>,
    headers: axum::http::HeaderMap,
    upgrade: axum::extract::ws::WebSocketUpgrade,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if let Err(response) = authorize_http(&state, &headers) {
        return response.into_response();
    }
    upgrade
        .max_message_size(16 * 1024 * 1024)
        .on_upgrade(move |socket| http_socket(socket, state.service))
        .into_response()
}
async fn http_socket(socket: axum::extract::ws::WebSocket, service: RuntimeService) {
    use axum::extract::ws::Message;
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::channel(256);
    let writer = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            if sink.send(message).await.is_err() {
                break;
            }
        }
    });
    let mut events = service.runtime().events();
    let mut requests = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            _ = service.wait_for_shutdown() => break,
            frame = stream.next() => match frame {
                Some(Ok(Message::Text(text))) => {
                    if requests.len() >= 128 { break; }
                    let service = service.clone(); let tx = tx.clone();
                    requests.spawn(async move { let response = match parse(&text) { Ok(request) => service.handle(request).await, Err(error) => Some(error) }; if let Some(value) = response { let _ = tx.send(Message::Text(value.to_string().into())).await; } });
                }
                Some(Ok(Message::Ping(data))) => { if tx.try_send(Message::Pong(data)).is_err() { break; } }
                Some(Ok(Message::Pong(_))) => (),
                _ => break,
            },
            event = events.recv() => {
                let value = match event { Ok(event) => notification(event), Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => json!({"jsonrpc":"2.0","method":"runtime/lagged","params":{"missed":n,"recovery":"session/events"}}), Err(_) => break };
                if tx.try_send(Message::Text(value.to_string().into())).is_err() { break; }
            },
            Some(_) = requests.join_next(), if !requests.is_empty() => (),
        }
    }
    // Drain accepted native effects even when their controlling client has departed.
    writer.abort();
    while requests.join_next().await.is_some() {}
}

async fn node_request(
    state: HttpState,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
    environment: Option<String>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if let Err(response) = authorize_http(&state, &headers) {
        return response.into_response();
    }
    let Some(nodes) = state.nodes else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let request: Value = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(_) => return axum::Json(rpc_error(Value::Null, -32700, "parse error")).into_response(),
    };
    if request["jsonrpc"] != "2.0"
        || !request["method"].is_string()
        || !(request["id"].is_number() || request["id"].is_string())
    {
        return axum::Json(rpc_error(Value::Null, -32600, "invalid request")).into_response();
    }
    let result = match environment {
        Some(id) => {
            if let Some(application) = state.application {
                application.dispatch(&id, request).await
            } else {
                nodes.forward(&id, request).await
            }
        }
        None => nodes.control(request).await,
    };
    axum::Json(result).into_response()
}
async fn http_node_control(
    axum::extract::State(state): axum::extract::State<HttpState>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    node_request(state, headers, body, None).await
}
async fn http_node_rpc(
    axum::extract::State(state): axum::extract::State<HttpState>,
    axum::extract::Path(environment): axum::extract::Path<String>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    node_request(state, headers, body, Some(environment)).await
}
async fn http_node_preview_root(
    axum::extract::State(state): axum::extract::State<HttpState>,
    axum::extract::Path((environment, id)): axum::extract::Path<(String, String)>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    http_preview(state, headers, id, String::new(), Some(environment)).await
}
async fn http_node_preview_file(
    axum::extract::State(state): axum::extract::State<HttpState>,
    axum::extract::Path((environment, id, path)): axum::extract::Path<(String, String, String)>,
    headers: axum::http::HeaderMap,
    uri: axum::http::Uri,
) -> axum::response::Response {
    let raw = uri
        .path()
        .strip_prefix(&format!("/nodes/{environment}/preview/{id}/"))
        .unwrap_or(&path)
        .to_owned();
    http_preview(state, headers, id, raw, Some(environment)).await
}
async fn http_rpc(
    axum::extract::State(state): axum::extract::State<HttpState>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if let Err(response) = authorize_http(&state, &headers) {
        return response.into_response();
    }
    let response = match serde_json::from_slice::<Value>(&body) {
        Ok(request) => {
            // Application intents are correlated named-parameter requests.
            // Keep raw runtime batches, notifications and malformed envelopes
            // on the canonical JSON-RPC dispatcher.
            let application_intent = request["jsonrpc"] == "2.0"
                && request.get("id").is_some_and(|id| id.is_null() || id.is_string() || id.is_number())
                && request.get("params").is_none_or(Value::is_object)
                && request["method"].as_str().is_some_and(|method| {
                    crate::client_operations::ClientOperations::handles(method)
                        || matches!(method, "client/submissions" | "client/sessionProjection")
                        || (method == "session/prompt" && serde_json::from_value::<swarmie::RuntimeIdentity>(request["expectedRuntime"].clone()).is_ok())
                });
            if application_intent
                && let (Some(application), Some(nodes)) = (&state.application, &state.nodes)
            {
                return axum::Json(application.dispatch(nodes.local_id(), request).await)
                    .into_response();
            }
            state.service.handle(request).await
        }
        Err(_) => Some(rpc_error(Value::Null, -32700, "parse error")),
    };
    match response {
        Some(response) => axum::Json(response).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}
pub async fn stdio(service: RuntimeService) -> Result<()> {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut events = service.runtime().events();
    let (tx, mut rx) = mpsc::channel::<Value>(256);
    let writer = tokio::spawn(async move {
        let mut out = tokio::io::stdout();
        while let Some(v) = rx.recv().await {
            let bytes = serde_json::to_vec(&v)?;
            out.write_all(&bytes).await?;
            out.write_all(b"\n").await?;
            out.flush().await?;
        }
        Ok::<_, Error>(())
    });
    let event_tx = tx.clone();
    let event_task = tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(e) => {
                    if event_tx.send(notification(e)).await.is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    let _ = event_tx.send(json!({"jsonrpc":"2.0","method":"runtime/lagged","params":{"missed":n,"recovery":"session/events"}})).await;
                }
                Err(_) => break,
            }
        }
    });
    let mut tasks = tokio::task::JoinSet::new();
    let shutdown = termination();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            line = lines.next_line() => {
                let Some(line) = line? else { break; };
                let service = service.clone(); let tx = tx.clone();
                tasks.spawn(async move { let result = match parse(&line) { Ok(r) => service.handle(r).await,Err(e) => Some(e) }; if let Some(r) = result { let _ = tx.send(r).await; } });
            }
            _ = &mut shutdown => break,
            _ = service.wait_for_shutdown() => break,
            Some(_) = tasks.join_next(), if !tasks.is_empty() => (),
        }
    }
    // Finish accepted input before cleaning up; bounded provider deadlines prevent an indefinite drain.
    while tasks.join_next().await.is_some() {}
    service.shutdown().await?;
    event_task.abort();
    drop(tx);
    writer.await.map_err(|e| Error::Provider(e.to_string()))??;
    Ok(())
}
pub async fn websocket(
    service: RuntimeService,
    addr: SocketAddr,
    token: Option<String>,
) -> Result<()> {
    if !addr.ip().is_loopback() && token.as_ref().is_none_or(|t| t.is_empty()) {
        return Err(Error::Invalid(
            "non-loopback WebSocket requires --token".into(),
        ));
    }
    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!("WebSocket listening on ws://{}", listener.local_addr()?);
    let mut tasks = tokio::task::JoinSet::new();
    let shutdown = termination();
    tokio::pin!(shutdown);
    let graceful = loop {
        tokio::select! {
            socket = listener.accept() => { let (socket,_) = socket?; let service = service.clone(); let token = token.clone(); tasks.spawn(async move { if let Err(e) = connection(socket,service,token).await { tracing::debug!("WebSocket client closed: {e}"); } }); }
            _ = &mut shutdown => break false,
            _ = service.wait_for_shutdown() => break true,
            Some(_) = tasks.join_next(), if !tasks.is_empty() => (),
        }
    };
    drop(listener);
    if !graceful {
        tasks.abort_all();
    }
    while tasks.join_next().await.is_some() {}
    service.shutdown().await
}
/// A private socket listener belongs to the runtime, never to an attached client.
#[cfg(unix)]
pub(crate) struct LocalSocket {
    task: Option<tokio::task::JoinHandle<()>>,
}
#[cfg(unix)]
impl LocalSocket {
    async fn finish(&mut self) -> Result<()> {
        if let Some(task) = self.task.take() {
            task.await.map_err(|e| Error::Provider(e.to_string()))?;
        }
        Ok(())
    }
}
#[cfg(unix)]
impl Drop for LocalSocket {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
#[cfg(unix)]
struct SocketFile {
    path: std::path::PathBuf,
    inode: u64,
}
#[cfg(unix)]
impl Drop for SocketFile {
    fn drop(&mut self) {
        use std::os::unix::fs::MetadataExt;
        if std::fs::symlink_metadata(&self.path).is_ok_and(|meta| meta.ino() == self.inode) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
#[cfg(unix)]
pub(crate) async fn local_socket(
    service: RuntimeService,
    path: std::path::PathBuf,
    token: Option<String>,
) -> Result<LocalSocket> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
    let parent = path
        .parent()
        .ok_or_else(|| Error::Invalid("socket path has no parent".into()))?;
    match std::fs::symlink_metadata(parent) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(parent)?;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
        }
        Err(error) => return Err(error.into()),
        Ok(meta)
            if !meta.is_dir()
                || meta.file_type().is_symlink()
                || meta.uid() != unsafe { libc::geteuid() }
                || meta.mode() & 0o077 != 0 =>
        {
            return Err(Error::Invalid(
                "local socket directory must be owned by the current user and private".into(),
            ));
        }
        Ok(_) => (),
    }
    match std::fs::symlink_metadata(&path) {
        Ok(meta) => {
            if !meta.file_type().is_socket() || meta.uid() != unsafe { libc::geteuid() } {
                return Err(Error::Invalid(
                    "refuse to replace a non-owned local socket path".into(),
                ));
            }
            match tokio::net::UnixStream::connect(&path).await {
                Ok(_) => {
                    return Err(Error::Invalid(
                        "local socket already has an active listener".into(),
                    ));
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
                    ) =>
                {
                    std::fs::remove_file(&path)?;
                }
                Err(error) => return Err(error.into()),
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => return Err(error.into()),
    }
    let listener = tokio::net::UnixListener::bind(&path)?;
    let file = SocketFile {
        path: path.clone(),
        inode: std::fs::symlink_metadata(&path)?.ino(),
    };
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    let task = tokio::spawn(async move {
        let _file = file;
        let mut clients = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                _ = service.wait_for_shutdown() => break,
                stream = listener.accept() => {
                    let Ok((stream, _)) = stream else { break; };
                    let service = service.clone(); let token = token.clone();
                    clients.spawn(async move { let _ = connection(stream, service, token).await; });
                }
                Some(_) = clients.join_next(), if !clients.is_empty() => (),
            }
        }
        drop(listener);
        while clients.join_next().await.is_some() {}
    });
    Ok(LocalSocket { task: Some(task) })
}

// Tungstenite requires the concrete HTTP ErrorResponse in its handshake callback.
#[allow(clippy::result_large_err)]
async fn connection<S>(socket: S, service: RuntimeService, token: Option<String>) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let callback =
        move |r: &Request, response: Response| -> std::result::Result<Response, ErrorResponse> {
            if let Some(token) = &token {
                let expected = format!("Bearer {token}");
                if r.headers()
                    .get("authorization")
                    .and_then(|h| h.to_str().ok())
                    != Some(expected.as_str())
                {
                    let mut error = ErrorResponse::new(Some("Unauthorized".into()));
                    *error.status_mut() = StatusCode::UNAUTHORIZED;
                    return Err(error);
                }
            }
            Ok(response)
        };
    let websocket = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        accept_hdr_async(socket, callback),
    )
    .await
    .map_err(|_| Error::Timeout("WebSocket handshake".into()))?
    .map_err(|e| Error::Provider(e.to_string()))?;
    let (mut sink, mut stream) = websocket.split();
    let mut events = service.runtime().events();
    let (tx, mut rx) = mpsc::channel::<Message>(256);
    let writer = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            if sink.send(message).await.is_err() {
                break;
            }
        }
    });
    let mut requests = tokio::task::JoinSet::new();
    let mut stopping = false;
    loop {
        tokio::select! {
            _ = service.wait_for_shutdown() => { stopping = true; break; },
            msg = stream.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if requests.len() >= 128 { break; }
                        let service = service.clone(); let tx = tx.clone();
                        requests.spawn(async move { let result = match parse(&text) { Ok(r) => service.handle(r).await,Err(e) => Some(e) }; if let Some(r) = result { let _ = tx.send(Message::Text(r.to_string().into())).await; } });
                    }
                    Some(Ok(Message::Ping(data))) => { if tx.send(Message::Pong(data)).await.is_err() { break; } }
                    Some(Ok(Message::Pong(_))) => (),
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => { if tx.send(Message::Text(rpc_error(Value::Null,-32600,"text JSON frames required").to_string().into())).await.is_err() { break; } }
                }
            }
            event = events.recv() => {
                let v = match event { Ok(e) => notification(e),Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => json!({"jsonrpc":"2.0","method":"runtime/lagged","params":{"missed":n,"recovery":"session/events"}}),Err(_) => break };
                if tx.try_send(Message::Text(v.to_string().into())).is_err() { break; }
            }
            Some(_) = requests.join_next(), if !requests.is_empty() => (),
        }
    }
    // Accepted RPC mutations survive client disconnect; they must not be canceled halfway through spawn.
    while requests.join_next().await.is_some() {}
    drop(tx);
    if stopping {
        let mut writer = writer;
        if tokio::time::timeout(std::time::Duration::from_secs(1), &mut writer)
            .await
            .is_err()
        {
            writer.abort();
        }
    } else {
        writer.abort();
    }
    Ok(())
}
pub async fn call(url: &str, method: String, params: Value, token: Option<String>) -> Result<()> {
    let mut request = url
        .into_client_request()
        .map_err(|e| Error::Invalid(e.to_string()))?;
    if let Some(token) = token {
        request.headers_mut().insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {token}"))
                .map_err(|e| Error::Invalid(e.to_string()))?,
        );
    }
    let (mut ws, _) = connect_async(request)
        .await
        .map_err(|e| Error::Provider(e.to_string()))?;
    ws.send(Message::Text(
        json!({"jsonrpc":"2.0","id":1,"method":method,"params":params})
            .to_string()
            .into(),
    ))
    .await
    .map_err(|e| Error::Provider(e.to_string()))?;
    tokio::time::timeout(std::time::Duration::from_secs(60), async {
        while let Some(message) = ws.next().await {
            let message = message.map_err(|e| Error::Provider(e.to_string()))?;
            if let Message::Text(t) = message {
                let v: Value = serde_json::from_str(&t)?;
                if v["id"] == 1 {
                    println!("{}", serde_json::to_string_pretty(&v)?);
                    if let Some(e) = v.get("error") {
                        return Err(Error::Provider(e.to_string()));
                    }
                    return Ok(());
                }
            }
        }
        Err(Error::Closed)
    })
    .await
    .map_err(|_| Error::Timeout("daemon RPC".into()))?
}

#[cfg(test)]
mod origin_tests {
    use super::validate_public_origin;
    #[test]
    fn authenticated_tunnels_can_pin_loopback_http_without_permitting_other_plaintext_origins() {
        for origin in [
            "http://127.0.0.1:43210",
            "http://[::1]:43210",
            "http://localhost:43210",
            "https://agents.example.test",
        ] {
            assert_eq!(
                validate_public_origin(Some(origin), Some("fixture-key"))
                    .unwrap()
                    .as_deref(),
                Some(origin)
            );
            assert!(validate_public_origin(Some(origin), None).is_err());
        }
        for origin in [
            "http://192.168.1.1:43210",
            "http://agents.example.test",
            "http://127.0.0.1.example.test",
            "http://user@127.0.0.1",
            "http://127.0.0.1/path",
            "http://127.0.0.1?x=y",
            "http://127.0.0.1#fragment",
        ] {
            assert!(
                validate_public_origin(Some(origin), Some("fixture-key")).is_err(),
                "{origin}"
            );
        }
    }
}
