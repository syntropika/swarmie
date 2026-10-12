use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
};
async fn ready_url(child: &mut Child) -> String {
    let mut lines = BufReader::new(child.stderr.take().unwrap()).lines();
    tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(line) = lines.next_line().await.unwrap() {
            if let Some(url) = line.strip_prefix("HTTP listening on ") {
                return url.to_string();
            }
        }
        panic!("daemon exited before reporting its listener");
    })
    .await
    .unwrap()
}
async fn cli(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mpai"))
        .args(["--state-dir", root.to_str().unwrap()])
        .args(args)
        .env_remove("MPAI_TOKEN")
        .output()
        .await
        .unwrap()
}
#[tokio::test]
async fn pairing_and_saved_calls_use_real_daemon_bearer_auth_and_redact_credentials() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("sessions");
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_mpai"))
        .args([
            "--state-dir",
            root.path().join("daemon").to_str().unwrap(),
            "serve",
            "--http",
            "127.0.0.1:0",
            "--token",
            "fixture-private-token",
        ])
        .kill_on_drop(true)
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let url = ready_url(&mut daemon).await;

    let unauthorized = cli(&state, &["remote", "pair", "fixture", &url]).await;
    assert!(!unauthorized.status.success());
    assert!(!root.path().join("connections.json").exists());
    let paired = cli(
        &state,
        &[
            "remote",
            "pair",
            "fixture",
            url.trim_end_matches("/rpc"),
            "--token",
            "fixture-private-token",
        ],
    )
    .await;
    assert!(
        paired.status.success(),
        "{}",
        String::from_utf8_lossy(&paired.stderr)
    );
    assert!(!String::from_utf8_lossy(&paired.stdout).contains("fixture-private-token"));
    let listed = cli(&state, &["remote", "list"]).await;
    let entries: Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(entries[0]["name"], "fixture");
    assert_eq!(entries[0]["authenticated"], true);
    assert!(!String::from_utf8_lossy(&listed.stdout).contains("fixture-private-token"));
    let called = cli(&state, &["remote", "call", "fixture", "runtime/initialize"]).await;
    assert!(called.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&called.stdout).unwrap()["name"],
        "mpai"
    );
    let removed = cli(&state, &["remote", "remove", "fixture"]).await;
    assert!(removed.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&cli(&state, &["remote", "list"]).await.stdout).unwrap(),
        json!([])
    );
    daemon.kill().await.unwrap();
    let _ = daemon.wait().await;
}
#[tokio::test]
async fn terminal_open_encodes_a_real_project_and_session_without_launching_when_printed() {
    let root = tempfile::tempdir().unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let output = cli(
        root.path(),
        &[
            "open",
            "--url",
            "http://127.0.0.1:7480/",
            "--project",
            root.path().to_str().unwrap(),
            "--session",
            &id,
            "--print",
        ],
    )
    .await;
    assert!(output.status.success());
    let url = reqwest::Url::parse(String::from_utf8_lossy(&output.stdout).trim()).unwrap();
    assert!(
        url.query_pairs()
            .any(|(key, value)| key == "session" && value == id)
    );
    assert!(
        url.query_pairs()
            .any(|(key, value)| key == "project" && value == root.path().to_string_lossy())
    );
}
#[cfg(target_os = "linux")]
#[tokio::test]
async fn service_cli_installs_private_config_and_invokes_the_user_manager() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config");
    let bin = root.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let manager = bin.join("systemctl");
    std::fs::write(
        &manager,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$MPAI_MANAGER_LEDGER\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&manager, std::fs::Permissions::from_mode(0o755)).unwrap();
    let ledger = root.path().join("manager.log");
    let command = |args: Vec<String>| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mpai"));
        command
            .args(["--state-dir", root.path().join("state").to_str().unwrap()])
            .args(args)
            .env("XDG_CONFIG_HOME", &config)
            .env("HOME", root.path())
            .env(
                "PATH",
                format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
            )
            .env("MPAI_MANAGER_LEDGER", &ledger)
            .env_remove("MPAI_TOKEN");
        command
    };
    let output = command(vec![
        "service".into(),
        "install".into(),
        "--port".into(),
        "7497".into(),
    ])
    .output()
    .await
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let unit = config.join("systemd/user/mpai.service");
    let token = config.join("mpai/service.token");
    assert!(unit.exists());
    assert_eq!(
        std::fs::metadata(&token).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let token = std::fs::read_to_string(&token).unwrap();
    assert_eq!(token.len(), 64);
    assert!(!String::from_utf8_lossy(&output.stdout).contains(&token));
    assert!(
        std::fs::read_to_string(&unit)
            .unwrap()
            .contains("127.0.0.1:7497")
    );
    for operation in ["start", "status", "stop", "uninstall"] {
        let output = command(vec!["service".into(), operation.into()])
            .output()
            .await
            .unwrap();
        assert!(output.status.success());
    }
    assert!(!unit.exists());
    let actions = std::fs::read_to_string(ledger).unwrap();
    assert!(actions.contains("--user start mpai.service"));
    assert!(actions.contains("--user status mpai.service"));
    assert!(actions.contains("--user stop mpai.service"));
}

