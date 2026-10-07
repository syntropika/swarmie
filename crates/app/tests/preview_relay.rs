use serde_json::{Value, json};
use std::time::Duration;
use tokio::process::{Child, Command};
async fn rpc(client: &reqwest::Client, origin: &str, command: &str, args: Value) -> Value {
    let response:Value=client.post(format!("{origin}/rpc")).bearer_auth("preview-fixture-token")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"extension/invoke","params":{"extensionId":"workspace","command":command,"arguments":args}}))
        .send().await.unwrap().json().await.unwrap();
    assert!(response.get("error").is_none(), "{response}");
    response["result"].clone()
}
async fn stop(child: &mut Child) {
    #[cfg(unix)]
    unsafe {
        libc::kill(child.id().unwrap() as i32, libc::SIGINT);
    }
    #[cfg(not(unix))]
    child.kill().await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .unwrap()
        .unwrap();
}
#[tokio::test]
async fn registered_static_relay_runs_without_bearer_and_isolates_the_runtime_api() {
    let state = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let gui = tempfile::tempdir().unwrap();
    std::fs::write(gui.path().join("index.html"), "Trusted GUI").unwrap();
    std::fs::create_dir(project.path().join("assets")).unwrap();
    std::fs::write(
        project.path().join("index.html"),
        r#"<script type="module" src="/assets/app.js"></script><img src="./relative.svg">"#,
    )
    .unwrap();
    std::fs::write(
        project.path().join("assets/app.js"),
        r#"document.body.textContent='Actual JavaScript'; fetch('/data.json');"#,
    )
    .unwrap();
    std::fs::write(
        project.path().join("data.json"),
        r#"{"message":"Scoped fetch"}"#,
    )
    .unwrap();
    std::fs::write(project.path().join(".private"), "never expose").unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let origin = format!("http://127.0.0.1:{port}");
    let mut child = Command::new(env!("CARGO_BIN_EXE_mpai"))
        .args([
            "--state-dir",
            state.path().to_str().unwrap(),
            "serve",
            "--http",
            &format!("127.0.0.1:{port}"),
            "--token",
            "preview-fixture-token",
            "--desktop-assets",
            gui.path().to_str().unwrap(),
        ])
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let client = reqwest::Client::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        while client.get(&origin).send().await.is_err() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let opened = rpc(
        &client,
        &origin,
        "project.open",
        json!({"path":project.path()}),
    )
    .await;
    let preview = rpc(
        &client,
        &origin,
        "preview.start",
        json!({"projectId":opened["id"]}),
    )
    .await;
    let prefix = preview["relayPath"].as_str().unwrap();
    let response = client
        .get(format!("{origin}{prefix}"))
        .header("origin", "null")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["access-control-allow-origin"], "null");
    let csp = response.headers()["content-security-policy"]
        .to_str()
        .unwrap();
    assert!(csp.contains(&format!("connect-src {origin}{prefix};")));
    assert!(csp.contains(&format!("script-src 'unsafe-inline' {origin}{prefix};")));
    assert!(csp.contains("sandbox allow-scripts allow-forms"));
    assert!(!csp.contains("allow-same-origin"));
    assert!(!csp.contains("connect-src 'self'"));
    let html = response.text().await.unwrap();
    assert!(html.contains(&format!("src=\"{prefix}assets/app.js\"")));
    assert!(html.contains("./relative.svg"));
    assert!(!html.contains("preview-fixture-token"));
    let javascript = client
        .get(format!("{origin}{prefix}assets/app.js"))
        .header("origin", "null")
        .send()
        .await
        .unwrap();
    assert_eq!(javascript.headers()["content-type"], "text/javascript");
    assert!(
        javascript
            .text()
            .await
            .unwrap()
            .contains(&format!("fetch('{prefix}data.json')"))
    );
    assert_eq!(
        client
            .get(format!("{origin}{prefix}.private"))
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        client
            .get(format!("{origin}/preview/{}/", uuid::Uuid::new_v4()))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    assert_eq!(
        client
            .post(format!("{origin}/rpc"))
            .header("origin", "null")
            .bearer_auth("preview-fixture-token")
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"session/list"}))
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        client
            .post(format!("{origin}{prefix}data.json"))
            .send()
            .await
            .unwrap()
            .status(),
        405
    );
    rpc(
        &client,
        &origin,
        "preview.stop",
        json!({"previewId":preview["previewId"]}),
    )
    .await;
    assert_eq!(
        client
            .get(format!("{origin}{prefix}"))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    stop(&mut child).await;
}
