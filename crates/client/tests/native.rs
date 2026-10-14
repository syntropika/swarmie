use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use serde_json::{Value, json};
use swarmie_client::execute;

struct Server {
    url: String,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
    cancellation_started: mpsc::Receiver<()>,
}

impl Server {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let (cancellation_ready, cancellation_started) = mpsc::channel();
        let worker = thread::spawn(move || {
            let mut requests = Vec::new();
            while !stopped.load(Ordering::Relaxed) {
                if let Ok((stream, _)) = listener.accept() {
                    let cancellation_ready = cancellation_ready.clone();
                    requests.push(thread::spawn(move || {
                        // Accepted sockets can inherit the listener's nonblocking mode.
                        // This handler uses bounded blocking reads, including partial headers.
                        stream.set_nonblocking(false).unwrap();
                        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                        let mut reader = BufReader::new(stream);
                        let mut length = 0;
                        loop {
                            let mut line = String::new();
                            if reader.read_line(&mut line).unwrap_or(0) == 0 { return; }
                            if line == "\r\n" { break; }
                            if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") { length = value.trim().parse().unwrap(); }
                        }
                        let mut bytes = vec![0; length];
                        reader.read_exact(&mut bytes).unwrap();
                        let request: Value = serde_json::from_slice(&bytes).unwrap();
                        let method = request["method"].as_str().unwrap();
                        let result = match method {
                            "large" => json!("x".repeat(2048)),
                            "session/transcript" => match request["params"]["sessionId"].as_str().unwrap() {
                                "unordered" => json!([{"sessionId":"unordered","sequence":2,"type":"turn.started"},{"sessionId":"unordered","sequence":1,"type":"turn.ended"}]),
                                "ended" => json!([{"sessionId":"ended","sequence":1,"type":"session.ended"}]),
                                "turn" => json!([{"sessionId":"turn","sequence":1,"type":"turn.started"},{"sessionId":"turn","sequence":2,"type":"turn.ended"}]),
                                _ => json!([]),
                            },
                            _ => Value::Null,
                        };
                        let id = if method == "wrong" { json!("wrong") } else { request["id"].clone() };
                        let body = json!({"jsonrpc":"2.0","id":id,"result":result}).to_string();
                        let mut stream = reader.into_inner();
                        if method == "redirect" {
                            let _ = stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                            return;
                        }
                        let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                        if method == "cancel" {
                            // Signal a received request, then hold the body until the caller disconnects.
                            cancellation_ready.send(()).unwrap();
                            let _ = stream.read(&mut [0]);
                            return;
                        }
                        if method == "slow" { thread::sleep(Duration::from_millis(100)); }
                        let _ = stream.write_all(body.as_bytes());
                    }));
                } else {
                    thread::sleep(Duration::from_millis(1));
                }
            }
            for request in requests {
                request.join().unwrap();
            }
        });
        Self {
            url,
            stop,
            worker: Some(worker),
            cancellation_started,
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.worker.take().unwrap().join().unwrap();
    }
}

fn command(value: Value) -> Value {
    serde_json::from_str(&execute(&value.to_string())).unwrap()
}

fn release(handle: &Value) {
    command(json!({"action":"release","handle":handle}));
}

#[test]
fn native_http_bounds_redirects_errors_and_concurrent_cancellation() {
    let server = Server::new();
    let client = command(json!({"action":"create_http","config":{"url":server.url,"token":null,"timeout_ms":5000,"max_response_bytes":1024}}))["ok"].clone();
    let operation = command(json!({"action":"prepare"}))["ok"].clone();
    for (method, expected) in [
        ("null", None),
        ("wrong", Some("protocol")),
        ("large", Some("protocol")),
        ("redirect", Some("protocol")),
        ("slow", Some("timeout")),
    ] {
        let timeout_ms = if method == "slow" { Some(50) } else { None };
        let result = command(
            json!({"action":"invoke","client":client,"operation":operation,"request":{"method":method,"params_json":"{}","timeout_ms":timeout_ms}}),
        );
        if let Some(expected) = expected {
            assert_eq!(result["error"]["kind"], expected, "{result}");
        } else {
            assert_eq!(result["ok"], "null", "{result}");
        }
    }
    let call = json!({"action":"invoke","client":client,"operation":operation,"request":{"method":"cancel","params_json":"{}","timeout_ms":null}});
    let worker = thread::spawn(move || command(call));
    server
        .cancellation_started
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    // Exercise a delayed cancel without racing the separate 50 ms deadline check.
    thread::sleep(Duration::from_millis(60));
    command(json!({"action":"cancel","operation":operation}));
    assert_eq!(worker.join().unwrap()["error"]["kind"], "cancelled");
    release(&operation);
    release(&client);
}

#[test]
fn native_event_validation_deadline_and_turn_completion_share_the_engine() {
    let server = Server::new();
    let client = command(json!({"action":"create_http","config":{"url":server.url,"token":null,"timeout_ms":1000,"max_response_bytes":1024}}))["ok"].clone();
    let operation = command(json!({"action":"prepare"}))["ok"].clone();
    for (session, kind) in [
        ("empty", Some("timeout")),
        ("unordered", Some("protocol")),
        ("ended", Some("protocol")),
        ("turn", None),
    ] {
        let timeout_ms = if session == "empty" { 25 } else { 1000 };
        let result = command(
            json!({"action":"wait_turn","client":client,"operation":operation,"session_id":session,"options":{"after_sequence":0,"timeout_ms":timeout_ms,"poll_interval_ms":2}}),
        );
        if let Some(kind) = kind {
            assert_eq!(result["error"]["kind"], kind, "{result}");
        } else {
            let events: Vec<Value> = serde_json::from_str(result["ok"].as_str().unwrap()).unwrap();
            assert_eq!(events.len(), 2);
        }
    }
    let stream = command(json!({"action":"open_events","client":client,"session_id":"turn","options":{"after_sequence":0,"timeout_ms":10,"poll_interval_ms":2}}))["ok"].clone();
    thread::sleep(Duration::from_millis(15));
    assert_eq!(
        command(json!({"action":"next_event","stream":stream,"operation":operation}))["error"]["kind"],
        "timeout"
    );
    release(&stream);
    release(&operation);
    release(&client);
}

#[test]
fn http_fixture_waits_for_fragmented_request_headers() {
    let server = Server::new();
    let mut stream = TcpStream::connect(server.url.strip_prefix("http://").unwrap()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream.write_all(b"POST /rpc HTTP/1.1\r\n").unwrap();
    // A valid request may pause between header fragments while the peer is scheduled.
    thread::sleep(Duration::from_millis(20));
    let body = json!({"jsonrpc":"2.0","id":"fragmented","method":"null","params":{}}).to_string();
    write!(
        stream,
        "Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
    let (_, body) = response.split_once("\r\n\r\n").unwrap();
    let envelope: Value = serde_json::from_str(body).unwrap();
    assert_eq!(envelope["id"], "fragmented");
    assert_eq!(envelope["result"], Value::Null);
}