#[tokio::test]
async fn explicit_https_proxy_origin_accepts_only_the_paired_origin_host_and_bearer() {
    let root = tempfile::tempdir().unwrap();
    let assets = root.path().join("assets");
    std::fs::create_dir_all(&assets).unwrap();
    std::fs::write(
        assets.join("index.html"),
        "<html lang=\"en\"><title>Fixture frontend</title></html>",
    )
    .unwrap();
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_mpai"))
        .args([
            "--state-dir",
            root.path().join("daemon").to_str().unwrap(),
            "serve",
            "--http",
            "127.0.0.1:0",
            "--desktop-assets",
            assets.to_str().unwrap(),
            "--public-origin",
            "https://paired.example.test",
            "--token",
            "fixture-proxy-token",
        ])
        .kill_on_drop(true)
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let url = ready_url(&mut daemon).await;
    let client = reqwest::Client::new();

    for (origin, host, token, status) in [
        (
            "https://paired.example.test",
            "paired.example.test",
            true,
            200,
        ),
        (
            "https://wrong.example.test",
            "paired.example.test",
            true,
            403,
        ),
        (
            "https://paired.example.test",
            "wrong.example.test",
            true,
            403,
        ),
        (
            "https://paired.example.test",
            "paired.example.test",
            false,
            401,
        ),
    ] {
        let mut request = client
            .post(&url)
            .header("Origin", origin)
            .header("Host", host)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"runtime/initialize","params":{}}));
        if token {
            request = request.bearer_auth("fixture-proxy-token");
        }
        let response = request.send().await.unwrap();
        assert_eq!(response.status().as_u16(), status);
    }
    daemon.kill().await.unwrap();
    let _ = daemon.wait().await;
}

