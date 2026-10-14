use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use swarmie_client::execute;

struct Server {
    url: String,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Server {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let worker = thread::spawn(move || {
            let mut requests = Vec::new();
            while !stopped.load(Ordering::Relaxed) {
                if let Ok((stream, _)) = listener.accept() {
                    requests.push(thread::spawn(move || {
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
    let client = command(json!({"action":"create_http","config":{"url":server.url,"token":null,"timeout_ms":50,"max_response_bytes":1024}}))["ok"].clone();
    let operation = command(json!({"action":"prepare"}))["ok"].clone();
    for (method, expected) in [
        ("null", None),
        ("wrong", Some("protocol")),
        ("large", Some("protocol")),
        ("redirect", Some("protocol")),
        ("slow", Some("timeout")),
    ] {
        let result = command(
            json!({"action":"invoke","client":client,"operation":operation,"request":{"method":method,"params_json":"{}","timeout_ms":null}}),
        );
        if let Some(expected) = expected {
            assert_eq!(result["error"]["kind"], expected, "{result}");
        } else {
            assert_eq!(result["ok"], "null");
        }
    }
    let call = json!({"action":"invoke","client":client,"operation":operation,"request":{"method":"slow","params_json":"{}","timeout_ms":null}});
    let started = Instant::now();
    let worker = thread::spawn(move || command(call));
    thread::sleep(Duration::from_millis(10));
    command(json!({"action":"cancel","operation":operation}));
    assert_eq!(worker.join().unwrap()["error"]["kind"], "cancelled");
    assert!(started.elapsed() < Duration::from_millis(90));
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
        let result = command(
            json!({"action":"wait_turn","client":client,"operation":operation,"session_id":session,"options":{"after_sequence":0,"timeout_ms":25,"poll_interval_ms":2}}),
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
