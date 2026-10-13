#![cfg(unix)]
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{path::{Path, PathBuf}, process::Stdio, time::Duration};
use tokio::{io::{AsyncBufReadExt, AsyncRead, AsyncWrite, BufReader}, process::{Child, Command}};
use tokio_tungstenite::{WebSocketStream, client_async, tungstenite::Message};

struct Daemon { child: Child, url: String, state: PathBuf, assets: bool }
impl Daemon {
    async fn start(state: &Path, assets: bool, path: Option<&str>, ledger: Option<&Path>) -> Self {
        std::fs::create_dir_all(state).unwrap();
        std::fs::write(state.join("index.html"), "<main>Node fixture</main>").unwrap();
        let script = state.join("script.json");
        std::fs::write(&script, json!({"turns":[{"expectedText":"survive disconnect","steps":[{"type":"sleep","milliseconds":500},{"type":"text","text":"still completed"},{"type":"end","outcome":"completed"}]}]}).to_string()).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_swarmie"));
        command.args(["--state-dir", state.to_str().unwrap(), "serve", "--http", "127.0.0.1:0", "--token", "private-http-token", "--mock-script", script.to_str().unwrap()]);
        if assets { command.args(["--desktop-assets", state.to_str().unwrap()]); }
        if let Some(path) = path { command.env("PATH", path); }
        if let Some(ledger) = ledger { command.env("SWARMIE_SSH_FIXTURE_LOG", ledger); }
        let mut child = command.stderr(Stdio::piped()).kill_on_drop(true).spawn().unwrap();
        let mut lines = BufReader::new(child.stderr.take().unwrap()).lines();
        let url = tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let line = lines.next_line().await.unwrap().expect("runtime exited before readiness");
                if let Some(url) = line.strip_prefix("HTTP listening on ") { break url.to_owned(); }
            }
        }).await.unwrap();
        Self { child, url, state: state.to_owned(), assets }
    }
    fn socket(&self) -> PathBuf {
        let endpoint: Value = serde_json::from_slice(&std::fs::read(self.state.join("runtime/http-endpoint.json")).unwrap()).unwrap();
        PathBuf::from(endpoint["localSocket"].as_str().unwrap())
    }
    async fn rpc(&self, path: &str, method: &str, params: Value, target: Option<&Value>) -> Value {
        let origin = self.url.trim_end_matches("/rpc");
        let mut request = json!({"jsonrpc":"2.0","id":"test","method":method,"params":params});
        if let Some(target) = target { request["expectedRuntime"] = target.clone(); }
        let mut builder = reqwest::Client::new().post(format!("{origin}{path}")).bearer_auth("private-http-token").json(&request);
        if self.assets { builder = builder.header("origin", origin); }
        builder.send().await.unwrap().error_for_status().unwrap().json().await.unwrap()
    }
    async fn target(&self) -> Value { self.rpc("/rpc", "runtime/initialize", json!({}), None).await["result"]["runtimeIdentity"].clone() }
}
async fn socket_rpc<S: AsyncRead + AsyncWrite + Unpin>(socket: &mut WebSocketStream<S>, id: u64, method: &str, params: Value, target: Option<&Value>) -> Value {
    let mut request = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
    if let Some(target) = target { request["expectedRuntime"] = target.clone(); }
    socket.send(Message::Text(request.to_string().into())).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(Ok(Message::Text(text))) = socket.next().await {
                let value: Value = serde_json::from_str(&text).unwrap();
                if value["id"] == id { return value; }
            } else { panic!("socket closed before response"); }
        }
    }).await.unwrap()
}
fn result(response: Value) -> Value { assert!(response.get("error").is_none(), "{response}"); response["result"].clone() }
async fn cli(state: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_swarmie")).args(["--state-dir", state.to_str().unwrap()]).args(args).env_remove("SWARMIE_TOKEN").output().await.unwrap()
}