// Detached test owners must be stopped even if an assertion fails.
struct RuntimeCleanup(std::path::PathBuf);
impl Drop for RuntimeCleanup {
    fn drop(&mut self) {
        let _ = std::process::Command::new(env!("CARGO_BIN_EXE_mpai"))
            .args(["--state-dir", self.0.to_str().unwrap(), "runtime", "stop"])
            .env_remove("MPAI_TOKEN")
            .output();
    }
}
#[cfg(unix)]
#[tokio::test]
async fn managed_desktop_reuses_detached_owner_and_stops_only_explicitly() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let assets = root.path().join("assets");
    std::fs::create_dir_all(&assets).unwrap();
    std::fs::write(assets.join("index.html"), "<html>fixture desktop</html>").unwrap();
    let host = root.path().join("fixture-host");
    std::fs::write(&host,r#"#!/usr/bin/env python3
import os,json,urllib.request
assert os.environ['MPAI_DESKTOP_MANAGED']=='1'
url=os.environ['MPAI_DESKTOP_URL']
r=urllib.request.Request(url+'/rpc', data=json.dumps({'jsonrpc':'2.0','id':1,'method':'runtime/initialize','params':{}}).encode(), headers={'Content-Type':'application/json'})
assert json.load(urllib.request.urlopen(r))['result']['name']=='mpai'
print(url)
"#).unwrap();
    std::fs::set_permissions(&host, std::fs::Permissions::from_mode(0o700)).unwrap();
    let state = root.path().join("sessions");
    let _cleanup = RuntimeCleanup(state.clone());
    let args = [
        "desktop",
        "--assets",
        assets.to_str().unwrap(),
        "--host",
        host.to_str().unwrap(),
    ];
    // Simultaneous first launch must publish only one owner.
    let (first, second) = tokio::join!(cli(&state, &args), cli(&state, &args));
    for output in [&first, &second] {
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let url = String::from_utf8(first.stdout).unwrap();
    assert_eq!(url, String::from_utf8(second.stdout).unwrap());
    assert!(
        reqwest::get(url.trim())
            .await
            .unwrap()
            .status()
            .is_success()
    );
    let before: Value =
        serde_json::from_slice(&cli(&state, &["runtime", "status"]).await.stdout).unwrap();
    let reopened = cli(&state, &args).await;
    assert!(reopened.status.success());
    assert_eq!(String::from_utf8(reopened.stdout).unwrap(), url);
    // A failed client must not stop the runtime either.
    std::fs::write(&host, "#!/bin/sh\nexit 7\n").unwrap();
    assert!(!cli(&state, &args).await.status.success());
    let after: Value =
        serde_json::from_slice(&cli(&state, &["runtime", "status"]).await.stdout).unwrap();
    let nodes: Value =
        serde_json::from_slice(&cli(&state, &["node", "list"]).await.stdout).unwrap();
    assert_eq!(nodes["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(nodes["nodes"][0]["local"], true);
    assert_eq!(nodes["nodes"][0]["displayName"], "This computer");
    assert_eq!(
        nodes["nodes"][0]["identity"],
        before["endpoint"]["runtimeIdentity"]["identity"]
    );
    assert_eq!(before, after);
    let stopped = cli(&state, &["runtime", "stop"]).await;
    assert!(
        stopped.status.success(),
        "{}",
        String::from_utf8_lossy(&stopped.stderr)
    );
    assert!(reqwest::get(url.trim()).await.is_err());
    assert!(state.join("state/host.sqlite").exists());
    let status: Value =
        serde_json::from_slice(&cli(&state, &["runtime", "status"]).await.stdout).unwrap();
    assert_eq!(status["running"], false);
}

#[cfg(unix)]
fn ssh_fixture(root: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let bin = root.join("fixture-bin");
    std::fs::create_dir_all(&bin).unwrap();
    let ssh = bin.join("ssh");
    std::fs::write(&ssh, include_str!("fixtures/ssh_forward.py")).unwrap();
    std::fs::set_permissions(ssh, std::fs::Permissions::from_mode(0o700)).unwrap();
    bin
}
#[cfg(unix)]
async fn ssh_receipt(
    lines: &mut tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    event: &str,
) -> Value {
    tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(line) = lines.next_line().await.unwrap() {
            let receipt: Value = serde_json::from_str(&line).unwrap();
            assert!(!line.contains("supervisor-private-token"));
            if receipt["event"] == event {
                return receipt;
            }
        }
        panic!("supervisor exited before {event}");
    })
    .await
    .unwrap()
}
#[cfg(unix)]
#[tokio::test]
async fn reconnect_restores_forwarded_session_execution_and_cancellation_stops_the_backoff() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("sessions");
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_mpai"))
        .args([
            "--state-dir",
            root.path().join("daemon").to_str().unwrap(),
            "serve",
            "--http",
            "127.0.0.1:0",
            "--token",
            "supervisor-private-token",
            "--mock-script",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../tests/fixtures/client-script.json"
            ),
        ])
        .kill_on_drop(true)
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let remote = reqwest::Url::parse(&ready_url(&mut daemon).await)
        .unwrap()
        .port()
        .unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let log = root.path().join("ssh-starts.jsonl");
    let bin = ssh_fixture(root.path());
    let mut supervisor = Command::new(env!("CARGO_BIN_EXE_mpai"))
        .args([
            "--state-dir",
            state.to_str().unwrap(),
            "remote",
            "ssh",
            "fixture",
            "fixture-host",
            "--local-port",
            &port.to_string(),
            "--remote-port",
            &remote.to_string(),
            "--token",
            "supervisor-private-token",
            "--reconnect",
        ])
        .env(
            "PATH",
            format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
        )
        .env("MPAI_SSH_FIXTURE_LOG", &log)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(supervisor.stdout.take().unwrap()).lines();
    let first = ssh_receipt(&mut lines, "ssh.connected").await;
    let first_pid = first["sshPid"].as_u64().unwrap() as i32;
    assert_eq!(unsafe { libc::kill(first_pid, libc::SIGKILL) }, 0);
    ssh_receipt(&mut lines, "ssh.disconnected").await;
    let retry = ssh_receipt(&mut lines, "ssh.reconnecting").await;
    assert_eq!(retry["retryAfterMs"], 500);
    let restored = ssh_receipt(&mut lines, "ssh.connected").await;
    assert_eq!(restored["url"], first["url"]);
    assert_eq!(restored["attempt"], 2);
    assert_ne!(restored["sshPid"], first["sshPid"]);
    let created = cli(
        &state,
        &[
            "remote",
            "call",
            "fixture",
            "session/create",
            "--params",
            &json!({"harness":"mock","cwd":root.path()}).to_string(),
        ],
    )
    .await;
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    let session: Value = serde_json::from_slice(&created.stdout).unwrap();
    let id = session["id"].as_str().unwrap();
    let prompted = cli(
        &state,
        &[
            "remote",
            "call",
            "fixture",
            "session/prompt",
            "--params",
            &json!({"sessionId":id,"text":"run through restored forward","idempotencyKey":"run through restored forward"}).to_string(),
        ],
    )
    .await;
    assert!(
        prompted.status.success(),
        "{}",
        String::from_utf8_lossy(&prompted.stderr)
    );
    let events: Value = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let transcript = cli(
                &state,
                &[
                    "remote",
                    "call",
                    "fixture",
                    "session/transcript",
                    "--params",
                    &json!({"sessionId":id}).to_string(),
                ],
            )
            .await;
            assert!(transcript.status.success());
            let events: Value = serde_json::from_slice(&transcript.stdout).unwrap();
            if events
                .as_array()
                .unwrap()
                .iter()
                .any(|event| event["type"] == "turn.ended")
            {
                break events;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        events
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["type"] == "text.delta" && event["data"]["text"] == "client-ready")
    );
    assert!(
        events
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["type"] == "turn.ended")
    );
    assert_eq!(
        unsafe { libc::kill(restored["sshPid"].as_u64().unwrap() as i32, libc::SIGKILL) },
        0
    );
    let retry = ssh_receipt(&mut lines, "ssh.reconnecting").await;
    assert_eq!(retry["retryAfterMs"], 1000);
    assert_eq!(
        unsafe { libc::kill(supervisor.id().unwrap() as i32, libc::SIGINT) },
        0
    );
    ssh_receipt(&mut lines, "ssh.stopped").await;
    assert!(
        tokio::time::timeout(Duration::from_secs(2), supervisor.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(
        std::fs::read_to_string(log).unwrap().lines().count(),
        2,
        "cancelled supervisor must not spawn another forward"
    );
    assert!(
        tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_err()
    );
    daemon.kill().await.unwrap();
    let _ = daemon.wait().await;
}
#[cfg(unix)]
#[tokio::test]
async fn ssh_handshake_is_promptly_cancellable_before_any_daemon_is_available() {
    let root = tempfile::tempdir().unwrap();
    let bin = ssh_fixture(root.path());
    let log = root.path().join("starts.jsonl");
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut supervisor = Command::new(env!("CARGO_BIN_EXE_mpai"))
        .args([
            "--state-dir",
            root.path().join("sessions").to_str().unwrap(),
            "remote",
            "ssh",
            "fixture",
            "fixture-host",
            "--local-port",
            &port.to_string(),
            "--remote-port",
            "1",
            "--reconnect",
        ])
        .env(
            "PATH",
            format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
        )
        .env("MPAI_SSH_FIXTURE_LOG", &log)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !log.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let started = tokio::time::Instant::now();
    assert_eq!(
        unsafe { libc::kill(supervisor.id().unwrap() as i32, libc::SIGINT) },
        0
    );
    assert!(
        tokio::time::timeout(Duration::from_secs(2), supervisor.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    assert!(started.elapsed() < Duration::from_secs(2));
    let process: Value =
        serde_json::from_str(std::fs::read_to_string(log).unwrap().trim()).unwrap();
    assert_eq!(
        unsafe { libc::kill(process["pid"].as_u64().unwrap() as i32, 0) },
        -1,
        "owned SSH child must be reaped"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn default_ssh_forward_exits_after_its_owned_child_drops_without_a_retry() {
    let root = tempfile::tempdir().unwrap();
    let bin = ssh_fixture(root.path());
    let log = root.path().join("starts.jsonl");
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_mpai"))
        .args([
            "--state-dir",
            root.path().join("daemon").to_str().unwrap(),
            "serve",
            "--http",
            "127.0.0.1:0",
        ])
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let remote = reqwest::Url::parse(&ready_url(&mut daemon).await)
        .unwrap()
        .port()
        .unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut supervisor = Command::new(env!("CARGO_BIN_EXE_mpai"))
        .args([
            "--state-dir",
            root.path().join("sessions").to_str().unwrap(),
            "remote",
            "ssh",
            "fixture",
            "fixture-host",
            "--local-port",
            &port.to_string(),
            "--remote-port",
            &remote.to_string(),
        ])
        .env_remove("MPAI_TOKEN")
        .env(
            "PATH",
            format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
        )
        .env("MPAI_SSH_FIXTURE_LOG", &log)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(supervisor.stdout.take().unwrap()).lines();
    let connected = ssh_receipt(&mut lines, "ssh.connected").await;
    assert_eq!(
        unsafe { libc::kill(connected["sshPid"].as_u64().unwrap() as i32, libc::SIGKILL) },
        0
    );
    assert!(
        !tokio::time::timeout(Duration::from_secs(2), supervisor.wait())
            .await
            .unwrap()
            .unwrap()
            .success()
    );
    assert_eq!(std::fs::read_to_string(log).unwrap().lines().count(), 1);
    assert!(lines.next_line().await.unwrap().is_none());
    daemon.kill().await.unwrap();
    let _ = daemon.wait().await;
}

async fn identity_daemon(root: &Path, address: &str) -> (Child, String) {
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_mpai"))
        .args([
            "--state-dir",
            root.to_str().unwrap(),
            "serve",
            "--http",
            address,
            "--token",
            "identity-fixture-token",
        ])
        .kill_on_drop(true)
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let url = ready_url(&mut daemon).await;
    (daemon, url)
}

#[tokio::test]
async fn saved_node_reconnect_accepts_restart_but_rejects_replacement_at_the_same_address() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("client");
    let data = root.path().join("original");
    let (mut daemon, url) = identity_daemon(&data, "127.0.0.1:0").await;
    let address = format!(
        "127.0.0.1:{}",
        reqwest::Url::parse(&url).unwrap().port().unwrap()
    );
    let paired = cli(
        &state,
        &[
            "remote",
            "pair",
            "node",
            &url,
            "--token",
            "identity-fixture-token",
        ],
    )
    .await;
    assert!(
        paired.status.success(),
        "{}",
        String::from_utf8_lossy(&paired.stderr)
    );
    let original: Value = serde_json::from_slice(&paired.stdout).unwrap();
    let first = cli(&state, &["remote", "call", "node", "runtime/initialize"]).await;
    let first: Value = serde_json::from_slice(&first.stdout).unwrap();
    let created = cli(
        &state,
        &[
            "remote",
            "call",
            "node",
            "project/create",
            "--params",
            r#"{"name":"retained"}"#,
        ],
    )
    .await;
    assert!(created.status.success());
    daemon.kill().await.unwrap();
    daemon.wait().await.unwrap();
    let (mut restarted, _) = identity_daemon(&data, &address).await;
    let second = cli(&state, &["remote", "call", "node", "runtime/initialize"]).await;
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    let second: Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(
        first["runtimeIdentity"]["identity"],
        second["runtimeIdentity"]["identity"]
    );
    assert_ne!(
        first["runtimeIdentity"]["incarnationId"],
        second["runtimeIdentity"]["incarnationId"]
    );
    let projects = cli(&state, &["remote", "call", "node", "project/list"]).await;
    assert_eq!(
        serde_json::from_slice::<Value>(&projects.stdout).unwrap()[0]["name"],
        "retained"
    );
    restarted.kill().await.unwrap();
    restarted.wait().await.unwrap();
    let (mut replacement, _) = identity_daemon(&root.path().join("replacement"), &address).await;
    let blocked = cli(
        &state,
        &[
            "remote",
            "call",
            "node",
            "project/create",
            "--params",
            r#"{"name":"must not exist"}"#,
        ],
    )
    .await;
    assert!(!blocked.status.success());
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("environment identity changed"));
    let re_pair = cli(
        &state,
        &[
            "remote",
            "pair",
            "node",
            &url,
            "--token",
            "identity-fixture-token",
        ],
    )
    .await;
    assert!(!re_pair.status.success());
    let listed = cli(&state, &["remote", "list"]).await;
    assert_eq!(
        serde_json::from_slice::<Value>(&listed.stdout).unwrap()[0]["identity"],
        original["identity"]
    );
    // A request already prepared for the old owner is also rejected by the receiver.
    let response: Value = reqwest::Client::new()
        .post(&url)
        .bearer_auth("identity-fixture-token")
        .json(&json!({"jsonrpc":"2.0","id":1,"method":"project/create",
            "params":{"name":"stale"},"expectedRuntime":second["runtimeIdentity"]}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(response["error"]["code"], -32005);
    assert!(
        cli(&state, &["remote", "remove", "node"])
            .await
            .status
            .success()
    );
    assert!(
        cli(
            &state,
            &[
                "remote",
                "pair",
                "node",
                &url,
                "--token",
                "identity-fixture-token"
            ]
        )
        .await
        .status
        .success()
    );
    let projects = cli(&state, &["remote", "call", "node", "project/list"]).await;
    assert_eq!(
        serde_json::from_slice::<Value>(&projects.stdout).unwrap(),
        json!([])
    );
    replacement.kill().await.unwrap();
    replacement.wait().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn ssh_reconnect_rejects_a_different_environment_and_reaps_its_forward() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("client");
    let (mut daemon, url) = identity_daemon(&root.path().join("original"), "127.0.0.1:0").await;
    let remote_port = reqwest::Url::parse(&url).unwrap().port().unwrap();
    let local_port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let bin = ssh_fixture(root.path());
    let log = root.path().join("ssh-starts.jsonl");
    let mut supervisor = Command::new(env!("CARGO_BIN_EXE_mpai"))
        .args([
            "--state-dir",
            state.to_str().unwrap(),
            "remote",
            "ssh",
            "node",
            "fixture-host",
            "--local-port",
            &local_port.to_string(),
            "--remote-port",
            &remote_port.to_string(),
            "--token",
            "identity-fixture-token",
            "--reconnect",
        ])
        .env(
            "PATH",
            format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()),
        )
        .env("MPAI_SSH_FIXTURE_LOG", &log)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(supervisor.stdout.take().unwrap()).lines();
    let first = ssh_receipt(&mut lines, "ssh.connected").await;
    daemon.kill().await.unwrap();
    daemon.wait().await.unwrap();
    let (mut replacement, _) = identity_daemon(
        &root.path().join("replacement"),
        &format!("127.0.0.1:{remote_port}"),
    )
    .await;
    assert_eq!(
        unsafe { libc::kill(first["sshPid"].as_u64().unwrap() as i32, libc::SIGKILL) },
        0
    );
    let status = tokio::time::timeout(Duration::from_secs(10), supervisor.wait())
        .await
        .unwrap()
        .unwrap();
    assert!(!status.success());
    use tokio::io::AsyncReadExt;
    let mut error = String::new();
    supervisor
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut error)
        .await
        .unwrap();
    assert!(error.contains("environment identity changed"), "{error}");
    let listed = cli(&state, &["remote", "list"]).await;
    assert_eq!(
        serde_json::from_slice::<Value>(&listed.stdout).unwrap()[0]["identity"],
        first["identity"]
    );
    for line in std::fs::read_to_string(&log).unwrap().lines() {
        let process: Value = serde_json::from_str(line).unwrap();
        assert_eq!(
            unsafe { libc::kill(process["pid"].as_u64().unwrap() as i32, 0) },
            -1
        );
    }
    assert!(
        tokio::net::TcpStream::connect(("127.0.0.1", local_port))
            .await
            .is_err()
    );
    replacement.kill().await.unwrap();
    replacement.wait().await.unwrap();
}

#[tokio::test]
async fn desktop_exit_preserves_running_work_for_another_client_and_rejects_foreign_stop_record() {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("owner");
    let assets = root.path().join("assets");
    std::fs::create_dir_all(&assets).unwrap();
    std::fs::write(assets.join("index.html"), "<html>fixture</html>").unwrap();
    let script = root.path().join("script.json");
    std::fs::write(&script, json!({"turns":[
        {"expectedText":"first client","steps":[{"type":"sleep","milliseconds":800},{"type":"text","text":"work survived"},{"type":"end","outcome":"completed"}]},
        {"expectedText":"second client","steps":[{"type":"text","text":"continued"},{"type":"end","outcome":"completed"}]}
    ]}).to_string()).unwrap();
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_mpai"))
        .args([
            "--state-dir",
            state.to_str().unwrap(),
            "serve",
            "--http",
            "127.0.0.1:0",
            "--desktop-assets",
            assets.to_str().unwrap(),
            "--mock-script",
            script.to_str().unwrap(),
        ])
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let url = ready_url(&mut daemon).await;
    let url = url.trim_end_matches("/rpc");
    let first = reqwest::Client::new();
    let call = |client: reqwest::Client, method: &'static str, params: Value| {
        let url = url.to_owned();
        async move {
            let value: Value = client
                .post(format!("{url}/rpc"))
                .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            assert!(value.get("error").is_none(), "{value}");
            value["result"].clone()
        }
    };
    let identity =
        call(first.clone(), "runtime/initialize", json!({})).await["runtimeIdentity"].clone();
    let session = call(
        first.clone(),
        "session/create",
        json!({"harness":"mock","cwd":root.path()}),
    )
    .await;
    let id = session["id"].as_str().unwrap();
    call(
        first.clone(),
        "session/prompt",
        json!({"sessionId":id,"text":"first client","idempotencyKey":"first"}),
    )
    .await;
    drop(first);
    #[cfg(unix)]
    {
        let host = root.path().join("host");
        std::fs::write(&host, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&host, std::fs::Permissions::from_mode(0o700)).unwrap();
        let desktop = cli(
            &state,
            &[
                "desktop",
                "--assets",
                assets.to_str().unwrap(),
                "--host",
                host.to_str().unwrap(),
            ],
        )
        .await;
        assert!(
            desktop.status.success(),
            "{}",
            String::from_utf8_lossy(&desktop.stderr)
        );
    }
    let second = reqwest::Client::new();
    assert_eq!(
        call(second.clone(), "runtime/initialize", json!({})).await["runtimeIdentity"],
        identity
    );
    assert_eq!(
        call(second.clone(), "session/get", json!({"sessionId":id})).await["state"],
        "running"
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let events = call(
                second.clone(),
                "session/events",
                json!({"sessionId":id,"afterSequence":0}),
            )
            .await;
            if events
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["type"] == "turn.ended")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    call(second.clone(), "session/resume", json!({"sessionId":id})).await;
    call(
        second.clone(),
        "session/prompt",
        json!({"sessionId":id,"text":"second client","idempotencyKey":"second"}),
    )
    .await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let events = call(
                second.clone(),
                "session/transcript",
                json!({"sessionId":id}),
            )
            .await;
            let text = events
                .as_array()
                .unwrap()
                .iter()
                .filter(|e| e["type"] == "text.delta")
                .map(|e| e["data"]["text"].as_str().unwrap_or(""))
                .collect::<String>();
            if text.contains("continued") {
                assert!(text.contains("work survived"));
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    // A copied endpoint record cannot authorize stopping an owner of another retained store.
    let other = root.path().join("other");
    let _store = storage::SqliteSessionStore::new(&other).await.unwrap();
    std::fs::copy(
        state.join("runtime/http-endpoint.json"),
        other.join("runtime/http-endpoint.json"),
    )
    .unwrap();
    let rejected = cli(&other, &["runtime", "stop"]).await;
    assert!(!rejected.status.success());
    assert_eq!(
        call(second.clone(), "runtime/initialize", json!({})).await["runtimeIdentity"],
        identity
    );
    let stopped = cli(&state, &["runtime", "stop"]).await;
    assert!(
        stopped.status.success(),
        "{}",
        String::from_utf8_lossy(&stopped.stderr)
    );
    assert!(daemon.wait().await.unwrap().success());
}

#[tokio::test]
async fn simultaneous_pairing_groups_one_node_and_preserves_legacy_files_and_remote_execution() {
    let root = tempfile::tempdir().unwrap();
    let client = root.path().join("client");
    let legacy = root.path().join("connections.json");
    std::fs::write(
        &legacy,
        "existing development metadata must remain untouched",
    )
    .unwrap();
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_mpai"))
        .args([
            "--state-dir",
            root.path().join("owner").to_str().unwrap(),
            "serve",
            "--http",
            "127.0.0.1:0",
            "--token",
            "registry-private-token",
        ])
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let url = ready_url(&mut daemon).await;
    let first_args = [
        "remote",
        "pair",
        "route-a",
        &url,
        "--token",
        "registry-private-token",
    ];
    let second_args = [
        "remote",
        "pair",
        "route-b",
        &url,
        "--token",
        "registry-private-token",
    ];
    let (first, second) = tokio::join!(cli(&client, &first_args), cli(&client, &second_args));
    for output in [&first, &second] {
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains("registry-private-token"));
    }
    let nodes: Value =
        serde_json::from_slice(&cli(&client, &["node", "list"]).await.stdout).unwrap();
    assert_eq!(nodes["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(
        nodes["nodes"][0]["connections"].as_array().unwrap().len(),
        2
    );
    assert!(nodes["nodes"][0].get("online").is_none());
    assert!(nodes["nodes"][0].get("incarnationId").is_none());
    assert!(!nodes.to_string().contains("registry-private-token"));
    for database in [
        client.join("state/host.sqlite"),
        client.join("state/host.sqlite-wal"),
    ] {
        if let Ok(bytes) = std::fs::read(database) {
            assert!(
                !bytes
                    .windows(b"registry-private-token".len())
                    .any(|value| value == b"registry-private-token")
            );
        }
    }
    assert_eq!(
        std::fs::read_to_string(&legacy).unwrap(),
        "existing development metadata must remain untouched"
    );
    let created = cli(
        &client,
        &[
            "remote",
            "call",
            "route-a",
            "project/create",
            "--params",
            r#"{"name":"retained remote project"}"#,
        ],
    )
    .await;
    assert!(created.status.success());
    assert!(
        cli(&client, &["remote", "remove", "route-a"])
            .await
            .status
            .success()
    );
    let read = cli(&client, &["remote", "call", "route-b", "project/list"]).await;
    assert!(read.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&read.stdout).unwrap()[0]["name"],
        "retained remote project"
    );
    assert!(
        cli(&client, &["remote", "remove", "route-b"])
            .await
            .status
            .success()
    );
    let tombstone: Value =
        serde_json::from_slice(&cli(&client, &["node", "list"]).await.stdout).unwrap();
    assert_eq!(tombstone["nodes"][0]["registered"], false);
    assert_eq!(
        tombstone["nodes"][0]["identity"],
        nodes["nodes"][0]["identity"]
    );
    assert_eq!(
        std::fs::read_dir(client.join("credentials/node-connections"))
            .unwrap()
            .count(),
        0
    );
    daemon.kill().await.unwrap();
    let _ = daemon.wait().await;
}
