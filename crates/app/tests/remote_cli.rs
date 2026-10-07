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

#[cfg(unix)]
#[tokio::test]
async fn managed_desktop_starts_real_daemon_then_closes_it_with_native_host() {
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
    let output = cli(
        &root.path().join("sessions"),
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
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let url = String::from_utf8(output.stdout).unwrap();
    assert!(
        reqwest::get(url.trim()).await.is_err(),
        "daemon should close when native host exits"
    );
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
            &json!({"sessionId":id,"text":"run through restored forward"}).to_string(),
        ],
    )
    .await;
    assert!(
        prompted.status.success(),
        "{}",
        String::from_utf8_lossy(&prompted.stderr)
    );
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