#[tokio::test]
async fn private_socket_and_transparent_stdio_proxy_share_identity_and_preserve_running_turns() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let mut daemon = Daemon::start(&temp.path().join("owner"), false, None, None).await;
    let metadata = std::fs::metadata(daemon.socket()).unwrap();
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    let stream = tokio::net::UnixStream::connect(daemon.socket()).await.unwrap();
    let (mut direct, _) = client_async("ws://localhost/ws", stream).await.unwrap();
    let target = result(socket_rpc(&mut direct, 1, "runtime/initialize", json!({}), None).await)["runtimeIdentity"].clone();
    assert_eq!(target, daemon.target().await);
    let nonexistent_client_state = temp.path().join("proxy-client-no-state");
    let mut proxy = Command::new(env!("CARGO_BIN_EXE_swarmie")).args(["--state-dir", nonexistent_client_state.to_str().unwrap(), "runtime", "proxy", "--socket", daemon.socket().to_str().unwrap()]).stdin(Stdio::piped()).stdout(Stdio::piped()).kill_on_drop(true).spawn().unwrap();
    let joined = tokio::io::join(proxy.stdout.take().unwrap(), proxy.stdin.take().unwrap());
    let (mut socket, _) = client_async("ws://localhost/ws", joined).await.unwrap();
    assert_eq!(result(socket_rpc(&mut socket, 2, "runtime/initialize", json!({}), None).await)["runtimeIdentity"], target);
    let session = result(socket_rpc(&mut socket, 3, "session/create", json!({"harness":"mock","cwd":temp.path()}), Some(&target)).await);
    let id = session["id"].clone();
    result(socket_rpc(&mut socket, 4, "session/prompt", json!({"sessionId":id,"text":"survive disconnect","idempotencyKey":"socket-once"}), Some(&target)).await);
    socket.close(None).await.unwrap();
    drop(socket);
    let proxy_status = tokio::time::timeout(Duration::from_secs(3), proxy.wait()).await.unwrap().unwrap();
    assert!(proxy_status.success(), "a clean proxy disconnect must exit successfully: {proxy_status}");
    assert!(!nonexistent_client_state.exists(), "proxy must not create or own runtime state");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let events = result(socket_rpc(&mut direct, 5, "session/events", json!({"sessionId":id,"afterSequence":0}), Some(&target)).await);
            if events.as_array().unwrap().iter().any(|e| e["type"] == "turn.ended") { break; }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await.unwrap();
    assert!(daemon.child.try_wait().unwrap().is_none());
    result(socket_rpc(&mut direct, 6, "runtime/shutdown", json!({}), Some(&target)).await);
    tokio::time::timeout(Duration::from_secs(5), daemon.child.wait()).await.unwrap().unwrap();
    assert!(!daemon.state.join("runtime/node.sock").exists());
}

fn ssh_fixture(root: &Path) -> (String, PathBuf) {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let bin = root.join("bin"); std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("ssh"), include_str!("fixtures/ssh_proxy.py")).unwrap();
    std::fs::set_permissions(bin.join("ssh"), std::fs::Permissions::from_mode(0o700)).unwrap();
    symlink(env!("CARGO_BIN_EXE_swarmie"), bin.join("swarmie")).unwrap();
    (format!("{}:{}", bin.display(), std::env::var("PATH").unwrap()), root.join("ssh.jsonl"))
}
fn proxy_pids(ledger: &Path) -> Vec<i32> {
    std::fs::read_to_string(ledger).unwrap_or_default().lines().filter_map(|line| {
        let value: Value = serde_json::from_str(line).unwrap();
        value["command"].as_str().unwrap().contains("runtime proxy").then(|| value["pid"].as_i64().unwrap() as i32)
    }).collect()
}

