//! Browser and native adapters share the application workflow implementation.
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

#[tokio::test]
async fn browser_application_retains_authored_input_and_verifies_node_target() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("index.html"), "<html></html>").unwrap();
    let script = root.path().join("script.json");
    std::fs::write(
        &script,
        json!({"turns":[{"steps":[{"type":"end","outcome":"completed"}]}]}).to_string(),
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_swarmie"))
        .args([
            "--state-dir",
            root.path().to_str().unwrap(),
            "serve",
            "--http",
            "127.0.0.1:0",
            "--desktop-assets",
            root.path().to_str().unwrap(),
            "--mock-script",
            script.to_str().unwrap(),
            "--token",
            "test-client-token",
        ])
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stderr.take().unwrap()).lines();
    let url = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            let line = lines.next_line().await.unwrap().unwrap();
            if let Some(url) = line.strip_prefix("HTTP listening on ") {
                break url.trim_end_matches("/rpc").to_owned();
            }
        }
    })
    .await
    .unwrap();
    let client = reqwest::Client::new();
    let request = |method: &str, params: Value, target: Value| {
        let mut request = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
        if !target.is_null() {
            request["expectedRuntime"] = target;
        }
        request
    };
    let rpc = |path: String, body: Value| {
        let client = client.clone();
        let origin = url.clone();
        async move {
            let response = client
                .post(path)
                .bearer_auth("test-client-token")
                .header("origin", origin)
                .json(&body)
                .send()
                .await
                .unwrap();
            assert!(response.status().is_success(), "{}", response.status());
            response.json::<Value>().await.unwrap()
        }
    };
    let initialized = rpc(
        format!("{url}/rpc"),
        request("runtime/initialize", json!({}), Value::Null),
    )
    .await;
    let target = initialized["result"]["runtimeIdentity"].clone();
    let batch = rpc(
        format!("{url}/rpc"),
        json!([
            request("runtime/initialize", json!({}), Value::Null),
            {"jsonrpc":"2.0","method":"runtime/initialize","params":{}}
        ]),
    )
    .await;
    assert_eq!(batch.as_array().unwrap().len(), 1);
    assert_eq!(batch[0]["result"]["runtimeIdentity"], target);
    let notification = client
        .post(format!("{url}/rpc"))
        .bearer_auth("test-client-token")
        .header("origin", &url)
        .json(&json!({"jsonrpc":"2.0","method":"runtime/initialize","params":{}}))
        .send()
        .await
        .unwrap();
    assert_eq!(notification.status(), reqwest::StatusCode::NO_CONTENT);
    let invalid = rpc(format!("{url}/rpc"), json!({"jsonrpc":"invalid","id":1,"method":"client/sessionCreate","params":{"harness":"mock"},"expectedRuntime":target})).await;
    assert_eq!(invalid["error"]["code"], -32600);
    assert_eq!(
        rpc(
            format!("{url}/rpc"),
            request("session/list", json!({}), target.clone())
        )
        .await["result"],
        json!([])
    );
    let selected = rpc(
        format!("{url}/rpc"),
        request(
            "client/chatDirectory",
            json!({"set":true,"path":root.path()}),
            target.clone(),
        ),
    )
    .await;
    assert!(selected.get("error").is_none(), "{selected}");
    let session = rpc(
        format!("{url}/rpc"),
        request(
            "client/sessionCreate",
            json!({"harness":"mock"}),
            target.clone(),
        ),
    )
    .await["result"]
        .clone();
    assert!(session["id"].is_string(), "{session}");
    let route = format!(
        "{url}/nodes/{}/rpc",
        target["identity"]["environmentId"].as_str().unwrap()
    );
    let intent = json!({"sessionId":session["id"],"text":"Browser authored input","idempotencyKey":"browser-input","attachments":[{"id":"file","name":"notes.txt","mime":"text/plain","image":false,"data":"retained notes"}]});
    let mut stale = target.clone();
    stale["incarnationId"] = "old".into();
    let rejected = rpc(
        route.clone(),
        request("client/sessionInput", intent.clone(), stale),
    )
    .await;
    assert_eq!(rejected["error"]["data"]["effectKnowledge"], "none");
    let sent = rpc(
        route.clone(),
        request("client/sessionInput", intent.clone(), target.clone()),
    )
    .await;
    assert_eq!(sent["result"]["receipt"]["delivery"], "accepted", "{sent}");
    let duplicate = rpc(
        format!("{url}/rpc"),
        request("client/sessionInput", intent, target.clone()),
    )
    .await;
    assert_eq!(
        duplicate["result"]["receipt"]["id"],
        sent["result"]["receipt"]["id"]
    );
    let projection = rpc(
        route,
        request(
            "client/sessionProjection",
            json!({"sessionId":session["id"]}),
            target.clone(),
        ),
    )
    .await;
    assert_eq!(
        projection["result"]["entries"][0]["input"]["text"],
        "Browser authored input"
    );
    assert_eq!(
        projection["result"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["type"] == "message.user")
            .count(),
        1
    );
    rpc(
        format!("{url}/rpc"),
        request("runtime/shutdown", json!({}), target),
    )
    .await;
    assert!(
        tokio::time::timeout(std::time::Duration::from_secs(10), child.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
}
