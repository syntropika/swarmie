use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
};

struct Daemon {
    child: Child,
    origin: String,
    client: reqwest::Client,
    assets: bool,
}
impl Daemon {
    async fn start(root: &Path, assets: bool, websocket: bool) -> Self {
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(root.join("index.html"), "<div>Node fixture</div>").unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_mpai"));
        command.args([
            "--state-dir",
            root.join("state").to_str().unwrap(),
            "serve",
            if websocket { "--websocket" } else { "--http" },
            "127.0.0.1:0",
            "--token",
            "node-fixture-secret",
            "--workbench",
        ]);
        if assets {
            command.args(["--desktop-assets", root.to_str().unwrap()]);
        }
        let mut child = command
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut lines = BufReader::new(child.stderr.take().unwrap()).lines();
        let endpoint = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let line = lines
                    .next_line()
                    .await
                    .unwrap()
                    .expect("daemon exited before listening");
                if let Some(url) = line.strip_prefix(if websocket {
                    "WebSocket listening on "
                } else {
                    "HTTP listening on "
                }) {
                    break url.to_string();
                }
            }
        })
        .await
        .unwrap();
        Self {
            child,
            assets,
            origin: endpoint.trim_end_matches("/rpc").to_string(),
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap(),
        }
    }
    async fn request(&self, path: &str, request: Value) -> Value {
        let builder = self.client.post(format!("{}{path}", self.origin));
        let builder = if self.assets {
            builder.header("origin", &self.origin)
        } else {
            builder
        };
        builder
            .bearer_auth("node-fixture-secret")
            .json(&request)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap()
    }
    async fn rpc(&self, path: &str, method: &str, params: Value, target: Option<&Value>) -> Value {
        let mut request = json!({"jsonrpc":"2.0","id":"client-id","method":method,"params":params});
        if let Some(target) = target {
            request["expectedRuntime"] = target.clone();
        }
        self.request(path, request).await
    }
    async fn initialize(&self, path: &str) -> Value {
        self.rpc(path, "runtime/initialize", json!({}), None).await["result"]["runtimeIdentity"]
            .clone()
    }
    async fn invoke(&self, path: &str, command: &str, args: Value, target: &Value) -> Value {
        result(
            self.rpc(
                path,
                "extension/invoke",
                json!({"extensionId":"workspace","command":command,"arguments":args}),
                Some(target),
            )
            .await,
        )
    }
}
fn result(response: Value) -> Value {
    assert!(response.get("error").is_none(), "{response}");
    response["result"].clone()
}
async fn pair(gateway: &Daemon, target: &Value, name: &str, endpoint: &str) -> Value {
    result(
        gateway
            .rpc(
                "/client/rpc",
                "nodes/pair",
                json!({"name":name,"url":endpoint,"token":"node-fixture-secret"}),
                Some(target),
            )
            .await,
    )
}

