use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use swarmie_model_contracts::*;
use swarmie_model_provider_genai::{
    Configuration, Connection, Credentials, GENERATION_CONTRACT, Limits, PROVIDER_CONTRACT,
    facet_with_credentials,
};
use tembra::{Host, core};
use tembra_native::TokioDriver;

pub enum Reply {
    Json(Value),
    Http(u16, String),
    Frames(Vec<(u64, String)>),
    Hold,
}

pub struct Server {
    pub url: String,
    pub requests: Arc<Mutex<Vec<Value>>>,
    pub disconnected: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Server {
    pub fn new(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/v1/", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let disconnected = Arc::new(AtomicBool::new(false));
        let closed = disconnected.clone();
        let worker = thread::spawn(move || {
            let mut replies = VecDeque::from(replies);
            while !stopping.load(Ordering::SeqCst) {
                let Ok((mut socket, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(2));
                    continue;
                };
                socket
                    .set_read_timeout(Some(Duration::from_millis(100)))
                    .unwrap();
                socket
                    .set_write_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let Some(request) = read_request(&mut socket) else {
                    continue;
                };
                seen.lock().unwrap().push(request);
                match replies.pop_front().expect("unexpected HTTP request") {
                    Reply::Json(v) => {
                        let body = v.to_string();
                        let response = format!(
                            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = socket.write_all(response.as_bytes());
                    }
                    Reply::Http(status, body) => {
                        let response = format!(
                            "HTTP/1.1 {status} Error\r\ncontent-type: application/json\r\nretry-after: 2\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = socket.write_all(response.as_bytes());
                    }
                    Reply::Frames(frames) => {
                        let content_type =
                            if frames.first().is_some_and(|(_, s)| s.starts_with("data:")) {
                                "text/event-stream"
                            } else {
                                "application/x-ndjson"
                            };
                        let _=socket.write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\nconnection: close\r\n\r\n").as_bytes());
                        for (delay, frame) in frames {
                            let until = Instant::now() + Duration::from_millis(delay);
                            while Instant::now() < until && !stopping.load(Ordering::SeqCst) {
                                thread::sleep(Duration::from_millis(2));
                            }
                            if stopping.load(Ordering::SeqCst)
                                || socket.write_all(frame.as_bytes()).is_err()
                            {
                                break;
                            }
                        }
                    }
                    Reply::Hold => {
                        let _=socket.write_all(b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n");
                        let _ = socket.write_all(
                            frame(json!({"choices":[{"delta":{"content":"partial"}}]})).as_bytes(),
                        );
                        let mut byte = [0];
                        while !stopping.load(Ordering::SeqCst) {
                            match socket.read(&mut byte) {
                                Ok(0) => {
                                    closed.store(true, Ordering::SeqCst);
                                    break;
                                }
                                Err(e)
                                    if matches!(
                                        e.kind(),
                                        std::io::ErrorKind::WouldBlock
                                            | std::io::ErrorKind::TimedOut
                                    ) => {}
                                _ => break,
                            }
                        }
                    }
                }
            }
        });
        Self {
            url,
            requests,
            disconnected,
            stop,
            worker: Some(worker),
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.worker.take().unwrap().join().unwrap();
    }
}
fn read_request(socket: &mut TcpStream) -> Option<Value> {
    let mut data = Vec::new();
    let mut buf = [0; 4096];
    loop {
        let n = socket.read(&mut buf).ok()?;
        if n == 0 {
            return None;
        }
        data.extend_from_slice(&buf[..n]);
        if let Some(end) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&data[..end]);
            let length = headers
                .lines()
                .find_map(|line| {
                    line.to_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|v| v.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if data.len() >= end + 4 + length {
                return Some(if length == 0 {
                    json!({"path":headers.lines().next().unwrap()})
                } else {
                    serde_json::from_slice(&data[end + 4..end + 4 + length]).unwrap()
                });
            }
        }
    }
}
pub fn frame(value: Value) -> String {
    format!("data: {value}\n\n")
}
pub fn done() -> String {
    "data: [DONE]\n\n".into()
}
pub fn completion(text: &str, finish: &str) -> Value {
    json!({"model":"fixture-model","choices":[{"index":0,"message":{"role":"assistant","content":text},"finish_reason":finish}],"usage":{"prompt_tokens":5,"completion_tokens":3,"total_tokens":8}})
}
pub fn configuration(url: &str) -> Configuration {
    Configuration {
        connections: vec![Connection {
            id: "primary".into(),
            provider: "openai".into(),
            models: vec!["fixture-model".into(), "second-model".into()],
            base_url: Some(url.into()),
            credential_env: Some("TEST_MODEL_CREDENTIAL".into()),
            timeout_ms: 2000,
            max_concurrent: 2,
            defaults: Default::default(),
        }],
        limits: Limits {
            batch_events: 4,
            batch_interval_ms: 5,
            ..Default::default()
        },
    }
}
pub fn request(id: &str) -> ModelRequest {
    ModelRequest {
        request_id: id.into(),
        model: ModelIdentity {
            connection: "primary".into(),
            model: "fixture-model".into(),
        },
        messages: vec![Message::text(Role::User, "hello")],
        tools: vec![],
        options: Default::default(),
    }
}
pub fn run<F, Fut>(test: F)
where
    F: FnOnce(Rc<TokioDriver>) -> Fut,
    Fut: Future<Output = ()>,
{
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(async {
            tokio::time::timeout(Duration::from_secs(12), test(driver.clone()))
                .await
                .expect("model acceptance did not settle");
        }));
}
pub async fn host(configuration: &Configuration, driver: Rc<TokioDriver>) -> Host {
    let host = Host::with_driver("models-test", "test", driver.clone()).unwrap();
    host.install(
        facet_with_credentials(configuration, driver, Rc::new(FixtureCredentials)).unwrap(),
        core::LoadOptions::default(),
    )
    .unwrap()
    .await
    .unwrap();
    host
}
pub async fn provider(host: &Host) -> ModelReference {
    host.get(
        ModelEndpoint::new(PROVIDER_CONTRACT).unwrap(),
        ModelProviderSchemaInput {},
    )
    .await
    .unwrap()
}
pub async fn generation(host: &Host, request: ModelRequest) -> GenerationReference {
    host.get(
        GenerationEndpoint::new(GENERATION_CONTRACT).unwrap(),
        GenerationInput { request },
    )
    .await
    .unwrap()
}
pub async fn settled(g: &GenerationReference) -> GenerationState {
    loop {
        let state = g.progress().value().unwrap();
        if state.physically_settled {
            return state;
        }
        g.acknowledge(state.last_sequence).await.unwrap();
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}
pub fn response(result: GenerateResult) -> ModelResponse {
    match result {
        GenerateResult::Response { response } => *response,
        GenerateResult::Error { error } => panic!("model error: {error:?}"),
    }
}
pub async fn wait_for(mut condition: impl FnMut() -> bool) {
    for _ in 0..500 {
        if condition() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    panic!("condition did not become true");
}

struct FixtureCredentials;
impl Credentials for FixtureCredentials {
    fn resolve(&self, _: &str) -> Option<String> {
        Some("fixture-private-key".into())
    }
}
