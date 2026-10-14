// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
//! CLI clients exit while their independently hosted account owner keeps the pending login alive.
use axum::{
    Json,
    routing::{get, post},
};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
async fn owner_request(root: &std::path::Path, method: &str, params: Value) -> Value {
    let endpoint: Value =
        serde_json::from_slice(&std::fs::read(root.join("identity/endpoint.json")).unwrap())
            .unwrap();
    let value = reqwest::Client::new()
        .post(format!("{}/rpc", endpoint["url"].as_str().unwrap()))
        .bearer_auth(endpoint["token"].as_str().unwrap())
        .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert!(value.get("error").is_none(), "{value}");
    value["result"].clone()
}
async fn command(root: &std::path::Path, args: &[&str]) -> Value {
    let output = tokio::process::Command::new(
        std::env::var_os("SWARMIE_TEST_BINARY")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_swarmie").into()),
    )
    .arg("--state-dir")
    .arg(root)
    .arg("account")
    .args(args)
    .kill_on_drop(true)
    .output()
    .await
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
#[tokio::test]
async fn native_owner_is_independent_of_windows_clients_and_execution_nodes() {
    let root = tempfile::tempdir().unwrap();
    let saved = Arc::new(Mutex::new(Value::Null));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let authorization = format!("{origin}/auth/native/authorize?request=browser-proof");
    let copy = saved.clone();
    let profile = json!({"id":uuid::Uuid::nil(),"email":"test@example.com","emailVerified":true,"displayName":"Test","avatarUrl":null,"revision":0});
    let grant = json!({"accessToken":"native-private-access","refreshToken":"native-private-refresh","expiresAt":i64::MAX/2,"profile":profile});
    let profile_copy = profile.clone();
    let router = axum::Router::new()
        .route(
            "/auth/native/request",
            post(move |Json(value): Json<Value>| {
                let saved = copy.clone();
                let authorization = authorization.clone();
                async move {
                    *saved.lock().await = value;
                    Json(json!({"authorizationUrl":authorization}))
                }
            }),
        )
        .route(
            "/auth/native/redeem",
            post(move || {
                let grant = grant.clone();
                async move { Json(grant) }
            }),
        )
        .route(
            "/auth/native/revoke",
            post(|| async { axum::http::StatusCode::NO_CONTENT }),
        )
        .route(
            "/api/native/profile",
            get(move || {
                let profile = profile_copy.clone();
                async move { Json(profile) }
            }),
        );
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let configured = command(root.path(), &["configure", &origin]).await;
    assert_eq!(configured["configured"], true);
    let initial = owner_request(root.path(), "account/observe", json!({})).await;
    let cursor = initial["cursor"].clone();
    let observers: Vec<_> = (0..2)
        .map(|_| {
            let path = root.path().to_owned();
            let cursor = cursor.clone();
            tokio::spawn(async move {
                owner_request(&path, "account/observe", json!({"cursor":cursor})).await
            })
        })
        .collect();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(observers.iter().all(|task| !task.is_finished()));
    let pending = command(root.path(), &["login", "--session-only", "--no-open"]).await;
    assert!(pending["loginId"].is_string());
    for task in observers {
        let observed = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(observed["account"]["status"], "signingIn");
        assert_eq!(
            observed["account"]["authorizationUrl"],
            pending["authorizationUrl"]
        );
        assert_eq!(observed["cursor"]["ownerId"], cursor["ownerId"]);
        assert!(observed["cursor"]["revision"].as_u64() > cursor["revision"].as_u64());
        assert!(!observed.to_string().contains("native-private"));
    }
    let waiting = owner_request(root.path(), "account/observe", json!({})).await;
    let path = root.path().to_owned();
    let completing = tokio::spawn(async move {
        owner_request(
            &path,
            "account/observe",
            json!({"cursor":waiting["cursor"]}),
        )
        .await
    });
    // No initiating client remains. A later browser callback still completes the same native attempt.
    let value = saved.lock().await.clone();
    let callback = value["callback"].as_str().unwrap();
    let nonce = value["state"].as_str().unwrap();
    let client = reqwest::Client::new();
    assert_eq!(
        client
            .get(callback)
            .query(&[("state", "wrong"), ("code", "test-code")])
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::BAD_REQUEST
    );
    assert!(
        client
            .get(callback)
            .query(&[("state", nonce), ("code", "test-code")])
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
    let snapshot = command(root.path(), &["read"]).await;
    assert_eq!(snapshot["status"], "signedIn");
    assert_eq!(snapshot["profile"], profile);
    assert_eq!(snapshot["authorizationUrl"], Value::Null);
    let completed = tokio::time::timeout(Duration::from_secs(5), completing)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(completed["account"]["status"], "signedIn");
    assert_eq!(completed["account"]["profile"], profile);
    assert!(!snapshot.to_string().contains("native-private"));
    assert!(!root.path().join("state").exists());
    assert!(!root.path().join("runtime").exists());
    let endpoint: Value =
        serde_json::from_slice(&std::fs::read(root.path().join("identity/endpoint.json")).unwrap())
            .unwrap();
    assert_eq!(
        client
            .post(format!("{}/rpc", endpoint["url"].as_str().unwrap()))
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"account/read"}))
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::FORBIDDEN
    );
    assert!(
        command(root.path(), &["logout"]).await["remoteRevocationConfirmed"]
            .as_bool()
            .unwrap()
    );
    command(root.path(), &["stop"]).await;
    for _ in 0..30 {
        if !root.path().join("identity/endpoint.json").exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(!root.path().join("identity/endpoint.json").exists());
    command(root.path(), &["read"]).await;
    let restarted = owner_request(root.path(), "account/observe", json!({"cursor":cursor})).await;
    assert_ne!(restarted["cursor"]["ownerId"], cursor["ownerId"]);
    assert_eq!(restarted["account"]["status"], "signedOut");
    command(root.path(), &["stop"]).await;
    server.abort();
}
