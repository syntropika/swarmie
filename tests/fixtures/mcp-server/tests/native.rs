use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use swarmie_desktop_runtime::DesktopNode;

const WAIT: Duration = Duration::from_secs(10);

struct Rpc<'a> {
    node: &'a DesktopNode,
    channel: u64,
    next: u64,
    inbox: VecDeque<Value>,
}

impl<'a> Rpc<'a> {
    fn new(node: &'a DesktopNode) -> Self {
        Self {
            node,
            channel: node.open_shared_channel().unwrap(),
            next: 1,
            inbox: VecDeque::new(),
        }
    }

    fn send(&mut self, op: &str, payload: Value) -> u64 {
        let id = self.next;
        self.next += 1;
        self.node
            .frame(
                self.channel,
                serde_json::to_vec(&json!({"id":id,"op":op,"payload":payload})).unwrap(),
            )
            .unwrap();
        id
    }

    fn response(&mut self, id: u64) -> Value {
        let deadline = Instant::now() + WAIT;
        loop {
            if let Some(index) = self.inbox.iter().position(|value| value["id"] == id) {
                return self.inbox.remove(index).unwrap();
            }
            self.poll();
            assert!(Instant::now() < deadline, "native RPC response timed out");
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn poll(&mut self) {
        for bytes in self.node.poll(self.channel).unwrap() {
            assert!(bytes.len() <= 64 * 1024);
            self.inbox
                .push_back(serde_json::from_slice(&bytes).unwrap());
        }
    }

    fn cancel_before_release(&mut self, id: u64) {
        self.send("cancel", json!({"id":id}));
        // Cancel changes the root synchronously in receive(). This native poll
        // follows that frame in the worker queue, before the effect is released.
        self.poll();
    }

    fn request(&mut self, op: &str, payload: Value) -> Value {
        let id = self.send(op, payload);
        self.response(id)
    }

    fn get(&mut self, service: &str, key: &str) -> Value {
        let result = self.request("get", json!({"service":service,"key":[key]}));
        assert!(result.get("error").is_none(), "{result}");
        result["ok"].clone()
    }

    fn start(&mut self, reference: &Value, method: &str, arguments: Value) -> u64 {
        self.send(
            "call",
            json!({"reference":reference,"method":method,"arguments":arguments}),
        )
    }

    fn call(&mut self, reference: &Value, method: &str, arguments: Value) -> Value {
        let id = self.start(reference, method, arguments);
        self.response(id)
    }
}

fn read_request(stream: &mut TcpStream) -> (String, Value) {
    stream.set_read_timeout(Some(WAIT)).unwrap();
    let mut data = Vec::new();
    let (end, length) = loop {
        let mut bytes = [0; 4096];
        let count = stream.read(&mut bytes).unwrap();
        assert!(count > 0);
        data.extend_from_slice(&bytes[..count]);
        if let Some(at) = data.windows(4).position(|part| part == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&data[..at]);
            let length = headers
                .lines()
                .find_map(|line| {
                    line.split_once(':')
                        .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                        .map(|(_, value)| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            let path = headers
                .lines()
                .next()
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap()
                .to_owned();
            break ((at + 4, path), length);
        }
    };
    while data.len() < end.0 + length {
        let mut bytes = [0; 4096];
        let count = stream.read(&mut bytes).unwrap();
        assert!(count > 0);
        data.extend_from_slice(&bytes[..count]);
    }
    let body = if length == 0 {
        Value::Null
    } else {
        serde_json::from_slice(&data[end.0..end.0 + length]).unwrap()
    };
    (end.1, body)
}

struct Provider {
    endpoint: String,
    requests: Arc<Mutex<Vec<(String, Value)>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Provider {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = thread::spawn(move || {
            let deadline = Instant::now() + WAIT;
            let mut models = 0;
            while !stopping.load(Ordering::SeqCst) {
                let mut stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "native provider fixture timed out"
                        );
                        thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    Err(error) => panic!("{error}"),
                };
                let request = read_request(&mut stream);
                let catalog = request.0.ends_with("/tags");
                captured.lock().unwrap().push(request);
                let (mime, body) = if catalog {
                    (
                        "application/json",
                        json!({"models":[{"name":"fixture-model"}]}).to_string(),
                    )
                } else {
                    models += 1;
                    let message = if models == 1 {
                        json!({"role":"assistant","content":"","tool_calls":[{"id":"call_fixture","function":{"name":"mcp__fixture__counter__add","arguments":{"amount":7}}}]})
                    } else {
                        json!({"role":"assistant","content":"Fresh response"})
                    };
                    let first = json!({"model":"fixture-model","message":message,"done":false});
                    let last = json!({"model":"fixture-model","message":{"role":"assistant","content":""},"done":true,"done_reason":if models==1 {"tool_calls"} else {"stop"},"prompt_eval_count":3,"eval_count":5});
                    ("application/x-ndjson", format!("{first}\n{last}\n"))
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 fixture\r\ncontent-type: {mime}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        Self {
            endpoint,
            requests,
            stop,
            worker: Some(worker),
        }
    }
}

impl Drop for Provider {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.worker.take().unwrap().join();
    }
}

fn configure(rpc: &mut Rpc<'_>, endpoint: &str, gate: &Path) -> Value {
    let providers = rpc.get("swarmie.providers.settings", "main");
    let configured = rpc.call(
        &providers["reference"],
        "configure",
        json!([{"providerId":"ollama","method":"local","url":endpoint}]),
    );
    assert!(configured.get("error").is_none(), "{configured}");
    let settings = rpc.get("swarmie.mcp.settings", "main");
    let saved=rpc.call(&settings["reference"],"save",json!([{"definition":{"id":"fixture","name":"Synthetic counter","scope":{"kind":"user"},"endpoint":{"kind":"local","program":env!("CARGO_BIN_EXE_swarmie-fixture-mcp-server"),"arguments":["--gate-effect",gate],"directory":null},"protocol":"2026-07-28","enabled":false,"revision":1},"expected_revision":null}]));
    assert!(saved.get("error").is_none(), "{saved}");
    let enabled = rpc.call(
        &settings["reference"],
        "enable",
        json!([{"id":"fixture","revision":saved["ok"]["revision"]}]),
    );
    assert!(enabled.get("error").is_none(), "{enabled}");
    assert_eq!(
        enabled["ok"]["observation"]["status"], "connected",
        "{enabled}"
    );
    configured["ok"].clone()
}

fn submission(
    node: &DesktopNode,
    account: &Value,
    conversation: &str,
    first: bool,
    mode: &str,
) -> Value {
    json!({"conversationId":conversation,"submissionId":if first {"original"} else {"fresh"},"createConversation":first,"draft":{"text":if first {"Use the counter"} else {"Fresh explicit message"},"attachments":[],"account":account["id"],"model":"fixture-model","effort":"","node":node.descriptor()["id"]},"mode":mode})
}

fn index(path: &Path, conversation: &str) -> Option<Value> {
    std::fs::read(path.join("conversations.json"))
        .ok()
        .map(|data| serde_json::from_slice::<Vec<Value>>(&data).unwrap())
        .and_then(|records| {
            records
                .into_iter()
                .find(|record| record["id"] == conversation)
        })
}

fn wait_until(message: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !ready() {
        assert!(Instant::now() < deadline, "{message}");
        thread::sleep(Duration::from_millis(1));
    }
}

fn interrupted_native_tool(kind: &str) {
    let provider = Provider::new();
    let state = tempfile::tempdir().unwrap();
    let gate = state.path().join("effect.json");
    let node = DesktopNode::open(state.path().into()).unwrap();
    let mut owner = Rpc::new(&node);
    let mut observer = Rpc::new(&node);
    let account = configure(&mut observer, &provider.endpoint, &gate);
    let active = owner.get("swarmie.conversation.session", kind);
    let observed = observer.get("swarmie.conversation.session", kind);
    let running = owner.start(
        &active["reference"],
        "run",
        json!([submission(&node, &account, kind, true, "queue")]),
    );
    wait_until("MCP tool did not reach the physical effect gate", || {
        std::fs::read(&gate)
            .ok()
            .and_then(|data| serde_json::from_slice::<Value>(&data).ok())
            .is_some()
    });
    let effect: Value = serde_json::from_slice(&std::fs::read(&gate).unwrap()).unwrap();
    assert_eq!(effect["effect_count"], 1);
    assert_ne!(effect["pid"], std::process::id());
    let action = match kind {
        "disconnect" => {
            node.close_channel(owner.channel);
            None
        }
        "stop" => Some(observer.start(&observed["reference"], "stop", json!([]))),
        "steer" => Some(observer.start(
            &observed["reference"],
            "run",
            json!([submission(&node, &account, kind, false, "steer")]),
        )),
        _ => unreachable!(),
    };
    if kind == "disconnect" {
        // The ordered poll confirms that Close cancelled the original root.
        assert_eq!(
            node.poll(owner.channel).unwrap_err(),
            "native channel ended"
        );
    } else {
        // These cases test reconciliation and fresh admission after a confirmed
        // transport cancellation. They do not establish when Stop/steer started.
        owner.cancel_before_release(running);
    }
    std::fs::write(gate.with_extension("release"), b"release").unwrap();
    if kind == "disconnect" {
        wait_until("cancelled native tool checkpoint was not persisted", || {
            index(state.path(), kind).is_some_and(|record| {
                record["phase"] == "unknown" && !record["checkpoint"].is_null()
            })
        });
        let checkpoint = index(state.path(), kind).unwrap()["checkpoint"].clone();
        assert!(checkpoint.to_string().contains("mcp.fixture.counter.add"));
    } else {
        let cancelled = owner.response(running);
        assert!(
            cancelled.get("error").is_none() || cancelled["error"]["code"] == "cancelled",
            "{cancelled}"
        );
        let result = observer.response(action.unwrap());
        assert!(result.get("error").is_none(), "{result}");
        if kind == "steer" {
            assert_eq!(result["ok"]["status"], "settled", "{result}");
        }
    }
    let refreshed = observer.call(&observed["reference"], "snapshot", json!([]));
    assert!(refreshed.get("error").is_none(), "{refreshed}");
    let settled = observer.get("swarmie.conversation.session", kind);
    assert_eq!(
        settled["state"]["phase"],
        if kind == "steer" {
            "completed"
        } else {
            "stopped"
        },
        "{settled}"
    );
    assert_eq!(settled["state"]["canAcknowledge"], false, "{settled}");
    let results: Vec<_> = settled["state"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "tool")
        .collect();
    assert_eq!(
        results.len(),
        1,
        "the actual tool result must be committed exactly once"
    );
    let result: Value = serde_json::from_str(results[0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        result["kind"], "success",
        "reconciliation must use the real tool result"
    );
    assert_eq!(
        result["value"]["structuredContent"]["effect_count"], 1,
        "{result}"
    );
    if kind != "steer" {
        assert_eq!(
            provider.requests.lock().unwrap().len(),
            2,
            "reconciliation must not call the model again"
        );
        let fresh = observer.call(
            &observed["reference"],
            "run",
            json!([submission(&node, &account, kind, false, "queue")]),
        );
        assert_eq!(fresh["ok"]["status"], "settled", "{fresh}");
    }
    let complete = observer.get("swarmie.conversation.session", kind);
    assert_eq!(complete["state"]["phase"], "completed", "{complete}");
    let effect: Value = serde_json::from_slice(&std::fs::read(&gate).unwrap()).unwrap();
    assert_eq!(
        effect["effect_count"], 1,
        "reconciliation or the explicit new turn must never redispatch the old tool"
    );
    let requests = provider.requests.lock().unwrap();
    assert_eq!(
        requests.len(),
        3,
        "one catalog read, the original tool request, and the explicit new turn"
    );
    let messages = requests[2].1["messages"].as_array().unwrap();
    let tool = messages
        .iter()
        .find(|message| message["role"] == "tool")
        .unwrap();
    let content: Value = serde_json::from_str(tool["content"].as_str().unwrap()).unwrap();
    assert_eq!(content["structuredContent"]["effect_count"], 1);
    assert!(
        messages
            .iter()
            .any(|message| message["role"] == "user"
                && message["content"] == "Fresh explicit message")
    );
}

#[test]
fn native_disconnect_persists_and_recovers_the_real_tool_checkpoint_without_replay() {
    interrupted_native_tool("disconnect");
}

#[test]
fn native_stop_reconciles_a_transport_cancelled_tool_and_allows_a_new_explicit_turn() {
    interrupted_native_tool("stop");
}

#[test]
fn native_steer_reconciles_a_transport_cancelled_tool_before_admitting_the_new_turn() {
    interrupted_native_tool("steer");
}