#[tokio::test]
async fn native_ssh_proxy_reconnects_without_replaying_and_reaps_children_on_route_removal() {
    let temp = tempfile::tempdir().unwrap();
    let (path, ledger) = ssh_fixture(temp.path());
    let mut remote = Daemon::start(&temp.path().join("remote node's state"), false, None, None).await;
    let gateway = Daemon::start(&temp.path().join("client"), true, Some(&path), Some(&ledger)).await;
    let local_target = gateway.target().await;
    let readiness = result(gateway.rpc("/client/rpc", "nodes/inspectSsh", json!({"host":"fixture-host","socketPath":remote.socket()}), Some(&local_target)).await);
    assert_eq!(readiness["ready"], true);
    let mut stale = readiness["runtimeIdentity"].clone(); stale["incarnationId"] = "stale".into();
    let rejected = gateway.rpc("/client/rpc", "nodes/pairSsh", json!({"name":"stale","host":"fixture-host","socketPath":remote.socket(),"runtimeIdentity":stale}), Some(&local_target)).await;
    assert!(rejected["error"]["message"].as_str().unwrap().contains("readiness verification"));
    let route = result(gateway.rpc("/client/rpc", "nodes/pairSsh", json!({"name":"ssh","host":"fixture-host","socketPath":remote.socket(),"runtimeIdentity":readiness["runtimeIdentity"]}), Some(&local_target)).await);
    let node_path = format!("/nodes/{}/rpc", route["identity"]["environmentId"].as_str().unwrap());
    let target = result(gateway.rpc(&node_path, "runtime/initialize", json!({}), None).await)["runtimeIdentity"].clone();
    assert_eq!(target, remote.target().await);
    let session = result(gateway.rpc(&node_path, "session/create", json!({"harness":"mock","cwd":temp.path()}), Some(&target)).await);
    result(gateway.rpc(&node_path, "session/prompt", json!({"sessionId":session["id"],"text":"survive disconnect","idempotencyKey":"ssh-once"}), Some(&target)).await);
    let pid = *proxy_pids(&ledger).last().unwrap();
    assert_eq!(unsafe { libc::kill(pid, libc::SIGKILL) }, 0);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let reconnected = result(gateway.rpc(&node_path, "runtime/initialize", json!({}), None).await);
    assert_eq!(reconnected["runtimeIdentity"], target);
    assert_ne!(*proxy_pids(&ledger).last().unwrap(), pid);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let transcript = result(gateway.rpc(&node_path, "session/transcript", json!({"sessionId":session["id"]}), Some(&target)).await);
            if transcript.as_array().unwrap().iter().any(|e| e["type"] == "turn.ended") {
                assert_eq!(transcript.as_array().unwrap().iter().filter(|e| e["type"] == "message.user").count(), 1);
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await.unwrap();
    remote.child.kill().await.unwrap(); remote.child.wait().await.unwrap();
    let retained = remote.state.clone();
    std::fs::rename(&retained, temp.path().join("original-retained-state")).unwrap();
    let replacement = Daemon::start(&retained, false, None, None).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let verified = gateway.rpc(&node_path, "runtime/initialize", json!({}), None).await;
            if verified["error"]["message"].as_str().is_some_and(|error| error.contains("environment identity changed")) { break; }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }).await.unwrap();
    let rejected = gateway.rpc(&node_path, "project/create", json!({"name":"must not exist"}), Some(&target)).await;
    assert_eq!(rejected["error"]["data"]["effectKnowledge"], "none");
    assert!(rejected["error"]["message"].as_str().unwrap().contains("environment identity changed"));
    assert_eq!(result(replacement.rpc("/rpc", "project/list", json!({}), Some(&replacement.target().await)).await), json!([]));
    result(gateway.rpc("/client/rpc", "nodes/removeRoute", json!({"name":"ssh","revision":route["revision"]}), Some(&local_target)).await);
    tokio::time::timeout(Duration::from_secs(4), async {
        while proxy_pids(&ledger).iter().any(|pid| unsafe { libc::kill(*pid, 0) } == 0) { tokio::time::sleep(Duration::from_millis(20)).await; }
    }).await.unwrap();
    assert!(replacement.child.id().is_some());
}

