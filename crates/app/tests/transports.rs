use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::HeaderValue},
};
fn spawn_params() -> Value {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    json!({"harness":"codex","cwd":root,"config":{"executable":"python3","args":[root.join("tests/fixtures/agent.py"),"codex"]}})
}
async fn stop(child: &mut Child) {
    #[cfg(unix)]
    unsafe {
        libc::kill(child.id().unwrap() as i32, libc::SIGINT);
    }
    #[cfg(not(unix))]
    {
        child.kill().await.unwrap();
    }
    tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .unwrap()
        .unwrap();
}
#[tokio::test]
async fn stdio_streams_notifications_and_retains_native_resume_across_restart() {
    let dir = tempfile::tempdir().unwrap();
    let mut saved_id = String::new();
    for resume in [false, true] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_mpai"))
            .args(["--state-dir", dir.path().to_str().unwrap(), "serve"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut input = child.stdin.take().unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let (method, params) = if resume {
            ("session/resume", json!({"sessionId":saved_id}))
        } else {
            ("session/create", spawn_params())
        };
        input
            .write_all(
                format!(
                    "{}\n",
                    json!({"jsonrpc":"2.0","id":1,"method":method,"params":params})
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let snapshot = loop {
            let line = tokio::time::timeout(Duration::from_secs(5), lines.next_line())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let v: Value = serde_json::from_str(&line).unwrap();
            if v["id"] == 1 {
                assert!(v.get("error").is_none(), "{v}");
                break v["result"].clone();
            }
        };
        let id = snapshot["id"].as_str().unwrap().to_string();
        if resume {
            assert_eq!(id, saved_id);
        } else {
            saved_id = id.clone();
        }
        input.write_all(format!("{}\n",json!({"jsonrpc":"2.0","id":2,"method":"session/prompt","params":{"sessionId":id,"text":"hello"}})).as_bytes()).await.unwrap();
        let mut text = String::new();
        loop {
            let line = tokio::time::timeout(Duration::from_secs(5), lines.next_line())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let v: Value = serde_json::from_str(&line).unwrap();
            let e = &v["params"]["event"];
            if e["type"] == "text.delta" {
                text.push_str(e["data"]["text"].as_str().unwrap());
            }
            if e["type"] == "turn.ended" {
                break;
            }
        }
        assert!(text.contains("Hello"));
        input.write_all(format!("{}\n",json!({"jsonrpc":"2.0","id":3,"method":"session/close","params":{"sessionId":id}})).as_bytes()).await.unwrap();
        loop {
            let v: Value =
                serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            if v["id"] == 3 {
                assert!(v.get("error").is_none());
                break;
            }
        }
        drop(input);
        assert!(
            tokio::time::timeout(Duration::from_secs(5), child.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
    }
}
#[tokio::test]
async fn websocket_authentication_and_sessions_survive_client_disconnect() {
    let dir = tempfile::tempdir().unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let url = format!("ws://127.0.0.1:{port}");
    let mut child = Command::new(env!("CARGO_BIN_EXE_mpai"))
        .args([
            "--state-dir",
            dir.path().to_str().unwrap(),
            "serve",
            "--websocket",
            &format!("127.0.0.1:{port}"),
            "--token",
            "fixture-secret",
        ])
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let (mut socket, _) = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let mut request = url.as_str().into_client_request().unwrap();
            request.headers_mut().insert(
                "authorization",
                HeaderValue::from_static("Bearer fixture-secret"),
            );
            if let Ok(ws) = connect_async(request).await {
                break ws;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(connect_async(&url).await.is_err());
    socket
        .send(Message::Text(
            json!({"jsonrpc":"2.0","id":1,"method":"session/create","params":spawn_params()})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    let id = loop {
        let m = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if let Message::Text(t) = m {
            let v: Value = serde_json::from_str(&t).unwrap();
            if v["id"] == 1 {
                assert!(v.get("error").is_none());
                break v["result"]["id"].as_str().unwrap().to_string();
            }
        }
    };
    socket.close(None).await.unwrap();
    let mut request = url.as_str().into_client_request().unwrap();
    request.headers_mut().insert(
        "authorization",
        HeaderValue::from_static("Bearer fixture-secret"),
    );
    let (mut socket, _) = connect_async(request).await.unwrap();
    socket.send(Message::Text(json!({"jsonrpc":"2.0","id":2,"method":"session/prompt","params":{"sessionId":id,"text":"hello"}}).to_string().into())).await.unwrap();
    let mut text = String::new();
    loop {
        let m = tokio::time::timeout(Duration::from_secs(5), socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        if let Message::Text(t) = m {
            let v: Value = serde_json::from_str(&t).unwrap();
            let e = &v["params"]["event"];
            if e["type"] == "text.delta" {
                text.push_str(e["data"]["text"].as_str().unwrap());
            }
            if e["type"] == "turn.ended" {
                break;
            }
        }
    }
    assert!(text.contains("Hello"));
    socket
        .send(Message::Text(
            json!({"jsonrpc":"2.0","id":3,"method":"session/close","params":{"sessionId":id}})
                .to_string()
                .into(),
        ))
        .await
        .unwrap();
    loop {
        if let Message::Text(t) = socket.next().await.unwrap().unwrap() {
            let v: Value = serde_json::from_str(&t).unwrap();
            if v["id"] == 3 {
                break;
            }
        }
    }
    socket.close(None).await.unwrap();
    stop(&mut child).await;
}

#[tokio::test]
async fn http_uses_the_shared_dispatcher_authentication_and_paged_session_replay() {
    let dir = tempfile::tempdir().unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let url = format!("http://127.0.0.1:{port}/rpc");
    let mut child = Command::new(env!("CARGO_BIN_EXE_mpai"))
        .args([
            "--state-dir",
            dir.path().to_str().unwrap(),
            "serve",
            "--http",
            &format!("127.0.0.1:{port}"),
            "--token",
            "fixture-secret",
            "--git-worktrees",
            dir.path().join("worktrees").to_str().unwrap(),
        ])
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let client = reqwest::Client::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if client.post(&url).send().await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        client
            .post(&url)
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        client
            .post(&url)
            .bearer_auth("fixture-secret")
            .header("origin", "https://example.test")
            .json(&json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    let call = async |method: &str, params: Value| -> Value {
        let value: Value = client
            .post(&url)
            .bearer_auth("fixture-secret")
            .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(value.get("error").is_none(), "{value}");
        value["result"].clone()
    };
    assert_eq!(call("extension/list", json!({})).await[0]["id"], "git");
    let snapshot = call("session/create", spawn_params()).await;
    let id = snapshot["id"].clone();
    call("session/prompt", json!({"sessionId":id,"text":"hello"})).await;
    let mut after = 0;
    let mut finished = false;
    let mut text = String::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !finished {
            let page = call(
                "session/events",
                json!({"sessionId":id,"afterSequence":after,"limit":2}),
            )
            .await;
            assert!(page.as_array().unwrap().len() <= 2);
            for e in page.as_array().unwrap() {
                let seq = e["sequence"].as_u64().unwrap();
                assert!(seq > after);
                after = seq;
                if e["type"] == "text.delta" {
                    text.push_str(e["data"]["text"].as_str().unwrap());
                }
                finished |= e["type"] == "turn.ended";
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(text.contains("Hello"));
    let notification = client
        .post(&url)
        .bearer_auth("fixture-secret")
        .json(&json!({"jsonrpc":"2.0","method":"session/close","params":{"sessionId":id}}))
        .send()
        .await
        .unwrap();
    assert_eq!(notification.status(), 204);
    stop(&mut child).await;
}

#[tokio::test]
async fn desktop_assets_allow_only_their_explicit_origin_and_host() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("index.html"),
        "<!doctype html><title>Desktop fixture</title>",
    )
    .unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let addr = format!("127.0.0.1:{port}");
    let origin = format!("http://{addr}");
    let rpc = format!("{origin}/rpc");
    let mut child = Command::new(env!("CARGO_BIN_EXE_mpai"))
        .args([
            "--state-dir",
            dir.path().join("state").to_str().unwrap(),
            "serve",
            "--http",
            &addr,
            "--desktop-assets",
            dir.path().to_str().unwrap(),
            "--token",
            "desktop-secret",
        ])
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let client = reqwest::Client::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if client.get(&origin).send().await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        client
            .get(&origin)
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap()
            .contains("Desktop fixture")
    );
    let request = json!({"jsonrpc":"2.0","id":1,"method":"runtime/initialize","params":{}});
    assert_eq!(
        client
            .post(&rpc)
            .header("origin", &origin)
            .json(&request)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let allowed: Value = client
        .post(&rpc)
        .header("origin", &origin)
        .bearer_auth("desktop-secret")
        .json(&request)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(allowed["result"]["name"], "mpai");
    for bad_origin in ["null", "https://example.test", "http://127.0.0.1:1"] {
        assert_eq!(
            client
                .post(&rpc)
                .header("origin", bad_origin)
                .bearer_auth("desktop-secret")
                .json(&request)
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
    }
    assert_eq!(
        client
            .post(&rpc)
            .header("host", "attacker.test")
            .bearer_auth("desktop-secret")
            .json(&request)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    assert_eq!(
        client
            .get(format!("{origin}/missing.js"))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    stop(&mut child).await;
}
