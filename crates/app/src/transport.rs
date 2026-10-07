use futures_util::{SinkExt, StreamExt};
use mpai::{Error, Result};
use protocol::{RuntimeService, notification, rpc_error};
use serde_json::{Value, json};
use std::net::SocketAddr;
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

fn parse(text: &str) -> std::result::Result<Value, Value> {
    serde_json::from_str(text).map_err(|_| rpc_error(Value::Null, -32700, "parse error"))
}

#[derive(Clone)]
struct HttpState {
    service: RuntimeService,
    token: Option<String>,
    desktop_origin: Option<String>,
}

pub async fn http(
    service: RuntimeService,
    addr: SocketAddr,
    token: Option<String>,
    desktop_assets: Option<std::path::PathBuf>,
    public_origin: Option<String>,
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
    eprintln!("HTTP listening on http://{}/rpc", listener.local_addr()?);
    let origin = format!("http://{}", listener.local_addr()?);
    let mut routes = axum::Router::new()
        .route("/rpc", axum::routing::post(http_rpc))
        .route("/preview/{id}/", axum::routing::get(http_preview_root))
        .route(
            "/preview/{id}/{*path}",
            axum::routing::get(http_preview_file),
        )
        .layer(axum::extract::DefaultBodyLimit::max(16 * 1024 * 1024))
        .with_state(HttpState {
            service,
            token,
            desktop_origin: desktop_assets
                .as_ref()
                .map(|_| public_origin.unwrap_or(origin)),
        });
    if let Some(path) = desktop_assets {
        routes = routes.fallback_service(tower_http::services::ServeDir::new(path));
    }
    axum::serve(listener, routes)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
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
    http_preview(state.0, headers, id, String::new()).await
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
    http_preview(state.0, headers, id, raw).await
}
async fn http_preview(
    state: HttpState,
    headers: axum::http::HeaderMap,
    id: String,
    path: String,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    use base64::{Engine, engine::general_purpose::STANDARD};
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
    let prefix = format!("/preview/{id}/");
    let value = match tokio::time::timeout(
        std::time::Duration::from_secs(3),
        state.service.runtime().invoke_extension(
            "workspace",
            "preview.read",
            json!({"previewId":id,"path":path}),
        ),
    )
    .await
    {
        Ok(Ok(value)) => value,
        Ok(Err(_)) => return (StatusCode::NOT_FOUND, "Unknown preview or file").into_response(),
        Err(_) => return StatusCode::GATEWAY_TIMEOUT.into_response(),
    };
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
    let body = match STANDARD.decode(encoded) {
        Ok(body) if body.len() <= 8 * 1024 * 1024 => body,
        _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
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
        if !location.starts_with(&prefix) || location.contains(['\r', '\n']) {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        response = response.header("location", location);
    }
    response
        .body(axum::body::Body::from(body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

async fn http_rpc(
    axum::extract::State(state): axum::extract::State<HttpState>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    // Desktop web content is allowed only from the explicitly enabled local frontend.
    if let Some(origin) = headers.get("origin")
        && origin.to_str().ok() != state.desktop_origin.as_deref()
    {
        return (StatusCode::FORBIDDEN, "Origin is not allowed").into_response();
    }
    if let Some(origin) = &state.desktop_origin
        && headers.get("host").and_then(|h| h.to_str().ok())
            != origin
                .strip_prefix("http://")
                .or_else(|| origin.strip_prefix("https://"))
    {
        return (StatusCode::FORBIDDEN, "Host is not allowed").into_response();
    }
    if let Some(token) = &state.token
        && headers.get("authorization").and_then(|h| h.to_str().ok())
            != Some(format!("Bearer {token}").as_str())
    {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }
    let response = match serde_json::from_slice(&body) {
        Ok(request) => state.service.handle(request).await,
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
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            line = lines.next_line() => {
                let Some(line) = line? else { break; };
                let service = service.clone(); let tx = tx.clone();
                tasks.spawn(async move { let result = match parse(&line) { Ok(r) => service.handle(r).await,Err(e) => Some(e) }; if let Some(r) = result { let _ = tx.send(r).await; } });
            }
            _ = &mut shutdown => break,
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
    let shutdown = tokio::signal::ctrl_c();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            socket = listener.accept() => { let (socket,_) = socket?; let service = service.clone(); let token = token.clone(); tasks.spawn(async move { if let Err(e) = connection(socket,service,token).await { tracing::debug!("WebSocket client closed: {e}"); } }); }
            _ = &mut shutdown => break,
            Some(_) = tasks.join_next(), if !tasks.is_empty() => (),
        }
    }
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    service.shutdown().await
}
// Tungstenite requires the concrete HTTP ErrorResponse in its handshake callback.
#[allow(clippy::result_large_err)]
async fn connection(
    socket: tokio::net::TcpStream,
    service: RuntimeService,
    token: Option<String>,
) -> Result<()> {
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
    loop {
        tokio::select! {
            msg = stream.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
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
    writer.abort();
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