#[tokio::test]
async fn ssh_readiness_reports_missing_install_without_creating_or_starting_runtime_state() {
    let temp = tempfile::tempdir().unwrap(); let (path, ledger) = ssh_fixture(temp.path());
    let client = temp.path().join("client"); let remote = temp.path().join("not-installed");
    let output = Command::new(env!("CARGO_BIN_EXE_swarmie")).args(["--state-dir", client.to_str().unwrap(), "remote", "inspect-ssh", "fixture-host", "--socket", remote.join("node.sock").to_str().unwrap()]).env("PATH", path).env("SWARMIE_SSH_FIXTURE_LOG", ledger).env("SWARMIE_SSH_FIXTURE_INSTALLED", "0").output().await.unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let status: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status["installed"], false); assert_eq!(status["nextAction"], "install-runtime"); assert!(!remote.exists());
}

struct OwnerCleanup(PathBuf);
impl Drop for OwnerCleanup {
    fn drop(&mut self) { let _ = std::process::Command::new(env!("CARGO_BIN_EXE_swarmie")).args(["--state-dir", self.0.to_str().unwrap(), "runtime", "stop"]).output(); }
}

#[tokio::test]
async fn long_state_directory_and_killed_owner_allow_guarded_native_reattach_with_retained_identity() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join("a".repeat(80)).join("b".repeat(80));
    let _cleanup = OwnerCleanup(state.clone());
    let mut original = Daemon::start(&state, false, None, None).await;
    let original_target = original.target().await;
    let socket_path = original.socket();
    assert!(socket_path.to_string_lossy().len() < 100);
    assert!(!socket_path.starts_with(&state));
    result(original.rpc("/rpc", "project/create", json!({"name":"retained"}), Some(&original_target)).await);
    original.child.kill().await.unwrap(); original.child.wait().await.unwrap();
    assert!(socket_path.exists());
    let status = Command::new(env!("CARGO_BIN_EXE_swarmie")).args(["--state-dir", state.to_str().unwrap(), "runtime", "status"]).env("TMPDIR", temp.path().join("different-temp")).env("XDG_RUNTIME_DIR", temp.path().join("different-runtime")).output().await.unwrap();
    assert!(status.status.success(), "{}", String::from_utf8_lossy(&status.stderr));
    assert_eq!(serde_json::from_slice::<Value>(&status.stdout).unwrap()["running"], false);
    let host = temp.path().join("host");
    std::fs::write(&host, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&host, std::fs::Permissions::from_mode(0o700)).unwrap();
    let attach = cli(&state, &["desktop", "--assets", state.to_str().unwrap(), "--host", host.to_str().unwrap()]).await;
    assert!(attach.status.success(), "{}", String::from_utf8_lossy(&attach.stderr));
    let status: Value = serde_json::from_slice(&cli(&state, &["runtime", "status"]).await.stdout).unwrap();
    assert_eq!(status["running"], true);
    let target = status["endpoint"]["runtimeIdentity"].clone();
    assert_eq!(target["identity"], original_target["identity"]);
    assert_ne!(target["incarnationId"], original_target["incarnationId"]);
    assert_eq!(status["endpoint"]["localSocket"], socket_path.to_string_lossy().as_ref());
    let socket = tokio::net::UnixStream::connect(&socket_path).await.unwrap(); let (mut socket, _) = client_async("ws://localhost/ws", socket).await.unwrap();
    let rejected = socket_rpc(&mut socket, 1, "project/create", json!({"name":"stale"}), Some(&original_target)).await;
    assert_eq!(rejected["error"]["data"]["effectKnowledge"], "none");
    let projects = result(socket_rpc(&mut socket, 2, "project/list", json!({}), Some(&target)).await);
    assert_eq!(projects[0]["name"], "retained");
    let stopped = cli(&state, &["runtime", "stop"]).await; assert!(stopped.status.success(), "{}", String::from_utf8_lossy(&stopped.stderr));
    assert!(!socket_path.exists());
}

