use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use desktop_extension_fixture::HtmlExtension;
use swarmie::Runtime;
use swarmie_workspace::WorkspaceExtension;
use protocol::RuntimeService;
use std::{net::SocketAddr, path::PathBuf};

#[derive(Clone)]
struct FixtureState {
    service: RuntimeService,
    authority: String,
    origin: String,
    token: Option<String>,
}

async fn rpc(State(state): State<FixtureState>, headers: HeaderMap, body: Bytes) -> Response {
    if headers.get("host").and_then(|h| h.to_str().ok()) != Some(&state.authority)
        || headers
            .get("origin")
            .is_some_and(|h| h.to_str().ok() != Some(&state.origin))
    {
        return (StatusCode::FORBIDDEN, "Fixture origin is not allowed").into_response();
    }
    if let Some(token) = &state.token
        && headers.get("authorization").and_then(|h| h.to_str().ok())
            != Some(format!("Bearer {token}").as_str())
    {
        return (StatusCode::UNAUTHORIZED, "Fixture token is required").into_response();
    }
    let response = match serde_json::from_slice(&body) {
        Ok(request) => state.service.handle(request).await,
        Err(_) => Some(protocol::rpc_error(
            serde_json::Value::Null,
            -32700,
            "parse error",
        )),
    };
    match response {
        Some(response) => axum::Json(response).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let assets = PathBuf::from(
        args.next()
            .ok_or("usage: desktop-extension-fixture ASSETS HTML STORAGE [127.0.0.1:PORT]")?,
    );
    let html = PathBuf::from(args.next().ok_or("missing HTML path")?);
    let storage = PathBuf::from(args.next().ok_or("missing storage path")?);
    let address: SocketAddr = args
        .next()
        .unwrap_or_else(|| "127.0.0.1:0".into())
        .parse()?;
    if !address.ip().is_loopback() || args.next().is_some() {
        return Err("use only a loopback listener and the documented arguments".into());
    }
    if !assets.join("index.html").is_file() || !html.is_file() {
        return Err("build the frontend and provide an existing HTML file first".into());
    }
    let runtime = Runtime::builder()
        .extension(WorkspaceExtension::new().with_storage_root(storage))
        .extension(HtmlExtension::new(html))
        .build();
    let service = RuntimeService::new(runtime);
    let listener = tokio::net::TcpListener::bind(address).await?;
    let authority = listener.local_addr()?.to_string();
    let origin = format!("http://{authority}");
    let router = axum::Router::new()
        .route("/rpc", post(rpc))
        .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024))
        .with_state(FixtureState {
            service: service.clone(),
            authority,
            origin: origin.clone(),
            token: std::env::var("SWARMIE_FIXTURE_TOKEN")
                .ok()
                .filter(|value| !value.is_empty()),
        })
        .fallback_service(tower_http::services::ServeDir::new(assets));
    println!("FIXTURE_URL={origin}");
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    service.shutdown().await?;
    Ok(())
}
