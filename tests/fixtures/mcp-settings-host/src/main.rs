//! Browser qualification host. The UI uses the production native FFI and Tembra RPC.
use axum::{
    Json, Router,
    extract::{OriginalUri, State},
    http::{HeaderMap, StatusCode},
    response::{Html, Redirect},
    routing::{get, post},
};
use std::{path::PathBuf, sync::Arc};
use swarmie_desktop_runtime::{DesktopNode, swarmie_desktop_runtime_exchange};

#[derive(Clone)]
struct Application {
    node: Arc<DesktopNode>,
    vite: String,
    origin: String,
}

async fn index(State(app): State<Application>) -> Html<String> {
    Html(format!(
        r#"<!doctype html><html><head><meta name="viewport" content="width=device-width, initial-scale=1"><title>Swarmie</title></head><body><div id="root"></div><script>
window.zero = {{ invoke: async (command, payload) => {{
 if (command !== "swarmie.desktop.runtime") throw new Error("Unsupported fixture bridge command");
 const response = await fetch("/bridge", {{method:"POST",headers:{{"content-type":"application/json"}},body:JSON.stringify(payload)}});
 if (!response.ok) throw new Error("Fixture native bridge failed");
 return await response.json();
}} }};
</script><script type="module">
import RefreshRuntime from "{vite}/@react-refresh";
RefreshRuntime.injectIntoGlobalHook(window); window.$RefreshReg$ = () => {{}}; window.$RefreshSig$ = () => (type) => type; window.__vite_plugin_react_preamble_installed__ = true;
await import("{vite}/src/main.tsx");
</script></body></html>"#,
        vite = app.vite
    ))
}

async fn exchange(
    State(app): State<Application>,
    headers: HeaderMap,
    Json(input): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let origin = headers
        .get("origin")
        .and_then(|value| value.to_str().ok())
        .ok_or(StatusCode::FORBIDDEN)?;
    if origin != app.origin {
        return Err(StatusCode::FORBIDDEN);
    }
    let input = serde_json::to_vec(&input).map_err(|_| StatusCode::BAD_REQUEST)?;
    let mut output = vec![0_u8; 1024 * 1024];
    // The Arc keeps the shared handle alive; buffers are separate owned allocations.
    let length = unsafe {
        swarmie_desktop_runtime_exchange(
            Arc::as_ptr(&app.node),
            input.as_ptr(),
            input.len(),
            output.as_mut_ptr(),
            output.len(),
        )
    };
    if length == 0 {
        return Err(StatusCode::BAD_GATEWAY);
    }
    serde_json::from_slice(&output[..length])
        .map(Json)
        .map_err(|_| StatusCode::BAD_GATEWAY)
}

async fn asset(State(app): State<Application>, OriginalUri(uri): OriginalUri) -> Redirect {
    Redirect::temporary(&format!("{}{uri}", app.vite))
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut args = std::env::args().skip(1);
    let state = PathBuf::from(
        args.next()
            .expect("supply an isolated fixture state directory"),
    );
    let vite = args.next().expect("supply the local Vite origin");
    let node = Arc::new(DesktopNode::open(state).expect("native fixture node"));
    let (server, _stop, _task) = swarmie_fixture_mcp_server::serve_http(Default::default()).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = Application {
        node,
        vite,
        origin: format!("http://{address}"),
    };
    println!(
        "{}",
        serde_json::json!({"url":format!("http://{address}"),"server":server})
    );
    axum::serve(
        listener,
        Router::new()
            .route("/", get(index))
            .route("/bridge", post(exchange))
            .fallback(get(asset))
            .with_state(app),
    )
    .await
    .unwrap();
}