#[tokio::test]
async fn ssh_readiness_distinguishes_unavailable_access_invalid_installation_and_stopped_runtime() {
    for (exit, version, expected) in [
        ("255", "swarmie 0.1.0", "ssh-unavailable"),
        ("2", "swarmie 0.1.0", "invalid-installation"),
        ("0", "unexpected shell banner", "invalid-installation"),
        ("0", &"x".repeat(300), "invalid-installation"),
        ("0", "swarmie 0.1.0", "runtime-unavailable"),
        ("0", "swarmie 0.1.0", "incompatible-runtime"),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let (path, ledger) = ssh_fixture(temp.path());
        let remote = temp.path().join("untouched-remote");
        let output = Command::new(env!("CARGO_BIN_EXE_swarmie"))
            .args(["--state-dir", temp.path().join("client").to_str().unwrap(), "remote", "inspect-ssh", "fixture-host", "--socket", remote.join("node.sock").to_str().unwrap()])
            .env("PATH", path).env("SWARMIE_SSH_FIXTURE_LOG", &ledger)
            .env("SWARMIE_SSH_FIXTURE_EXIT", exit).env("SWARMIE_SSH_FIXTURE_VERSION", version)
            .env("SWARMIE_SSH_FIXTURE_BAD_PROTOCOL", if expected == "incompatible-runtime" { "1" } else { "0" })
            .output().await.unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let status: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(status["status"], expected, "{status}");
        assert_eq!(status["ready"], false);
        assert!(status["runtimeIdentity"].is_null());
        assert_eq!(status["installed"], if matches!(expected, "runtime-unavailable" | "incompatible-runtime") { json!(true) } else { Value::Null });
        assert!(!remote.exists());
        if !matches!(expected, "runtime-unavailable" | "incompatible-runtime") {
            assert_eq!(std::fs::read_to_string(&ledger).unwrap().lines().count(), 1);
            assert!(status["version"].is_null());
        }
    }
}

#[tokio::test]
async fn ssh_readiness_reports_a_missing_client_without_claiming_the_remote_runtime_is_absent() {
    let temp = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_swarmie"))
        .args(["--state-dir", temp.path().join("client").to_str().unwrap(), "remote", "inspect-ssh", "remote-host"])
        .env("PATH", temp.path()).output().await.unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let status: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status["status"], "ssh-client-unavailable");
    assert_eq!(status["installed"], Value::Null);
    assert_eq!(status["ready"], false);
    assert!(status["runtimeIdentity"].is_null());
}

#[tokio::test]
async fn native_ssh_preparation_rejects_occupied_names_and_custom_sockets_before_remote_changes() {
    let temp = tempfile::tempdir().unwrap();
    let (path, ledger) = ssh_fixture(temp.path());
    let mut client = Daemon::start(&temp.path().join("client"), true, Some(&path), Some(&ledger)).await;
    let local = client.target().await;
    result(client.rpc("/client/rpc", "nodes/pair", json!({"name":"occupied","url":client.url,"token":"private-http-token"}), Some(&local)).await);
    let rejected = client.rpc("/client/rpc", "nodes/prepareSsh", json!({"name":"occupied","host":"fixture","action":"install"}), Some(&local)).await;
    assert!(rejected["error"]["message"].as_str().unwrap().contains("already in use"));
    let rejected = client.rpc("/client/rpc", "nodes/prepareSsh", json!({"name":"new","host":"fixture","socketPath":"/custom/socket","action":"install"}), Some(&local)).await;
    assert!(rejected["error"]["message"].as_str().unwrap().contains("default runtime"));
    let rejected = client.rpc("/client/rpc", "nodes/prepareSsh", json!({"name":"new","host":"fixture","action":"erase"}), Some(&local)).await;
    assert!(rejected["error"]["message"].as_str().unwrap().contains("explicit install or start"));
    assert!(!ledger.exists(), "rejected preparation must not execute SSH");
    result(client.rpc("/rpc", "runtime/shutdown", json!({}), Some(&local)).await);
    tokio::time::timeout(Duration::from_secs(5), client.child.wait()).await.unwrap().unwrap();
}