#[tokio::test]
async fn gateway_routes_real_files_terminals_previews_and_retains_remote_execution_on_removal() {
    let dir = tempfile::tempdir().unwrap();
    let gateway = Daemon::start(&dir.path().join("client"), true, false).await;
    let remote = Daemon::start(&dir.path().join("remote"), false, false).await;
    let local_target = gateway.initialize("/rpc").await;
    let route = pair(
        &gateway,
        &local_target,
        "remote",
        &format!("{}/rpc", remote.origin),
    )
    .await;
    pair(
        &gateway,
        &local_target,
        "second",
        &format!("{}/rpc", remote.origin),
    )
    .await;
    let env = route["identity"]["environmentId"].as_str().unwrap();
    let path = format!("/nodes/{env}/rpc");
    let target = gateway.initialize(&path).await;
    assert_eq!(target, remote.initialize("/rpc").await);
    let catalog = result(
        gateway
            .rpc("/client/rpc", "nodes/list", json!({}), Some(&local_target))
            .await,
    );
    assert_eq!(catalog["nodes"].as_array().unwrap().len(), 2);
    let node = catalog["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["identity"]["environmentId"] == env)
        .unwrap();
    assert_eq!(node["connections"].as_array().unwrap().len(), 2);
    assert_eq!(node["connections"][0]["authenticated"], true);
    assert!(!catalog.to_string().contains("credentialRef"));
    assert!(!catalog.to_string().contains("node-fixture-secret"));
    result(
        gateway
            .rpc(
                "/client/rpc",
                "nodes/select",
                json!({"environmentId":env}),
                Some(&local_target),
            )
            .await,
    );
    assert_eq!(
        result(
            gateway
                .rpc("/client/rpc", "nodes/list", json!({}), Some(&local_target))
                .await
        )["selectedNodeId"],
        env
    );
    for wrong in [
        &local_target,
        &json!({"identity":target["identity"],"incarnationId":"old"}),
    ] {
        let rejected = gateway
            .rpc(
                &path,
                "project/create",
                json!({"name":"Wrong node"}),
                Some(wrong),
            )
            .await;
        assert_eq!(rejected["error"]["code"], -32005);
        assert_eq!(rejected["error"]["data"]["effectKnowledge"], "none");
    }
    let missing = gateway
        .rpc(&path, "project/create", json!({"name":"No target"}), None)
        .await;
    assert_eq!(missing["error"]["data"]["effectKnowledge"], "none");
    result(
        gateway
            .rpc(
                &path,
                "project/create",
                json!({"name":"Destination project"}),
                Some(&target),
            )
            .await,
    );
    assert_eq!(
        result(
            remote
                .rpc("/rpc", "project/list", json!({}), Some(&target))
                .await
        )
        .as_array()
        .unwrap()
        .len(),
        1
    );
    assert_eq!(
        result(
            gateway
                .rpc("/rpc", "project/list", json!({}), Some(&local_target))
                .await
        ),
        json!([])
    );
    let native_error = remote
        .rpc("/rpc", "unknown/action", json!({}), Some(&target))
        .await;
    assert_eq!(
        gateway
            .rpc(&path, "unknown/action", json!({}), Some(&target))
            .await,
        native_error
    );

    let folder = dir.path().join("working");
    std::fs::create_dir_all(folder.join("docs")).unwrap();
    std::fs::write(
        folder.join("index.html"),
        "<h1>Remote preview</h1><script src='/app.js'></script>",
    )
    .unwrap();
    std::fs::write(folder.join("app.js"), "fetch('/data.json')").unwrap();
    std::fs::write(folder.join("docs/index.html"), "Nested").unwrap();
    let project = gateway
        .invoke(&path, "project.open", json!({"path":folder}), &target)
        .await;
    let asset = gateway
        .invoke(
            &path,
            "assets.write",
            json!({"projectId":project["id"],"name":"remote.txt","data":"cmVtb3Rl"}),
            &target,
        )
        .await;
    assert_eq!(
        gateway
            .invoke(
                &path,
                "assets.read",
                json!({"assetId":asset["assetId"]}),
                &target
            )
            .await["data"],
        "cmVtb3Rl"
    );
    let local_asset = gateway.rpc("/rpc", "extension/invoke", json!({"extensionId":"workspace","command":"assets.read","arguments":{"assetId":asset["assetId"]}}), Some(&local_target)).await;
    assert!(local_asset.get("error").is_some());
    #[cfg(unix)]
    {
        let terminal = gateway.invoke(&path, "terminal.start", json!({"projectId":project["id"],"program":"/bin/sh","args":["-c","printf 'REMOTE_TERMINAL'; printf 'destination' > marker.txt"]}), &target).await;
        let mut text = String::new();
        let mut after = 0;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let read = gateway
                    .invoke(
                        &path,
                        "terminal.read",
                        json!({"terminalId":terminal["terminalId"],"after":after}),
                        &target,
                    )
                    .await;
                text.push_str(read["text"].as_str().unwrap());
                after = read["next"].as_u64().unwrap();
                if read["exited"] == true {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert!(text.contains("REMOTE_TERMINAL"));
        assert_eq!(
            std::fs::read_to_string(folder.join("marker.txt")).unwrap(),
            "destination"
        );
        gateway
            .invoke(
                &path,
                "terminal.close",
                json!({"terminalId":terminal["terminalId"]}),
                &target,
            )
            .await;
    }
    let preview = gateway
        .invoke(
            &path,
            "preview.start",
            json!({"projectId":project["id"]}),
            &target,
        )
        .await;
    let preview_id = preview["previewId"].as_str().unwrap();
    let prefix = format!("/nodes/{env}/preview/{preview_id}/");
    let response = gateway
        .client
        .get(format!("{}{prefix}", gateway.origin))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(
        response.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains(&prefix)
    );
    let html = response.text().await.unwrap();
    assert!(html.contains("Remote preview"));
    assert!(html.contains(&format!("{prefix}app.js")));
    let js = gateway
        .client
        .get(format!("{}{prefix}app.js", gateway.origin))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(js.contains(&format!("{prefix}data.json")));
    let redirect = gateway
        .client
        .get(format!("{}{prefix}docs", gateway.origin))
        .send()
        .await
        .unwrap();
    assert_eq!(redirect.headers()["location"], format!("{prefix}docs/"));
    assert_eq!(
        gateway
            .client
            .get(format!("{}/preview/{preview_id}/", gateway.origin))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    let stale = gateway
        .rpc(
            "/client/rpc",
            "nodes/removeRoute",
            json!({"name":"remote","revision":route["revision"].as_u64().unwrap()+100}),
            Some(&local_target),
        )
        .await;
    assert!(stale.get("error").is_some());
    for connection in node["connections"].as_array().unwrap() {
        result(
            gateway
                .rpc(
                    "/client/rpc",
                    "nodes/removeRoute",
                    json!({"name":connection["name"],"revision":connection["revision"]}),
                    Some(&local_target),
                )
                .await,
        );
    }
    let removed = result(
        gateway
            .rpc("/client/rpc", "nodes/list", json!({}), Some(&local_target))
            .await,
    );
    assert_eq!(removed["selectedNodeId"], env);
    assert_eq!(
        removed["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["identity"]["environmentId"] == env)
            .unwrap()["registered"],
        false
    );
    assert_eq!(
        gateway
            .rpc(&path, "project/list", json!({}), Some(&target))
            .await["error"]["data"]["effectKnowledge"],
        "none"
    );
    assert_eq!(
        result(
            remote
                .rpc("/rpc", "project/list", json!({}), Some(&target))
                .await
        )
        .as_array()
        .unwrap()
        .len(),
        1
    );
    assert_eq!(
        remote
            .client
            .get(format!("{}/preview/{preview_id}/", remote.origin))
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
}

#[tokio::test]
async fn gateway_requires_local_auth_origin_host_and_exact_target_and_handles_websocket_routes() {
    let dir = tempfile::tempdir().unwrap();
    let gateway = Daemon::start(&dir.path().join("client"), true, false).await;
    let mut remote = Daemon::start(&dir.path().join("remote"), false, true).await;
    let target = gateway.initialize("/rpc").await;
    let route = pair(&gateway, &target, "socket", &remote.origin).await;
    let path = format!(
        "/nodes/{}/rpc",
        route["identity"]["environmentId"].as_str().unwrap()
    );
    let remote_target = gateway.initialize(&path).await;
    assert_eq!(
        result(
            gateway
                .rpc(&path, "project/list", json!({}), Some(&remote_target))
                .await
        ),
        json!([])
    );
    for endpoint in ["/client/rpc", path.as_str()] {
        let request =
            json!({"jsonrpc":"2.0","id":1,"method":"nodes/list","expectedRuntime":target});
        assert_eq!(
            gateway
                .client
                .post(format!("{}{endpoint}", gateway.origin))
                .json(&request)
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
        for origin in ["null", "https://foreign.invalid"] {
            assert_eq!(
                gateway
                    .client
                    .post(format!("{}{endpoint}", gateway.origin))
                    .bearer_auth("node-fixture-secret")
                    .header("origin", origin)
                    .json(&request)
                    .send()
                    .await
                    .unwrap()
                    .status(),
                403
            );
        }
        assert_eq!(
            gateway
                .client
                .post(format!("{}{endpoint}", gateway.origin))
                .bearer_auth("node-fixture-secret")
                .header("host", "foreign.invalid")
                .json(&request)
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
    }
    assert_eq!(
        gateway
            .rpc("/client/rpc", "nodes/list", json!({}), Some(&remote_target))
            .await["error"]["code"],
        -32005
    );
    let duplicate = gateway
        .rpc(
            "/client/rpc",
            "nodes/pair",
            json!({"name":"socket","url":remote.origin,"token":"node-fixture-secret"}),
            Some(&target),
        )
        .await;
    assert!(duplicate.get("error").is_some());
    remote.child.kill().await.unwrap();
    remote.child.wait().await.unwrap();
    let lost = gateway
        .rpc(
            &path,
            "session/prompt",
            json!({"sessionId":"unknown","text":"do not retry"}),
            Some(&remote_target),
        )
        .await;
    assert_eq!(lost["error"]["code"], -32006);
    assert_eq!(lost["error"]["data"]["effectKnowledge"], "unknown");
    let bare = Daemon::start(&dir.path().join("bare"), false, false).await;
    let request = json!({"jsonrpc":"2.0","id":1,"method":"nodes/list"});
    assert_eq!(
        bare.client
            .post(format!("{}/client/rpc", bare.origin))
            .bearer_auth("node-fixture-secret")
            .json(&request)
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
}

#[tokio::test]
async fn gateway_preserves_native_error_data_and_uncertain_delivery_without_repeating_effects() {
    use axum::{Json, extract::State, response::IntoResponse};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    type Peer = (Arc<tokio::sync::Mutex<Value>>, Arc<AtomicUsize>);
    async fn peer(
        State((target, effects)): State<Peer>,
        Json(request): Json<Value>,
    ) -> axum::response::Response {
        let target = target.lock().await.clone();
        match request["method"].as_str().unwrap() {
            "runtime/initialize" => Json(json!({"jsonrpc":"2.0","id":request["id"],"result":{"name":"mpai","protocolVersion":"1","runtimeIdentity":target}})).into_response(),
            "session/prompt" => {
                effects.fetch_add(1,Ordering::SeqCst);
                // The effect has happened, but the acknowledgment is unusable.
                (axum::http::StatusCode::OK,"truncated acknowledgment").into_response()
            }
            _ => Json(json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32800,"message":"native cancellation","data":{"effectKnowledge":"accepted","provider":"fixture"}}})).into_response(),
        }
    }
    async fn peer_websocket(
        State(state): State<Peer>,
        upgrade: axum::extract::ws::WebSocketUpgrade,
    ) -> axum::response::Response {
        upgrade.on_upgrade(move |mut socket| async move {
            use axum::extract::ws::Message;
            while let Some(Ok(frame)) = socket.recv().await {
                match frame {
                    Message::Text(text) => {
                        let request: Value = serde_json::from_str(&text).unwrap();
                        let target = state.0.lock().await.clone();
                        let response = match request["method"].as_str().unwrap() {
                            "runtime/initialize" => json!({"jsonrpc":"2.0","id":request["id"],"result":{"name":"mpai","protocolVersion":"1","runtimeIdentity":target}}),
                            "session/prompt" => { state.1.fetch_add(1, Ordering::SeqCst); let _ = socket.send(Message::Text("truncated acknowledgment".into())).await; break; },
                            _ => json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32800,"message":"native cancellation","data":{"effectKnowledge":"accepted","provider":"fixture"}}}),
                        };
                        if socket.send(Message::Text(response.to_string().into())).await.is_err() { break; }
                    },
                    Message::Ping(data) => { if socket.send(Message::Pong(data)).await.is_err() { break; } },
                    _ => break,
                }
            }
        }).into_response()
    }
    let target = Arc::new(tokio::sync::Mutex::new(
        json!({"identity":{"environmentId":"11111111-1111-4111-8111-111111111111","storeId":"22222222-2222-4222-8222-222222222222","ownerScopeId":"33333333-3333-4333-8333-333333333333"},"incarnationId":"first"}),
    ));
    let effects = Arc::new(AtomicUsize::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/rpc", listener.local_addr().unwrap());
    let app = axum::Router::new()
        .route("/rpc", axum::routing::post(peer))
        .route("/ws", axum::routing::get(peer_websocket))
        .with_state((target.clone(), effects.clone()));
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let dir = tempfile::tempdir().unwrap();
    let gateway = Daemon::start(dir.path(), true, false).await;
    let local = gateway.initialize("/rpc").await;
    pair(&gateway, &local, "fixture", &endpoint).await;
    let path = "/nodes/11111111-1111-4111-8111-111111111111/rpc";
    let observed = gateway.initialize(path).await;
    let response = gateway
        .rpc(path, "session/cancel", json!({}), Some(&observed))
        .await;
    assert_eq!(response["id"], "client-id");
    assert_eq!(response["error"]["code"], -32800);
    assert_eq!(
        response["error"]["data"],
        json!({"effectKnowledge":"accepted","provider":"fixture"})
    );
    let lost = gateway
        .rpc(
            path,
            "session/prompt",
            json!({"text":"one effect"}),
            Some(&observed),
        )
        .await;
    assert_eq!(lost["error"]["code"], -32006);
    assert_eq!(lost["error"]["data"]["effectKnowledge"], "unknown");
    assert_eq!(effects.load(Ordering::SeqCst), 1);
    target.lock().await["identity"]["storeId"] = json!("44444444-4444-4444-8444-444444444444");
    let blocked = gateway
        .rpc(
            path,
            "session/prompt",
            json!({"text":"must not happen"}),
            Some(&observed),
        )
        .await;
    assert_eq!(blocked["error"]["code"], -32005);
    assert_eq!(blocked["error"]["data"]["effectKnowledge"], "none");
    assert_eq!(effects.load(Ordering::SeqCst), 1);
    server.abort();
}
