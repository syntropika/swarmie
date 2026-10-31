#![cfg(unix)]

use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

// Each case starts fresh processes that compile the offline inspection corpus.
// Isolate unrelated cold starts so readiness deadlines measure the tested host,
// while each case retains its own multi-process ownership/concurrency scenario.
fn isolate_startup() -> std::sync::MutexGuard<'static, ()> {
    static STARTUP: std::sync::Mutex<()> = std::sync::Mutex::new(());
    // A failed case must not prevent independent cases from running.
    STARTUP
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct Owner(Child);

impl Owner {
    fn start(profile: &Path) -> Self {
        Self(
            Command::new(env!("CARGO_BIN_EXE_swarmie-node"))
                .arg("--profile")
                .arg(profile)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        )
    }

    fn endpoint(&mut self, profile: &Path) -> Value {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                let mut error = String::new();
                self.0
                    .stderr
                    .as_mut()
                    .unwrap()
                    .read_to_string(&mut error)
                    .unwrap();
                panic!("node failed to start: {status}: {error}");
            }
            if let Ok(bytes) = fs::read(profile.join("node-endpoint.json")) {
                return serde_json::from_slice(&bytes).unwrap();
            }
            assert!(
                Instant::now() < deadline,
                "node did not publish its endpoint"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn shutdown(&mut self) {
        // The test owns this live child. SIGTERM exercises its explicit host
        // shutdown path rather than treating a client disconnect as shutdown.
        assert_eq!(
            unsafe { libc::kill(self.0.id() as libc::pid_t, libc::SIGTERM) },
            0
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                assert!(status.success(), "node shutdown failed: {status}");
                return;
            }
            assert!(
                Instant::now() < deadline,
                "node did not finish physical shutdown"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn connect(endpoint: &Value) -> TcpStream {
    let mut stream = TcpStream::connect(endpoint["address"].as_str().unwrap()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .write_all(
            Uuid::parse_str(endpoint["token"].as_str().unwrap())
                .unwrap()
                .as_bytes(),
        )
        .unwrap();
    let mut acknowledged = [0];
    stream.read_exact(&mut acknowledged).unwrap();
    assert_eq!(acknowledged, [1]);
    stream
}

fn assert_closed(stream: &mut TcpStream) {
    let mut byte = [0];
    match stream.read(&mut byte) {
        Ok(0) => {}
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
            ) => {}
        other => panic!("attachment did not close: {other:?}"),
    }
}

fn frame(stream: &mut TcpStream, value: Value) -> Value {
    let expected = value["id"].clone();
    send(stream, value);
    loop {
        let mut prefix = [0; 4];
        stream.read_exact(&mut prefix).unwrap();
        let length = u32::from_le_bytes(prefix) as usize;
        assert!(length <= swarmie_node::MAX_FRAME_BYTES);
        let mut bytes = vec![0; length];
        stream.read_exact(&mut bytes).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        if value["id"] == expected {
            return value;
        }
    }
}

fn send(stream: &mut TcpStream, value: Value) {
    let bytes = serde_json::to_vec(&value).unwrap();
    stream
        .write_all(&(bytes.len() as u32).to_le_bytes())
        .unwrap();
    stream.write_all(&bytes).unwrap();
}

fn get(stream: &mut TcpStream, service: &str, key: &str) -> Value {
    let result = frame(
        stream,
        json!({"id":11,"op":"get","payload":{"service":service,"key":[key]}}),
    );
    assert!(result.get("error").is_none(), "{result}");
    result["ok"].clone()
}

fn call(stream: &mut TcpStream, object: &Value, method: &str, arguments: Value) -> Value {
    let result = frame(
        stream,
        json!({"id":12,"op":"call","payload":{"reference":object["reference"],"method":method,"arguments":arguments}}),
    );
    assert!(result.get("error").is_none(), "{result}");
    result["ok"].clone()
}

fn release(stream: &mut TcpStream, object: &Value) {
    let result = frame(
        stream,
        json!({"id":13,"op":"release","payload":{"reference":object["reference"]}}),
    );
    assert!(result.get("error").is_none(), "{result}");
}

fn snapshot(stream: &mut TcpStream) -> Value {
    let object = get(stream, "swarmie.conversation.session", "process-chat");
    call(stream, &object, "snapshot", json!([]));
    release(stream, &object);
    let current = get(stream, "swarmie.conversation.session", "process-chat");
    let state = current["state"].clone();
    release(stream, &current);
    state
}

fn identity(stream: &mut TcpStream) -> Value {
    let reference = frame(
        stream,
        json!({"id":1,"op":"get","payload":{"service":"swarmie.node","key":["main"]}}),
    );
    assert!(reference.get("error").is_none(), "{reference}");
    let response = frame(
        stream,
        json!({"id":2,"op":"call","payload":{"reference":reference["ok"]["reference"],"method":"identify","arguments":[]}}),
    );
    assert!(response.get("error").is_none(), "{response}");
    response["ok"].clone()
}

#[test]
fn independent_owner_survives_client_detach_and_reuses_identity_after_shutdown() {
    let _fixture = isolate_startup();
    let profile = tempfile::tempdir().unwrap();
    let mut owner = Owner::start(profile.path());
    let endpoint = owner.endpoint(profile.path());
    assert_eq!(
        fs::metadata(profile.path().join("node-endpoint.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let first = identity(&mut connect(&endpoint));
    assert_eq!(first["nodeId"], endpoint["node"]);
    assert_eq!(first["principal"], "desktop-user");
    // The first client has fully closed its socket. The independent process is
    // still live and a different client observes the same real identity.
    assert!(owner.0.try_wait().unwrap().is_none());
    let mut second = connect(&endpoint);
    assert_eq!(identity(&mut second)["nodeId"], first["nodeId"]);
    owner.shutdown();
    assert!(!profile.path().join("node-endpoint.json").exists());
    drop(second);
    let mut restarted = Owner::start(profile.path());
    let next = restarted.endpoint(profile.path());
    assert_eq!(next["node"], endpoint["node"]);
    assert_ne!(next["token"], endpoint["token"]);
    assert_eq!(identity(&mut connect(&next))["nodeId"], first["nodeId"]);
    restarted.shutdown();
}

#[test]
fn unauthenticated_and_private_requests_cannot_borrow_operator_authority() {
    let _fixture = isolate_startup();
    let profile = tempfile::tempdir().unwrap();
    let mut owner = Owner::start(profile.path());
    let endpoint = owner.endpoint(profile.path());
    let mut unauthenticated = TcpStream::connect(endpoint["address"].as_str().unwrap()).unwrap();
    unauthenticated
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    unauthenticated.write_all(&[0; 16]).unwrap();
    assert_closed(&mut unauthenticated);
    let mut public = connect(&endpoint);
    let response = frame(
        &mut public,
        json!({"id":1,"op":"get","payload":{"service":"swarmie.vault.control","key":["main"],"principal":"desktop-shell"}}),
    );
    assert_eq!(response["error"]["code"], "forbidden");
    assert_eq!(identity(&mut public)["principal"], "desktop-user");
    owner.shutdown();
}

#[test]
fn another_host_is_refused_without_replacing_the_live_profile_owner() {
    let _fixture = isolate_startup();
    let profile = tempfile::tempdir().unwrap();
    let mut first = Owner::start(profile.path());
    let endpoint = first.endpoint(profile.path());
    let mut other = Owner::start(profile.path());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = other.0.try_wait().unwrap() {
            assert!(!status.success());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "duplicate owner did not refuse profile"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        serde_json::from_slice::<Value>(
            &fs::read(profile.path().join("node-endpoint.json")).unwrap()
        )
        .unwrap(),
        endpoint
    );
    assert_eq!(
        identity(&mut connect(&endpoint))["nodeId"],
        endpoint["node"]
    );
    first.shutdown();
}

#[test]
fn oversized_input_closes_only_its_attachment() {
    let _fixture = isolate_startup();
    let profile = tempfile::tempdir().unwrap();
    let mut owner = Owner::start(profile.path());
    let endpoint = owner.endpoint(profile.path());
    let mut invalid = connect(&endpoint);
    invalid
        .write_all(&((swarmie_node::MAX_FRAME_BYTES + 1) as u32).to_le_bytes())
        .unwrap();
    assert_closed(&mut invalid);
    assert_eq!(
        identity(&mut connect(&endpoint))["nodeId"],
        endpoint["node"]
    );
    owner.shutdown();
}

#[test]
fn an_unrecognized_endpoint_file_is_preserved() {
    let _fixture = isolate_startup();
    let profile = tempfile::tempdir().unwrap();
    let path = profile.path().join("node-endpoint.json");
    fs::write(&path, b"user-owned content").unwrap();
    let mut owner = Owner::start(profile.path());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = owner.0.try_wait().unwrap() {
            assert!(!status.success());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "unrecognized endpoint was not refused"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(fs::read(path).unwrap(), b"user-owned content");
    assert!(!profile.path().join("node-id").exists());
    assert!(!profile.path().join("vault").exists());
}

#[test]
fn crash_releases_exclusive_ownership_and_rotates_the_stale_endpoint() {
    let _fixture = isolate_startup();
    let profile = tempfile::tempdir().unwrap();
    let mut owner = Owner::start(profile.path());
    let previous = owner.endpoint(profile.path());
    owner.0.kill().unwrap();
    owner.0.wait().unwrap();
    assert!(profile.path().join("node-endpoint.json").exists());
    let mut restarted = Owner::start(profile.path());
    let deadline = Instant::now() + Duration::from_secs(15);
    let endpoint = loop {
        let value = restarted.endpoint(profile.path());
        if value["token"] != previous["token"] {
            break value;
        }
        assert!(Instant::now() < deadline, "stale endpoint was not replaced");
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(endpoint["node"], previous["node"]);
    assert_eq!(
        identity(&mut connect(&endpoint))["nodeId"],
        previous["node"]
    );
    restarted.shutdown();
}

#[test]
#[ignore = "subprocess fixture for a client that exits after confirmed admission"]
fn admitted_client_process() {
    let path = std::env::var_os("SWARMIE_NODE_TEST_ENDPOINT").expect("fixture endpoint");
    let endpoint: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let submission: Value =
        serde_json::from_str(&std::env::var("SWARMIE_NODE_TEST_SUBMISSION").unwrap()).unwrap();
    let mut client = connect(&endpoint);
    let object = get(&mut client, "swarmie.conversation.session", "process-chat");
    send(
        &mut client,
        json!({"id":20,"op":"call","payload":{"reference":object["reference"],"method":"run","arguments":[submission]}}),
    );
    let mut observer = connect(&endpoint);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let state = snapshot(&mut observer);
        if state["admitted"]
            .as_array()
            .is_some_and(|ids| ids.iter().any(|id| id == "process-submission"))
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "client could not observe admission"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn admitted_work_survives_real_client_process_exit_without_replaying_on_restart() {
    let _fixture = isolate_startup();
    let profile = tempfile::tempdir().unwrap();
    let mut owner = Owner::start(profile.path());
    let endpoint = owner.endpoint(profile.path());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (entered, pending) = mpsc::channel();
    let (finish, released) = mpsc::channel();
    let dispatches = Arc::new(AtomicUsize::new(0));
    let count = dispatches.clone();
    let server = thread::spawn(move || {
        for index in 0..2 {
            let deadline = Instant::now() + Duration::from_secs(15);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "fixture provider request timed out"
                        );
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let (end, size) = loop {
                let mut bytes = [0; 4096];
                let n = stream.read(&mut bytes).unwrap();
                assert!(n > 0);
                request.extend_from_slice(&bytes[..n]);
                assert!(request.len() <= 128 * 1024);
                if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    let size = String::from_utf8_lossy(&request[..end])
                        .lines()
                        .find_map(|line| {
                            line.split_once(':')
                                .filter(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                                .map(|(_, value)| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    break (end + 4, size);
                }
            };
            while request.len() < end + size {
                let mut bytes = [0; 4096];
                let n = stream.read(&mut bytes).unwrap();
                assert!(n > 0);
                request.extend_from_slice(&bytes[..n]);
            }
            let body = if index == 0 {
                assert!(request.starts_with(b"GET /api/tags "));
                json!({"models":[{
                    "name":"fixture-model",
                    "model":"fixture-model",
                    "limit":{"context":128000,"input":120000,"output":8192},
                    "swarmie_capabilities":{
                        "profile":"swarmie.text-tools.v1",
                        "tools":true,
                        "streaming":true
                    }
                }]})
                .to_string()
            } else {
                assert!(request.starts_with(b"POST /api/chat "));
                count.fetch_add(1, Ordering::SeqCst);
                entered.send(()).unwrap();
                released.recv_timeout(Duration::from_secs(15)).unwrap();
                format!(
                    "{}\n{}\n",
                    json!({"model":"fixture-model","created_at":"2026-01-01T00:00:00Z","message":{"role":"assistant","content":"Standalone finished."},"done":false}),
                    json!({"model":"fixture-model","created_at":"2026-01-01T00:00:00Z","message":{"role":"assistant","content":""},"done":true,"done_reason":"stop","prompt_eval_count":3,"eval_count":5})
                )
            };
            write!(stream,"HTTP/1.1 200 OK\r\ncontent-type: application/x-ndjson\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
    });
    let mut client = connect(&endpoint);
    let settings = get(&mut client, "swarmie.providers.settings", "main");
    let account = call(
        &mut client,
        &settings,
        "configure",
        json!([{"providerId":"ollama","method":"local","url":url}]),
    );
    release(&mut client, &settings);
    let submission = json!({"conversationId":"process-chat","submissionId":"process-submission","createConversation":true,"draft":{"text":"Hello","attachments":[],"account":account["id"],"model":"fixture-model","effort":"","node":endpoint["node"]},"mode":"queue"});
    let mut admitted = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "admitted_client_process"])
        .env(
            "SWARMIE_NODE_TEST_ENDPOINT",
            profile.path().join("node-endpoint.json"),
        )
        .env("SWARMIE_NODE_TEST_SUBMISSION", submission.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    pending.recv_timeout(Duration::from_secs(15)).unwrap();
    assert!(admitted.wait().unwrap().success());
    assert!(
        owner.0.try_wait().unwrap().is_none(),
        "independent owner exited with its client"
    );
    finish.send(()).unwrap();
    server.join().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let state = snapshot(&mut client);
        if state["phase"] == "completed" {
            assert_eq!(state["admitted"], json!(["process-submission"]));
            assert_eq!(state["messages"][1]["text"], "Standalone finished.");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "admitted operation did not settle: {state}"
        );
        thread::sleep(Duration::from_millis(10));
    }
    owner.shutdown();
    drop(client);
    let mut restarted = Owner::start(profile.path());
    let endpoint = restarted.endpoint(profile.path());
    let mut client = connect(&endpoint);
    assert_eq!(snapshot(&mut client)["phase"], "completed");
    let object = get(&mut client, "swarmie.conversation.session", "process-chat");
    assert_eq!(
        call(&mut client, &object, "run", json!([submission]))["status"],
        "settled"
    );
    assert_eq!(dispatches.load(Ordering::SeqCst), 1);
    restarted.shutdown();
}
