use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use swarmie_conversations::ConversationStorage;
use swarmie_desktop_runtime::Node;

const WAIT: Duration = Duration::from_secs(10);

fn send(node: &Node, channel: u64, id: u64, op: &str, payload: Value) {
    node.frame(
        channel,
        serde_json::to_vec(&json!({"id":id,"op":op,"payload":payload})).unwrap(),
    )
    .unwrap();
}

fn response(node: &Node, channel: u64, id: u64) -> Value {
    let deadline = Instant::now() + WAIT;
    loop {
        for frame in node.poll(channel).unwrap() {
            assert!(
                frame.len() <= 64 * 1024,
                "native output exceeded its frame budget"
            );
            let value: Value = serde_json::from_slice(&frame).unwrap();
            if value["id"] == id {
                return value;
            }
        }
        assert!(Instant::now() < deadline, "native RPC response timed out");
        thread::sleep(Duration::from_millis(1));
    }
}

fn get(node: &Node, channel: u64, service: &str, key: &str) -> Value {
    send(
        node,
        channel,
        1,
        "get",
        json!({"service":service,"key":[key]}),
    );
    let value = response(node, channel, 1);
    assert!(value.get("error").is_none(), "{value}");
    value["ok"].clone()
}

fn peek(node: &Node, channel: u64, id: &str) -> Value {
    let object = get(node, channel, "swarmie.conversation.session", id);
    send(
        node,
        channel,
        3,
        "release",
        json!({"reference":object["reference"]}),
    );
    let result = response(node, channel, 3);
    assert!(result.get("error").is_none(), "{result}");
    object
}

fn start(node: &Node, channel: u64, id: u64, reference: &Value, method: &str, arguments: Value) {
    send(
        node,
        channel,
        id,
        "call",
        json!({"reference":reference,"method":method,"arguments":arguments}),
    );
}

fn call(node: &Node, channel: u64, reference: &Value, method: &str, arguments: Value) -> Value {
    start(node, channel, 2, reference, method, arguments);
    response(node, channel, 2)
}

fn session(node: &Node, channel: u64, id: &str) -> Value {
    let object = get(node, channel, "swarmie.conversation.session", id);
    let refreshed = call(node, channel, &object["reference"], "snapshot", json!([]));
    assert!(refreshed.get("error").is_none(), "{refreshed}");
    get(node, channel, "swarmie.conversation.session", id)
}

fn submission(node: &Node, account: &Value, id: &str, receipt: &str) -> Value {
    json!({"conversationId":id,"submissionId":receipt,"createConversation":true,"draft":{"text":"Hello","attachments":[],"account":account["id"],"model":"fixture-model","effort":"","node":node.descriptor()["id"]},"mode":"queue"})
}

struct Reply {
    status: u16,
    first: String,
    tail: String,
    gate: Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>,
}

fn delta(text: &str, done: bool) -> String {
    format!(
        "{}\n",
        json!({"model":"fixture-model","created_at":"2026-01-01T00:00:00Z","message":{"role":"assistant","content":text},"done":done,"done_reason":"stop","prompt_eval_count":3,"eval_count":5})
    )
}

fn answer(text: &str) -> Reply {
    Reply {
        status: 200,
        first: delta(text, false),
        tail: delta("", true),
        gate: None,
    }
}

fn read_request(stream: &mut TcpStream) -> String {
    stream.set_nonblocking(false).unwrap();
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
            break (at + 4, length);
        }
    };
    while data.len() < end + length {
        let mut bytes = [0; 4096];
        let count = stream.read(&mut bytes).unwrap();
        assert!(count > 0);
        data.extend_from_slice(&bytes[..count]);
    }
    String::from_utf8(data).unwrap()
}

fn provider(replies: Vec<Reply>) -> (String, thread::JoinHandle<Vec<String>>) {
    let (endpoint, server, _) = observed_provider(replies);
    (endpoint, server)
}

fn observed_provider(
    replies: Vec<Reply>,
) -> (
    String,
    thread::JoinHandle<Vec<String>>,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
) {
    // Compile the bundled public rules before starting this fixture's network deadline.
    assert!(swarmie_providers::protection::Detector::bundled().is_some());
    let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = count.clone();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        let mut requests = Vec::new();
        let catalog = Reply {
            status: 200,
            first:
                r#"{"models":[{"name":"fixture-model","limit":{"context":128000,"output":8192},"swarmie_capabilities":{"profile":"swarmie.text-tools.v1","tools":true,"streaming":true}}]}"#
                    .into(),
            tail: String::new(),
            gate: None,
        };
        for (index, reply) in std::iter::once(catalog).chain(replies).enumerate() {
            let deadline = Instant::now() + WAIT;
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "provider request timed out");
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            requests.push(read_request(&mut stream));
            observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mime = if index == 0 || reply.status != 200 {
                "application/json"
            } else {
                "application/x-ndjson"
            };
            write!(stream, "HTTP/1.1 {} fixture\r\ncontent-type: {mime}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}", reply.status, reply.first.len() + reply.tail.len(), reply.first).unwrap();
            stream.flush().unwrap();
            if let Some((started, release)) = reply.gate {
                started.send(()).unwrap();
                release.recv_timeout(WAIT).unwrap();
            }
            // A cancelled consumer may already have closed the socket.
            let _ = stream.write_all(reply.tail.as_bytes());
        }
        requests
    });
    (endpoint, server, count)
}

fn configure(node: &Node, channel: u64, endpoint: &str) -> Value {
    let object = get(node, channel, "swarmie.providers.settings", "main");
    let result = call(
        node,
        channel,
        &object["reference"],
        "configure",
        json!([{"providerId":"ollama","method":"local","url":endpoint}]),
    );
    assert!(result.get("error").is_none(), "{result}");
    result["ok"].clone()
}

fn canonical_messages(path: &Path, node: &Value, id: &str) -> Vec<Value> {
    let history =
        swarmie_storage::sqlite::SqliteConversationStorage::open(path.join("conversations.sqlite"))
            .unwrap();
    let principal = node["principal"].as_str().unwrap();
    let identity = uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_URL,
        format!("{}/{principal}/{id}", node["id"].as_str().unwrap()).as_bytes(),
    )
    .to_string()
    .parse()
    .unwrap();
    futures::executor::block_on(async {
        let conversation = history.get(principal, identity).await.unwrap().unwrap();
        let mut read = swarmie_conversations::Read::new(conversation.head);
        let mut entries = Vec::new();
        loop {
            let page = history.read(principal, read.clone()).await.unwrap();
            entries.extend(page.entries.into_iter().map(|entry| entry.data));
            let Some(next) = page.next else {
                break;
            };
            read.after = Some(next);
        }
        entries
    })
}

#[test]
fn escaped_large_answers_have_bounded_readable_projections_and_complete_canonical_history() {
    let text = "\"\\\n".repeat(30 * 1024);
    let (endpoint, server) = provider(vec![answer(&text), answer("Next answer")]);
    let directory = tempfile::tempdir().unwrap();
    let node = Node::open(directory.path().into()).unwrap();
    let channel = node.open_shared_channel().unwrap();
    let account = configure(&node, channel, &endpoint);
    let object = session(&node, channel, "large");
    for receipt in ["large-first", "large-next"] {
        let result = call(
            &node,
            channel,
            &object["reference"],
            "run",
            json!([submission(&node, &account, "large", receipt)]),
        );
        assert_eq!(result["ok"]["status"], "settled", "{result}");
        let snapshot = session(&node, channel, "large");
        assert_eq!(snapshot["state"]["phase"], "completed", "{snapshot}");
        assert!(serde_json::to_vec(&snapshot["state"]).unwrap().len() <= 48 * 1024);
        assert!(
            snapshot["state"]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|message| message["truncated"] == true)
        );
    }
    let descriptor = node.descriptor();
    drop(node);
    let reopened = Node::open(directory.path().into()).unwrap();
    let reconnected = reopened.open_shared_channel().unwrap();
    let restored = session(&reopened, reconnected, "large");
    assert_eq!(restored["state"]["phase"], "completed");
    assert!(serde_json::to_vec(&restored["state"]).unwrap().len() <= 48 * 1024);
    assert!(
        restored["state"]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["truncated"] == true)
    );
    drop(reopened);
    let entries = canonical_messages(directory.path(), &descriptor, "large");
    let encoded = serde_json::to_string(&entries).unwrap();
    assert!(encoded.len() > 48 * 1024);
    assert!(entries.iter().any(|entry| {
        serde_json::to_string(entry)
            .unwrap()
            .contains(&serde_json::to_string(&text).unwrap())
    }));
    assert!(encoded.contains("Next answer"));
    assert_eq!(server.join().unwrap().len(), 3);
}

#[test]
fn explicit_pre_admission_refusals_do_not_admit_or_lock_the_next_submission() {
    let (endpoint, server) = provider(vec![answer("Accepted")]);
    let directory = tempfile::tempdir().unwrap();
    let node = Node::open(directory.path().into()).unwrap();
    let channel = node.open_shared_channel().unwrap();
    let account = configure(&node, channel, &endpoint);
    let object = session(&node, channel, "refused");
    let mut rejected = submission(&node, &account, "refused", "unsupported-file");
    rejected["draft"]["attachments"] = json!(["fixture.txt"]);
    let result = call(
        &node,
        channel,
        &object["reference"],
        "run",
        json!([rejected]),
    );
    assert_eq!(result["ok"]["status"], "refused", "{result}");
    assert!(
        result["ok"]["reason"]
            .as_str()
            .is_some_and(|reason| !reason.is_empty())
    );
    assert_eq!(
        session(&node, channel, "refused")["state"]["admitted"],
        json!([])
    );
    let result = call(
        &node,
        channel,
        &object["reference"],
        "run",
        json!([submission(&node, &account, "refused", "accepted")]),
    );
    assert_eq!(result["ok"]["status"], "settled", "{result}");
    let snapshot = session(&node, channel, "refused");
    assert_eq!(snapshot["state"]["phase"], "completed");
    assert_eq!(snapshot["state"]["admitted"], json!(["accepted"]));
    assert_eq!(server.join().unwrap().len(), 2);
}

#[test]
fn rejected_provider_response_settles_failed_and_allows_a_new_explicit_message() {
    let rejected = Reply {
        status: 400,
        first: json!({"error":"synthetic provider rejection"}).to_string(),
        tail: String::new(),
        gate: None,
    };
    let (endpoint, server) = provider(vec![rejected, answer("Recovered")]);
    let directory = tempfile::tempdir().unwrap();
    let node = Node::open(directory.path().into()).unwrap();
    let channel = node.open_shared_channel().unwrap();
    let account = configure(&node, channel, &endpoint);
    let object = session(&node, channel, "failure");
    for (receipt, phase) in [("failed", "failed"), ("new-input", "completed")] {
        let result = call(
            &node,
            channel,
            &object["reference"],
            "run",
            json!([submission(&node, &account, "failure", receipt)]),
        );
        assert_eq!(result["ok"]["status"], "settled", "{result}");
        assert_eq!(session(&node, channel, "failure")["state"]["phase"], phase);
    }
    let requests = server.join().unwrap();
    assert_eq!(
        requests.len(),
        3,
        "a failed request must not be replayed automatically"
    );
}

fn saved_turn(path: &Path, id: &str) -> swarmie_durable_tasks::TaskRecord {
    use swarmie_durable_tasks::TaskStorage;
    let metadata: Value =
        serde_json::from_slice(&std::fs::read(path.join("conversations.json")).unwrap()).unwrap();
    let task = metadata
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["id"] == id)
        .unwrap()["task"]
        .clone();
    let id = serde_json::from_value(task).unwrap();
    let storage = swarmie_storage::sqlite::SqliteOwner::new(path.join("conversations.sqlite"))
        .tasks()
        .unwrap();
    futures::executor::block_on(async {
        let record = storage.get(id).await.unwrap().unwrap();
        storage.close().await.unwrap();
        record
    })
}

fn interrupted_stream(kind: &str) {
    let (started, observed) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let mut held = answer("Uncommitted preview");
    held.gate = Some((started, wait));
    let (endpoint, server) = provider(vec![held, answer("New explicit message")]);
    let directory = tempfile::tempdir().unwrap();
    let node = Node::open(directory.path().into()).unwrap();
    let first = node.open_shared_channel().unwrap();
    let observer = node.open_shared_channel().unwrap();
    let account = configure(&node, first, &endpoint);
    let object = session(&node, first, kind);
    start(
        &node,
        first,
        90,
        &object["reference"],
        "run",
        json!([submission(&node, &account, kind, "interrupted")]),
    );
    observed.recv_timeout(WAIT).unwrap();
    let deadline = Instant::now() + WAIT;
    loop {
        let snapshot = peek(&node, observer, kind);
        if snapshot["state"]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["id"] == "preview")
        {
            break;
        }
        assert!(Instant::now() < deadline, "stream preview was not observed");
        thread::sleep(Duration::from_millis(1));
    }
    let record = saved_turn(directory.path(), kind);
    assert!(record.assignment.is_some());
    assert!(matches!(
        record.status,
        swarmie_durable_tasks::TaskStatus::Ready
    ));
    let checkpoint = record.checkpoint.unwrap();
    assert_eq!(checkpoint["phase"]["kind"], "model_pending");
    assert!(checkpoint["phase"]["request"].is_string());
    assert!(checkpoint["input"].get("context").is_none());
    let remote = session(&node, observer, kind);
    match kind {
        "disconnect" => node.close_channel(first),
        "stop" => start(&node, observer, 91, &remote["reference"], "stop", json!([])),
        "steer" => {
            let mut next = submission(&node, &account, kind, "new-explicit");
            next["mode"] = json!("steer");
            start(
                &node,
                observer,
                91,
                &remote["reference"],
                "run",
                json!([next]),
            );
        }
        _ => unreachable!(),
    }
    // The command is queued before the provider finishes. No cancellation assumes rollback.
    thread::sleep(Duration::from_millis(20));
    release.send(()).unwrap();
    if kind != "disconnect" {
        let result = response(&node, first, 90);
        assert!(
            result.get("error").is_none() || result["error"]["code"] == "cancelled",
            "{result}"
        );
        let result = response(&node, observer, 91);
        assert!(result.get("error").is_none(), "{result}");
    }
    let deadline = Instant::now() + WAIT;
    loop {
        // Observe emitted State only. A snapshot call could repair a broken tail projection.
        let snapshot = peek(&node, observer, kind);
        if snapshot["state"]["phase"] != "working" {
            assert_eq!(
                snapshot["state"]["phase"],
                if kind == "steer" || kind == "disconnect" {
                    "completed"
                } else {
                    "stopped"
                },
                "{snapshot}"
            );
            assert!(
                !snapshot["state"]["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|message| message["id"] == "preview")
            );
            assert_eq!(
                snapshot["state"]["messages"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|message| message["text"] == "Uncommitted preview"),
                kind == "disconnect",
                "only observation was released; explicit stop still discards the unsettled preview"
            );
            break;
        }
        assert!(Instant::now() < deadline, "cancelled stream did not settle");
        thread::sleep(Duration::from_millis(1));
    }
    if kind != "steer" {
        let result = call(
            &node,
            observer,
            &remote["reference"],
            "run",
            json!([submission(&node, &account, kind, "new-explicit")]),
        );
        assert_eq!(result["ok"]["status"], "settled", "{result}");
    }
    assert_eq!(
        session(&node, observer, kind)["state"]["phase"],
        "completed"
    );
    assert_eq!(
        server.join().unwrap().len(),
        3,
        "only explicit user submissions may run"
    );
}

#[test]
fn closing_the_observer_preserves_admitted_durable_work_and_allows_a_new_explicit_message() {
    interrupted_stream("disconnect");
}

#[test]
fn stopping_clears_uncommitted_stream_preview_after_physical_settlement() {
    interrupted_stream("stop");
}

#[test]
fn steering_waits_for_cancelled_execution_and_admits_only_the_new_explicit_message() {
    interrupted_stream("steer");
}

#[test]
fn conversation_catalog_pages_stay_bounded_and_refuse_a_stale_revision_cursor() {
    let directory = tempfile::tempdir().unwrap();
    let records: Vec<Value> = (0..350).map(|index| json!({"id":format!("conversation-{index}"),"title":"\\\"".repeat(200),"principal":"desktop-user","node":"synthetic-node","account":"synthetic-account","model":"fixture-model","effort":"","archived":false,"pinned":false,"receipts":[],"phase":"completed"})).collect();
    std::fs::write(
        directory.path().join("conversations.json"),
        serde_json::to_vec(&records).unwrap(),
    )
    .unwrap();
    let node = Node::open(directory.path().into()).unwrap();
    let channel = node.open_shared_channel().unwrap();
    let catalog = get(&node, channel, "swarmie.conversation.catalog", "main");
    let mut cursor = Value::Null;
    let mut ids = std::collections::BTreeSet::new();
    let mut first_cursor = Value::Null;
    loop {
        let page = call(
            &node,
            channel,
            &catalog["reference"],
            "list",
            json!([cursor]),
        );
        assert!(page.get("error").is_none(), "{page}");
        assert!(serde_json::to_vec(&page["ok"]).unwrap().len() < 48 * 1024);
        for entry in page["ok"]["entries"].as_array().unwrap() {
            assert!(ids.insert(entry["id"].as_str().unwrap().to_owned()));
            assert!(entry.get("principal").is_none());
            assert!(entry.get("receipts").is_none());
        }
        cursor = page["ok"]["nextCursor"].clone();
        if first_cursor.is_null() {
            first_cursor = cursor.clone();
            let index = directory.path().join("conversations.json");
            let mut metadata: Vec<Value> =
                serde_json::from_slice(&std::fs::read(&index).unwrap()).unwrap();
            metadata[0]["phase"] = json!("working");
            metadata[0]["receipts"] = json!([{"id":"progress-only","digest":"synthetic-digest"}]);
            let head = json!({"conversation":uuid::Uuid::new_v4().to_string(),"entry":null});
            let checkpoint = json!({"input":{"at":head,"instructions":"","tools":[],"max_turns":1},"head":head,"turn":0,"usage":{"input_tokens":0,"output_tokens":0},"available_tools":[],"phase":{"kind":"prepare_turn"}});
            let _: swarmie_agent_loop::conversation::Checkpoint =
                serde_json::from_value(checkpoint.clone()).unwrap();
            metadata[0]["checkpoint"] = checkpoint;
            std::fs::write(index, serde_json::to_vec(&metadata).unwrap()).unwrap();
        }
        if cursor.is_null() {
            break;
        }
    }
    assert_eq!(ids.len(), 350);
    let update = call(
        &node,
        channel,
        &catalog["reference"],
        "update",
        json!(["conversation-0", "Updated", false, false]),
    );
    assert!(update.get("error").is_none(), "{update}");
    let stale = call(
        &node,
        channel,
        &catalog["reference"],
        "list",
        json!([first_cursor]),
    );
    assert!(
        stale.get("error").is_some(),
        "a revision change must refuse mixed-page results: {stale}"
    );
    let fresh = call(&node, channel, &catalog["reference"], "list", json!([null]));
    assert!(fresh.get("error").is_none(), "{fresh}");
    let index = directory.path().join("conversations.json");
    let mut inventory: Vec<Value> =
        serde_json::from_slice(&std::fs::read(&index).unwrap()).unwrap();
    let mut additional = inventory[1].clone();
    additional["id"] = json!("new-inventory-entry");
    inventory.push(additional);
    std::fs::write(index, serde_json::to_vec(&inventory).unwrap()).unwrap();
    let stale = call(
        &node,
        channel,
        &catalog["reference"],
        "list",
        json!([fresh["ok"]["nextCursor"]]),
    );
    assert!(
        stale.get("error").is_some(),
        "inventory changes must refuse mixed-page results: {stale}"
    );
}

fn seed_history(
    path: &Path,
    node: &Value,
    account: &Value,
    id: &str,
    phase: &str,
    messages: Vec<swarmie_agent_loop::Message>,
) {
    use futures::{FutureExt, executor::LocalPool};
    use std::rc::Rc;
    use swarmie_agent_loop::conversation::Record;
    use swarmie_conversations::{Conversations, EntryDraft, WriteConfirmation};

    let principal = node["principal"].as_str().unwrap();
    let identity = uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_URL,
        format!("{}/{principal}/{id}", node["id"].as_str().unwrap()).as_bytes(),
    )
    .to_string()
    .parse()
    .unwrap();
    let storage = Rc::new(
        swarmie_storage::sqlite::SqliteConversationStorage::open(path.join("conversations.sqlite"))
            .unwrap(),
    );
    let history = Conversations::new(storage);
    let mut pool = LocalPool::new();
    let host = tembra::Host::new(pool.spawner());
    pool.run_until(host.root(principal, move |context| {
        async move {
            let created = history.create(&context, identity).await?;
            assert_eq!(created.confirmation, WriteConfirmation::Committed);
            let entries = messages
                .into_iter()
                .map(|message| EntryDraft::encode(Record::Message(message)))
                .collect::<Result<Vec<_>, _>>()?;
            let appended = history.append(&context, created.head, entries).await?;
            assert_eq!(appended.confirmation, WriteConfirmation::Committed);
            Ok(())
        }
        .boxed_local()
    }))
    .unwrap();
    let index = path.join("conversations.json");
    let mut records: Vec<Value> = match std::fs::read(&index) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => vec![],
        Err(error) => panic!("{error}"),
    };
    records.push(json!({"id":id,"title":"Persisted tool fixture","principal":principal,"node":node["id"],"account":account["id"],"model":"fixture-model","effort":"","archived":false,"pinned":false,"receipts":[],"phase":phase,"checkpoint":null}));
    std::fs::write(index, serde_json::to_vec(&records).unwrap()).unwrap();
}

fn pending_tool_messages() -> Vec<swarmie_agent_loop::Message> {
    use swarmie_agent_loop::{AssistantMessage, Message, ToolCall, ToolCallId, ToolName};

    vec![
        Message::User {
            content: "Run the synthetic tool".into(),
        },
        Message::Assistant(AssistantMessage {
            provider: None,
            content: String::new(),
            tool_calls: vec![ToolCall {
                id: ToolCallId::new("pending-call").unwrap(),
                name: ToolName::new("mcp.synthetic.effect").unwrap(),
                arguments: json!({"synthetic":true}),
            }],
        }),
    ]
}

#[test]
fn persisted_unknown_tool_outcome_requires_explicit_acknowledgement_without_dispatch() {
    use swarmie_agent_loop::{Message, ToolResult, conversation::Record};

    let (endpoint, server) = provider(vec![answer("Fresh response after acknowledgement")]);
    let directory = tempfile::tempdir().unwrap();
    let initial = Node::open(directory.path().into()).unwrap();
    let channel = initial.open_shared_channel().unwrap();
    let account = configure(&initial, channel, &endpoint);
    let descriptor = initial.descriptor();
    drop(initial);
    seed_history(
        directory.path(),
        &descriptor,
        &account,
        "interrupted-tool",
        "unknown",
        pending_tool_messages(),
    );
    let before = canonical_messages(directory.path(), &descriptor, "interrupted-tool");
    assert_eq!(before.len(), 2);
    let node = Node::open(directory.path().into()).unwrap();
    let channel = node.open_shared_channel().unwrap();
    let object = session(&node, channel, "interrupted-tool");
    assert_eq!(object["state"]["phase"], "unknown");
    assert_eq!(object["state"]["canAcknowledge"], true);
    let stopped = call(&node, channel, &object["reference"], "stop", json!([]));
    assert!(stopped.get("error").is_none(), "{stopped}");
    assert_eq!(
        peek(&node, channel, "interrupted-tool")["state"]["phase"],
        "unknown"
    );
    let refused = call(
        &node,
        channel,
        &object["reference"],
        "run",
        json!([submission(
            &node,
            &account,
            "interrupted-tool",
            "before-acknowledgement"
        )]),
    );
    assert_eq!(refused["ok"]["status"], "refused", "{refused}");
    assert_eq!(
        canonical_messages(directory.path(), &descriptor, "interrupted-tool"),
        before
    );
    let acknowledged = call(
        &node,
        channel,
        &object["reference"],
        "acknowledge",
        json!([]),
    );
    assert!(acknowledged.get("error").is_none(), "{acknowledged}");
    let acknowledged = peek(&node, channel, "interrupted-tool");
    assert_eq!(acknowledged["state"]["phase"], "stopped");
    assert_eq!(acknowledged["state"]["canAcknowledge"], false);
    let annotated = canonical_messages(directory.path(), &descriptor, "interrupted-tool");
    assert_eq!(&annotated[..2], before);
    assert_eq!(
        annotated.len(),
        3,
        "acknowledgement must only append a terminal annotation"
    );
    let annotation: Record = serde_json::from_value(annotated[2].clone()).unwrap();
    assert!(
        matches!(annotation, Record::Message(Message::Tool { call, result: ToolResult::Failed(reason) }) if call.as_str() == "pending-call" && reason == "outcome unknown; not replayed")
    );
    let repeated = call(
        &node,
        channel,
        &object["reference"],
        "acknowledge",
        json!([]),
    );
    assert!(repeated.get("error").is_none(), "{repeated}");
    assert_eq!(
        canonical_messages(directory.path(), &descriptor, "interrupted-tool"),
        annotated
    );
    let result = call(
        &node,
        channel,
        &object["reference"],
        "run",
        json!([submission(
            &node,
            &account,
            "interrupted-tool",
            "fresh-explicit-message"
        )]),
    );
    assert_eq!(result["ok"]["status"], "settled", "{result}");
    assert_eq!(
        peek(&node, channel, "interrupted-tool")["state"]["phase"],
        "completed"
    );
    let requests = server.join().unwrap();
    assert_eq!(
        requests.len(),
        2,
        "acknowledgement cannot invoke a model or a tool"
    );
    assert!(requests[1].contains("outcome unknown; not replayed"));
}

#[test]
fn large_canonical_tool_result_has_a_bounded_readable_projection_without_rewriting_history() {
    use swarmie_agent_loop::{Message, ToolCallId, ToolResult};

    let directory = tempfile::tempdir().unwrap();
    let initial = Node::open(directory.path().into()).unwrap();
    let descriptor = initial.descriptor();
    drop(initial);
    let text = "\"\\\n暖".repeat(30 * 1024);
    let mut messages = pending_tool_messages();
    messages.push(Message::Tool {
        call: ToolCallId::new("pending-call").unwrap(),
        result: ToolResult::Success(json!({"payload":text})),
    });
    seed_history(
        directory.path(),
        &descriptor,
        &json!({"id":"synthetic-account"}),
        "large-tool",
        "completed",
        messages,
    );
    let before = canonical_messages(directory.path(), &descriptor, "large-tool");
    assert!(serde_json::to_vec(&before).unwrap().len() > 48 * 1024);
    let node = Node::open(directory.path().into()).unwrap();
    let channel = node.open_shared_channel().unwrap();
    let object = session(&node, channel, "large-tool");
    assert_eq!(object["state"]["phase"], "completed");
    assert!(serde_json::to_vec(&object["state"]).unwrap().len() <= 48 * 1024);
    let tool = object["state"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|message| message["role"] == "tool")
        .unwrap();
    assert_eq!(tool["truncated"], true);
    assert!(!tool["text"].as_str().unwrap().is_empty());
    assert!(tool["text"].as_str().unwrap().len() < text.len());
    drop(node);
    assert_eq!(
        canonical_messages(directory.path(), &descriptor, "large-tool"),
        before
    );
}

#[test]
fn confirmed_duplicate_stays_admitted_when_its_provider_account_becomes_unavailable() {
    let (endpoint, server) = provider(vec![answer("Committed answer")]);
    let directory = tempfile::tempdir().unwrap();
    let node = Node::open(directory.path().into()).unwrap();
    let channel = node.open_shared_channel().unwrap();
    let account = configure(&node, channel, &endpoint);
    let object = session(&node, channel, "duplicate-without-provider");
    let input = submission(
        &node,
        &account,
        "duplicate-without-provider",
        "confirmed-receipt",
    );
    let first = call(
        &node,
        channel,
        &object["reference"],
        "run",
        json!([input.clone()]),
    );
    assert_eq!(first["ok"]["status"], "settled", "{first}");
    let confirmed = peek(&node, channel, "duplicate-without-provider");
    assert_eq!(confirmed["state"]["phase"], "completed");
    assert_eq!(confirmed["state"]["admitted"], json!(["confirmed-receipt"]));
    let before = canonical_messages(
        directory.path(),
        &node.descriptor(),
        "duplicate-without-provider",
    );
    assert_eq!(
        server.join().unwrap().len(),
        2,
        "the initial submission must invoke exactly one model request"
    );
    // Remove only metadata created by this synthetic fixture. Confirmed admission
    // remains authoritative when its former provider can no longer be resolved.
    std::fs::write(directory.path().join("providers.json"), b"[]").unwrap();
    let duplicate = call(&node, channel, &object["reference"], "run", json!([input]));
    assert_eq!(
        canonical_messages(
            directory.path(),
            &node.descriptor(),
            "duplicate-without-provider"
        ),
        before,
        "observing a confirmed duplicate cannot append or replay history"
    );
    assert_eq!(
        peek(&node, channel, "duplicate-without-provider")["state"]["admitted"],
        json!(["confirmed-receipt"])
    );
    assert_eq!(
        duplicate["ok"]["status"], "settled",
        "confirmed admission cannot become a pre-admission refusal when provider resolution fails: {duplicate}"
    );
}

#[test]
fn repeating_a_live_steer_submission_observes_admission_without_cancelling_or_replaying() {
    use swarmie_agent_loop::{Message, conversation::Record};

    let (started, observed) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let mut held = answer("Original live response");
    held.gate = Some((started, wait));
    let (endpoint, server) = provider(vec![held]);
    let directory = tempfile::tempdir().unwrap();
    let node = Node::open(directory.path().into()).unwrap();
    let first = node.open_shared_channel().unwrap();
    let observer = node.open_shared_channel().unwrap();
    let account = configure(&node, first, &endpoint);
    let object = session(&node, first, "duplicate-live-steer");
    let remote = session(&node, observer, "duplicate-live-steer");
    let mut input = submission(
        &node,
        &account,
        "duplicate-live-steer",
        "live-steer-receipt",
    );
    input["mode"] = json!("steer");
    start(
        &node,
        first,
        90,
        &object["reference"],
        "run",
        json!([input.clone()]),
    );
    observed.recv_timeout(WAIT).unwrap();
    let deadline = Instant::now() + WAIT;
    loop {
        let snapshot = peek(&node, observer, "duplicate-live-steer");
        if snapshot["state"]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["id"] == "preview")
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "live stream preview did not arrive"
        );
        thread::sleep(Duration::from_millis(1));
    }
    let duplicate = call(&node, observer, &remote["reference"], "run", json!([input]));
    assert_eq!(duplicate["ok"]["status"], "settled", "{duplicate}");
    let still_running = peek(&node, observer, "duplicate-live-steer");
    assert_eq!(
        still_running["state"]["phase"], "working",
        "observing a duplicate cannot cancel its original invocation"
    );
    assert_eq!(
        still_running["state"]["admitted"],
        json!(["live-steer-receipt"])
    );
    release.send(()).unwrap();
    let original = response(&node, first, 90);
    assert_eq!(original["ok"]["status"], "settled", "{original}");
    let completed = session(&node, observer, "duplicate-live-steer");
    assert_eq!(completed["state"]["phase"], "completed");
    let entries = canonical_messages(directory.path(), &node.descriptor(), "duplicate-live-steer");
    let messages: Vec<Message> = entries
        .into_iter()
        .filter_map(
            |entry| match serde_json::from_value::<Record>(entry).unwrap() {
                Record::Message(message) => Some(message),
                _ => None,
            },
        )
        .collect();
    assert_eq!(
        messages.len(),
        2,
        "a duplicate cannot append another canonical user or assistant message"
    );
    assert!(matches!(&messages[0], Message::User { content } if content == "Hello"));
    assert!(
        matches!(&messages[1], Message::Assistant(assistant) if assistant.content == "Original live response")
    );
    assert_eq!(
        server.join().unwrap().len(),
        2,
        "only the original submission may invoke inference"
    );
}

#[cfg(unix)]
fn captured_checkout(node: &Node, channel: u64, root: &Path) -> (Value, Value) {
    assert!(
        std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(root)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        std::process::Command::new("git")
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "--quiet",
                "--allow-empty",
                "-m",
                "test: initialize fixture"
            ])
            .current_dir(root)
            .status()
            .unwrap()
            .success()
    );
    node.grant_directory(root).unwrap();
    let git = get(node, channel, "swarmie.git", "main");
    let discovered = call(
        node,
        channel,
        &git["reference"],
        "request",
        json!([{ "kind":"discover", "path":root }]),
    );
    let target = discovered["ok"]["repository"].clone();
    assert!(target["git_dir"].is_string(), "{discovered}");
    let descriptor = node.descriptor();
    let binding = json!({"version":1,"nodeId":descriptor["id"],"principal":descriptor["principal"],"path":target["root"],"owner":"git","identity":serde_json::to_string(&json!([target["root"], target["git_dir"], target["common_dir"]])).unwrap()});
    (binding, target)
}

#[test]
#[cfg(unix)]
fn checkout_admission_is_bound_persisted_and_restored_with_actual_git_tool_routing() {
    let directory = tempfile::tempdir().unwrap();
    let repository = tempfile::tempdir().unwrap();
    let initial = Node::open(directory.path().into()).unwrap();
    let channel = initial.open_shared_channel().unwrap();
    let (binding, target) = captured_checkout(&initial, channel, repository.path());
    let tool_reply = Reply {
        status: 200,
        first: format!(
            "{}\n",
            json!({"model":"fixture-model","message":{"role":"assistant","content":"","tool_calls":[{"id":"git-inspect-call","function":{"name":"git_inspect","arguments":{"kind":"status","target":target}}}]},"done":false})
        ),
        tail: format!(
            "{}\n",
            json!({"model":"fixture-model","message":{"role":"assistant","content":""},"done":true,"done_reason":"stop","prompt_eval_count":3,"eval_count":5})
        ),
        gate: None,
    };
    let (endpoint, server) = provider(vec![tool_reply, answer("Repository inspected")]);
    let account = configure(&initial, channel, &endpoint);
    let object = session(&initial, channel, "checkout-bound");
    let mut input = submission(&initial, &account, "checkout-bound", "first");
    input["executionContext"] = binding.clone();
    input["draft"]["executionContext"] = binding.clone();
    let result = call(
        &initial,
        channel,
        &object["reference"],
        "run",
        json!([input]),
    );
    assert_eq!(result["ok"]["status"], "settled", "{result}");
    let requests = server.join().unwrap();
    assert_eq!(
        requests.len(),
        3,
        "one catalog request and two inference turns"
    );
    assert!(requests[1].contains("git_inspect"));
    assert!(
        requests[1].contains("vault_request_input"),
        "Git and vault providers must coexist in the actual client's tool catalog"
    );
    let request: Value =
        serde_json::from_str(requests[2].split_once("\r\n\r\n").unwrap().1).unwrap();
    assert!(
        request["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| {
                message["role"] == "tool"
                    && message["content"].as_str().is_some_and(|content| {
                        serde_json::from_str::<Value>(content)
                            .is_ok_and(|result| result["kind"] == "status")
                    })
            }),
        "the second inference turn must contain the Git owner result: {request}"
    );
    let snapshot = session(&initial, channel, "checkout-bound");
    assert_eq!(snapshot["state"]["executionContext"], binding);
    let stored: Value = serde_json::from_slice(
        &std::fs::read(directory.path().join("conversations.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(stored[0]["executionContext"], binding);
    let catalog = get(&initial, channel, "swarmie.conversation.catalog", "main");
    let listed = call(
        &initial,
        channel,
        &catalog["reference"],
        "list",
        json!([null]),
    );
    assert_eq!(listed["ok"]["entries"][0]["executionContext"], binding);
    drop(initial);
    let restored = Node::open(directory.path().into()).unwrap();
    let channel = restored.open_shared_channel().unwrap();
    let object = session(&restored, channel, "checkout-bound");
    assert_eq!(object["state"]["executionContext"], binding);
    let mut changed = submission(&restored, &account, "checkout-bound", "changed-checkout");
    changed["executionContext"] = binding.clone();
    changed["executionContext"]["identity"] = json!("another-checkout");
    let refused = call(
        &restored,
        channel,
        &object["reference"],
        "run",
        json!([changed]),
    );
    assert_eq!(refused["ok"]["status"], "refused", "{refused}");
    let missing = call(
        &restored,
        channel,
        &object["reference"],
        "run",
        json!([submission(
            &restored,
            &account,
            "checkout-bound",
            "missing-checkout"
        )]),
    );
    assert_eq!(missing["ok"]["status"], "refused", "{missing}");
    let mut unavailable = submission(
        &restored,
        &account,
        "checkout-bound",
        "without-native-grant",
    );
    unavailable["executionContext"] = binding;
    let refused = call(
        &restored,
        channel,
        &object["reference"],
        "run",
        json!([unavailable]),
    );
    assert_eq!(
        refused["ok"]["status"], "refused",
        "a restored context cannot restore filesystem grants: {refused}"
    );
    let catalog = get(&restored, channel, "swarmie.conversation.catalog", "main");
    let archived = call(
        &restored,
        channel,
        &catalog["reference"],
        "update",
        json!([
            "checkout-bound",
            "Archived without restored filesystem authority",
            true,
            true
        ]),
    );
    assert!(
        archived.get("error").is_none(),
        "confirmed organizational archival must survive a refused Git snapshot: {archived}"
    );
    let snapshot = session(&restored, channel, "checkout-bound");
    assert!(snapshot["state"]["lifecycleWarning"].is_string());
    let metadata: Value = serde_json::from_slice(
        &std::fs::read(directory.path().join("conversations.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(metadata[0]["archived"], true);
    assert_eq!(metadata[0]["pinned"], true);
    assert!(metadata[0]["lifecycleWarning"].is_string());
}

#[test]
#[cfg(unix)]
fn checkout_admission_rejects_wrong_node_principal_identity_and_disagreement_before_history() {
    let directory = tempfile::tempdir().unwrap();
    let repository = tempfile::tempdir().unwrap();
    let node = Node::open(directory.path().into()).unwrap();
    let channel = node.open_shared_channel().unwrap();
    let (binding, _) = captured_checkout(&node, channel, repository.path());
    let account = json!({"id":"not-resolved-before-checkout-validation"});
    let object = session(&node, channel, "invalid-checkout");
    for (field, replacement) in [
        ("nodeId", "another-node"),
        ("principal", "another-principal"),
        ("identity", "another-checkout"),
    ] {
        let mut input = submission(&node, &account, "invalid-checkout", field);
        input["executionContext"] = binding.clone();
        input["executionContext"][field] = json!(replacement);
        let result = call(&node, channel, &object["reference"], "run", json!([input]));
        assert_eq!(result["ok"]["status"], "refused", "{result}");
        assert!(
            !directory.path().join("conversations.json").exists(),
            "checkout rejection must happen before admission"
        );
    }
    let mut input = submission(&node, &account, "invalid-checkout", "conflicting-contexts");
    input["executionContext"] = binding.clone();
    input["draft"]["executionContext"] = binding;
    input["draft"]["executionContext"]["path"] = json!("/different-resource");
    let result = call(&node, channel, &object["reference"], "run", json!([input]));
    assert_eq!(result["ok"]["status"], "refused", "{result}");
    assert!(!directory.path().join("conversations.json").exists());
}

#[test]
#[cfg(unix)]
fn agent_git_mutation_uses_explicit_native_authority_and_cannot_cross_captured_checkouts() {
    use swarmie_agent_loop::{Message, ToolResult, conversation::Record};
    let directory = tempfile::tempdir().unwrap();
    let repository = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    std::fs::write(repository.path().join("owned.txt"), "captured checkout\n").unwrap();
    std::fs::write(other.path().join("owned.txt"), "another checkout\n").unwrap();
    let node = Node::open(directory.path().into()).unwrap();
    let channel = node.open_shared_channel().unwrap();
    let (binding, target) = captured_checkout(&node, channel, repository.path());
    let (_, other_target) = captured_checkout(&node, channel, other.path());
    let tool = |target: Value, id: &str| Reply {
        status: 200,
        first: format!(
            "{}\n",
            json!({"model":"fixture-model","message":{"role":"assistant","content":"","tool_calls":[{"id":id,"function":{"name":"git_mutate","arguments":{"target":target,"operation":{"kind":"stage","paths":["owned.txt"]}}}}]},"done":false})
        ),
        tail: format!(
            "{}\n",
            json!({"model":"fixture-model","message":{"role":"assistant","content":""},"done":true,"done_reason":"stop","prompt_eval_count":3,"eval_count":5})
        ),
        gate: None,
    };
    let (endpoint, server) = provider(vec![
        tool(target, "owned-stage-call"),
        answer("Staged captured file"),
        tool(other_target, "cross-stage-call"),
        answer("Cross-checkout write blocked"),
    ]);
    let account = configure(&node, channel, &endpoint);
    let object = session(&node, channel, "scoped-agent-write");
    for receipt in ["native-authorized-stage", "cross-checkout-attempt"] {
        let mut input = submission(&node, &account, "scoped-agent-write", receipt);
        input["executionContext"] = binding.clone();
        let result = call(&node, channel, &object["reference"], "run", json!([input]));
        assert_eq!(result["ok"]["status"], "settled", "{result}");
    }
    let staged = |path: &Path| {
        std::process::Command::new("git")
            .args(["diff", "--cached", "--name-only"])
            .current_dir(path)
            .output()
            .unwrap()
            .stdout
    };
    assert_eq!(
        String::from_utf8(staged(repository.path())).unwrap().trim(),
        "owned.txt"
    );
    assert!(
        staged(other.path()).is_empty(),
        "even a separately granted repository must not receive the conversation's writes"
    );
    let messages = canonical_messages(directory.path(), &node.descriptor(), "scoped-agent-write");
    assert!(messages.into_iter().any(|record| matches!(
        serde_json::from_value::<Record>(record).unwrap(),
        Record::Message(Message::Tool {
            result: ToolResult::Blocked(_),
            ..
        })
    )));
    assert_eq!(server.join().unwrap().len(), 5);
}

#[test]
#[cfg(unix)]
fn conversation_archive_and_pin_defer_capture_until_physical_agent_settlement() {
    let (started, observed) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let mut held = answer("Physical execution finished");
    held.gate = Some((started, wait));
    let (endpoint, server) = provider(vec![held]);
    let directory = tempfile::tempdir().unwrap();
    let repository = tempfile::tempdir().unwrap();
    std::fs::write(
        repository.path().join("user-owned.txt"),
        "retained local content\n",
    )
    .unwrap();
    let node = Node::open(directory.path().into()).unwrap();
    let first = node.open_shared_channel().unwrap();
    let observer = node.open_shared_channel().unwrap();
    let (binding, _) = captured_checkout(&node, first, repository.path());
    let account = configure(&node, first, &endpoint);
    let object = session(&node, first, "archive-with-live-writer");
    let mut input = submission(&node, &account, "archive-with-live-writer", "first");
    input["executionContext"] = binding;
    start(
        &node,
        first,
        90,
        &object["reference"],
        "run",
        json!([input]),
    );
    observed.recv_timeout(WAIT).unwrap();
    let catalog = get(&node, observer, "swarmie.conversation.catalog", "main");
    let archived = call(
        &node,
        observer,
        &catalog["reference"],
        "update",
        json!([
            "archive-with-live-writer",
            "Archived while running",
            true,
            true
        ]),
    );
    assert!(archived.get("error").is_none(), "{archived}");
    let read_hold = || {
        let files: Vec<_> = std::fs::read_dir(directory.path().join("git/conversation-holds"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .collect();
        assert_eq!(files.len(), 1);
        serde_json::from_slice::<Value>(&std::fs::read(&files[0]).unwrap()).unwrap()
    };
    let hold = read_hold();
    assert_eq!(hold["active"], true);
    assert_eq!(hold["pinned"], true);
    assert_eq!(hold["archived"], true);
    assert_eq!(hold["archive_state"], "deferred");
    assert!(
        hold["archive_id"].is_null(),
        "organizational archive cannot capture while this writer is physically active"
    );
    let snapshot = session(&node, observer, "archive-with-live-writer");
    assert!(
        snapshot["state"]["lifecycleWarning"]
            .as_str()
            .unwrap()
            .contains("physical execution")
    );
    release.send(()).unwrap();
    let result = response(&node, first, 90);
    assert_eq!(result["ok"]["status"], "settled", "{result}");
    let hold = read_hold();
    assert_eq!(hold["active"], false);
    assert_eq!(hold["pinned"], true);
    assert_eq!(hold["archived"], true);
    assert_eq!(hold["archive_state"], "completed", "{hold}");
    assert!(hold["archive_id"].is_string());
    assert!(hold["warning"].is_null());
    let snapshot = session(&node, observer, "archive-with-live-writer");
    assert!(snapshot["state"]["lifecycleWarning"].is_null());
    assert_eq!(
        std::fs::read_to_string(repository.path().join("user-owned.txt")).unwrap(),
        "retained local content\n"
    );
    let index: Value = serde_json::from_slice(
        &std::fs::read(directory.path().join("conversations.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(index[0]["archived"], true);
    assert_eq!(index[0]["pinned"], true);
    assert_eq!(server.join().unwrap().len(), 2);
}

#[test]
#[cfg(unix)]
fn archiving_an_idle_chat_waits_for_a_different_chat_using_the_same_checkout() {
    let (started, observed) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let mut held = answer("Other checkout consumer settled");
    held.gate = Some((started, wait));
    let (endpoint, server) = provider(vec![answer("Idle chat finished"), held]);
    let directory = tempfile::tempdir().unwrap();
    let repository = tempfile::tempdir().unwrap();
    let node = Node::open(directory.path().into()).unwrap();
    let first = node.open_shared_channel().unwrap();
    let observer = node.open_shared_channel().unwrap();
    let (binding, _) = captured_checkout(&node, first, repository.path());
    let account = configure(&node, first, &endpoint);
    let idle = session(&node, first, "idle-shared-checkout");
    let mut input = submission(&node, &account, "idle-shared-checkout", "idle-first");
    input["executionContext"] = binding.clone();
    let result = call(&node, first, &idle["reference"], "run", json!([input]));
    assert_eq!(result["ok"]["status"], "settled", "{result}");
    let writer = session(&node, first, "writer-shared-checkout");
    let mut input = submission(&node, &account, "writer-shared-checkout", "writer-first");
    input["executionContext"] = binding;
    start(
        &node,
        first,
        90,
        &writer["reference"],
        "run",
        json!([input]),
    );
    observed.recv_timeout(WAIT).unwrap();
    let catalog = get(&node, observer, "swarmie.conversation.catalog", "main");
    let archived = call(
        &node,
        observer,
        &catalog["reference"],
        "update",
        json!(["idle-shared-checkout", "Idle chat archived", false, true]),
    );
    assert!(archived.get("error").is_none(), "{archived}");
    assert_eq!(archived["ok"]["archived"], true);
    assert!(
        archived["ok"]["lifecycleWarning"]
            .as_str()
            .unwrap()
            .contains("physical execution"),
        "the confirmed update must expose deferred capture immediately: {archived}"
    );
    let read_holds = || {
        std::fs::read_dir(directory.path().join("git/conversation-holds"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .map(|path| serde_json::from_slice::<Value>(&std::fs::read(path).unwrap()).unwrap())
            .collect::<Vec<_>>()
    };
    let holds = read_holds();
    let idle_hold = holds
        .iter()
        .find(|hold| hold["conversation_id"] == "idle-shared-checkout")
        .unwrap();
    assert_eq!(idle_hold["active"], true);
    assert_eq!(idle_hold["archive_state"], "deferred");
    assert!(idle_hold["archive_id"].is_null());
    release.send(()).unwrap();
    let result = response(&node, first, 90);
    assert_eq!(result["ok"]["status"], "settled", "{result}");
    let holds = read_holds();
    let idle_hold = holds
        .iter()
        .find(|hold| hold["conversation_id"] == "idle-shared-checkout")
        .unwrap();
    assert_eq!(idle_hold["active"], false);
    assert_eq!(
        idle_hold["archive_state"], "completed",
        "the other chat's settlement must resume the deferred archive: {idle_hold}"
    );
    assert!(idle_hold["archive_id"].is_string());
    assert!(holds.iter().all(|hold| hold["active"] == false));
    assert!(repository.path().exists());
    assert_eq!(server.join().unwrap().len(), 3);
}

#[test]
#[ignore = "subprocess fixture for durable admission recovery"]
fn admitted_turn_bootstrap_child() {
    use futures::task::{LocalFutureObj, LocalSpawn, SpawnError};
    struct Executor;
    impl LocalSpawn for Executor {
        fn spawn_local_obj(&self, future: LocalFutureObj<'static, ()>) -> Result<(), SpawnError> {
            tokio::task::spawn_local(future);
            Ok(())
        }
    }
    let profile =
        std::path::PathBuf::from(std::env::var_os("SWARMIE_ADMISSION_CHILD_PROFILE").unwrap());
    let profile_lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(profile.join("owner.lock"))
        .unwrap();
    profile_lock.try_lock().unwrap();
    let descriptor: Value =
        serde_json::from_str(&std::env::var("SWARMIE_ADMISSION_CHILD_NODE").unwrap()).unwrap();
    let account: Value =
        serde_json::from_str(&std::env::var("SWARMIE_ADMISSION_CHILD_ACCOUNT").unwrap()).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    tokio::task::LocalSet::new().block_on(&runtime, async {
        let host = tembra::Host::new(Executor);
        host.install(swarmie_node::composition::orbs(&host, swarmie_node::composition::ServicesConfiguration { environment: None,
            state: profile.clone(), node: swarmie_runtime::NodeId::new(descriptor["id"].as_str().unwrap()).unwrap(),
            oauth_host: uuid::Uuid::new_v4(), principal: descriptor["principal"].as_str().unwrap().into(),
            directory_root: None, git: None, agent_git_authority: None,
        }, None).unwrap()).unwrap();
        let principal = descriptor["principal"].as_str().unwrap().to_string();
        let input = json!({"conversationId":"restart-admission","submissionId":"restart-receipt","createConversation":true,"draft":{"text":"Hello","attachments":[],"account":account["id"],"model":"fixture-model","effort":"","node":descriptor["id"]},"mode":"queue"});
        let mut input = input;
        if let Ok(text) = std::env::var("SWARMIE_ADMISSION_CHILD_TEXT") {
            input["draft"]["text"] = json!(text);
        }
        let output = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Value>::new()));
        let delivered = output.clone();
        let connection = tembra::rpc::serve(host, tembra::rpc::Options::new(principal, |_, _| true), move |frame| {
            delivered.borrow_mut().push(serde_json::from_slice(&frame).unwrap());
            Ok(())
        });
        connection.receive(&serde_json::to_vec(&json!({"id":1,"op":"get","payload":{"service":"swarmie.conversation.session","key":["restart-admission"]}})).unwrap()).unwrap();
        let deadline = Instant::now() + WAIT;
        let reference = loop {
            if let Some(reply) = output.borrow_mut().pop() {
                assert!(reply.get("error").is_none(), "{reply}");
                break reply["ok"]["reference"].clone();
            }
            assert!(Instant::now() < deadline, "the child session did not open");
            tokio::task::yield_now().await;
        };
        connection.receive(&serde_json::to_vec(&json!({"id":2,"op":"call","payload":{"reference":reference,"method":"run","arguments":[input]}})).unwrap()).unwrap();
        let deadline = Instant::now() + WAIT;
        loop {
            let index = profile.join("conversations.json");
            if index.exists() {
                use swarmie_durable_tasks::TaskStorage;
                let metadata: Value = serde_json::from_slice(&std::fs::read(index).unwrap()).unwrap();
                if let Some(id) = metadata[0].get("task") {
                    let storage = swarmie_storage::sqlite::SqliteOwner::new(profile.join("conversations.sqlite")).tasks().unwrap();
                    let task = storage.get(serde_json::from_value(id.clone()).unwrap()).await.unwrap();
                    storage.close().await.unwrap();
                    if let Some(task) = task {
                        assert!(matches!(task.status, swarmie_durable_tasks::TaskStatus::Ready));
                        assert!(task.assignment.is_none());
                        assert!(task.checkpoint.is_none());
                        if std::env::var("SWARMIE_ADMISSION_CHILD_STEER").as_deref() == Ok("1") {
                            connection.receive(&serde_json::to_vec(&json!({"op":"cancel","payload":{"id":2}})).unwrap()).unwrap();
                            loop {
                                if output.borrow().iter().any(|reply| reply["id"] == 2) {
                                    break;
                                }
                                assert!(Instant::now() < deadline, "the cancelled observer did not settle");
                                tokio::task::yield_now().await;
                            }
                            let mut steered = input.clone();
                            steered["submissionId"] = json!("steer-after-detach");
                            steered["mode"] = json!("steer");
                            connection.receive(&serde_json::to_vec(&json!({"id":3,"op":"call","payload":{"reference":reference,"method":"run","arguments":[steered]}})).unwrap()).unwrap();
                            loop {
                                let metadata: Value = serde_json::from_slice(&std::fs::read(profile.join("conversations.json")).unwrap()).unwrap();
                                if metadata[0]["stopRequested"] == true {
                                    break;
                                }
                                assert!(Instant::now() < deadline, "steer did not stop the detached admitted task");
                                tokio::task::yield_now().await;
                            }
                        }
                        // End without host cleanup after atomic user/task admission, before dispatch.
                        std::process::exit(0);
                    }
                }
            }
            assert!(Instant::now() < deadline, "durable admission did not appear");
            tokio::task::yield_now().await;
        }
    });
}

#[test]
fn restarting_the_real_client_executes_a_durably_admitted_turn_once() {
    restart_admitted_turn(false);
}

#[test]
fn steering_a_detached_queued_turn_is_persisted_before_restart_without_model_dispatch() {
    restart_admitted_turn(true);
}

fn restart_admitted_turn(steer: bool) {
    let (endpoint, server) = provider(if steer {
        vec![]
    } else {
        vec![answer("Recovered admitted turn")]
    });
    let directory = tempfile::tempdir().unwrap();
    let node = Node::open(directory.path().into()).unwrap();
    let channel = node.open_shared_channel().unwrap();
    let account = configure(&node, channel, &endpoint);
    let descriptor = node.descriptor();
    drop(node);
    admit_in_child(directory.path(), &descriptor, &account, steer, "Hello");
    let before = saved_turn(directory.path(), "restart-admission");
    assert!(before.assignment.is_none());
    assert!(before.checkpoint.is_none());
    let node = Node::open(directory.path().into()).unwrap();
    let channel = node.open_shared_channel().unwrap();
    let deadline = Instant::now() + WAIT;
    loop {
        let object = session(&node, channel, "restart-admission");
        if object["state"]["phase"] == if steer { "stopped" } else { "completed" }
            && matches!(
                saved_turn(directory.path(), "restart-admission").status,
                swarmie_durable_tasks::TaskStatus::Completed(_)
            )
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the admitted turn was not recovered: {object}"
        );
        thread::sleep(Duration::from_millis(1));
    }
    let after = saved_turn(directory.path(), "restart-admission");
    assert_eq!(before.spec.id, after.spec.id);
    assert!(matches!(
        after.status,
        swarmie_durable_tasks::TaskStatus::Completed(_)
    ));
    assert!(after.assignment.is_none());
    if steer {
        assert!(after.checkpoint.is_none());
    } else {
        assert_eq!(after.checkpoint.unwrap()["phase"]["kind"], "finished");
    }
    assert_eq!(
        server.join().unwrap().len(),
        if steer { 1 } else { 2 },
        "one catalog read, and a model call only for the uninterrupted task"
    );
}

fn admit_in_child(profile: &Path, descriptor: &Value, account: &Value, steer: bool, text: &str) {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "admitted_turn_bootstrap_child",
            "--nocapture",
        ])
        .env("SWARMIE_ADMISSION_CHILD_PROFILE", profile)
        .env("SWARMIE_ADMISSION_CHILD_NODE", descriptor.to_string())
        .env("SWARMIE_ADMISSION_CHILD_ACCOUNT", account.to_string())
        .env("SWARMIE_ADMISSION_CHILD_TEXT", text)
        .env(
            "SWARMIE_ADMISSION_CHILD_STEER",
            if steer { "1" } else { "0" },
        )
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{} {}",
        String::from_utf8_lossy(&child.stdout),
        String::from_utf8_lossy(&child.stderr)
    );
}

fn client_answer_sample(prior: usize) -> u64 {
    let (endpoint, server) = provider(vec![answer("Done")]);
    let directory = tempfile::tempdir().unwrap();
    let node = Node::open(directory.path().into()).unwrap();
    let channel = node.open_shared_channel().unwrap();
    let account = configure(&node, channel, &endpoint);
    if prior != 0 {
        seed_history(
            directory.path(),
            &node.descriptor(),
            &account,
            "performance",
            "stopped",
            (0..prior)
                .map(|_| swarmie_agent_loop::Message::User {
                    content: "x".repeat(128),
                })
                .collect(),
        );
    }
    let object = session(&node, channel, "performance");
    let mut input = submission(&node, &account, "performance", "measured-turn");
    input["draft"]["text"] = json!("Add one");
    let started = Instant::now();
    let result = call(&node, channel, &object["reference"], "run", json!([input]));
    let elapsed = started.elapsed().as_nanos() as u64;
    assert_eq!(result["ok"]["status"], "settled", "{result}");
    let task = saved_turn(directory.path(), "performance");
    assert!(
        matches!(task.status, swarmie_durable_tasks::TaskStatus::Completed(_)),
        "{:?}",
        task.status
    );
    assert!(task.assignment.is_none());
    assert_eq!(task.checkpoint.unwrap()["phase"]["kind"], "finished");
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 2);
    let payload: Value =
        serde_json::from_str(requests[1].split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(
        payload["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["role"] == "user")
            .count(),
        prior + 1
    );
    let snapshot = session(&node, channel, "performance");
    assert_eq!(snapshot["state"]["phase"], "completed");
    assert!(snapshot["state"]["messages"].as_array().unwrap().len() <= 24);
    assert!(serde_json::to_vec(&snapshot["state"]).unwrap().len() <= 48 * 1024);
    elapsed
}

#[test]
fn large_history_preserves_full_model_input_and_a_bounded_view() {
    client_answer_sample(1000);
}

#[test]
#[ignore = "manual optimized client performance probe; no timing gate in tests"]
fn persisted_client_answer_performance() {
    let prior: usize = std::env::var("SWARMIE_PERF_PRIOR")
        .unwrap_or_else(|_| "0".into())
        .parse()
        .unwrap();
    let repetitions: usize = std::env::var("SWARMIE_PERF_REPETITIONS")
        .unwrap_or_else(|_| "20".into())
        .parse()
        .unwrap();
    assert!([0, 100, 1000].contains(&prior));
    assert!([5, 20, 60].contains(&repetitions));
    let mut samples = Vec::new();
    for iteration in 0..repetitions + 5 {
        let elapsed = client_answer_sample(prior);
        if iteration >= 5 {
            samples.push(elapsed);
        }
    }
    println!(
        "PERF_CLIENT_TURN {}",
        json!({"prior_messages":prior,"message_bytes":128,"warmups":5,"turn_samples_ns":samples,"model_calls":1,"completion_verified":true,"scope":"native client RPC user admission through durable task release and presentation refresh; loopback HTTP streaming provider; setup, history seed, assertions and close excluded"})
    );
}

#[test]
fn a_presentation_write_failure_does_not_hold_a_confirmed_model_answer() {
    let (entered, started) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let (endpoint, server) = provider(vec![Reply {
        status: 200,
        first: delta("Confirmed answer", false),
        tail: delta("", true),
        gate: Some((entered, released)),
    }]);
    let profile = tempfile::tempdir().unwrap();
    let node = Node::open(profile.path().into()).unwrap();
    let channel = node.open_shared_channel().unwrap();
    let account = configure(&node, channel, &endpoint);
    let object = session(&node, channel, "presentation-failure");
    start(
        &node,
        channel,
        90,
        &object["reference"],
        "run",
        json!([submission(
            &node,
            &account,
            "presentation-failure",
            "presentation-receipt"
        )]),
    );
    started.recv_timeout(WAIT).unwrap();
    let index = profile.path().join("conversations.json");
    let backup = profile.path().join("fixture-index-backup");
    std::fs::rename(&index, &backup).unwrap();
    std::fs::create_dir(&index).unwrap();
    release.send(()).unwrap();
    let result = response(&node, channel, 90);
    assert_eq!(result["ok"]["status"], "settled", "{result}");
    std::fs::remove_dir(&index).unwrap();
    std::fs::rename(&backup, &index).unwrap();
    let snapshot = session(&node, channel, "presentation-failure");
    assert_eq!(snapshot["state"]["phase"], "completed", "{snapshot}");
    assert!(
        snapshot["state"]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|message| message["text"] == "Confirmed answer")
    );
    assert_eq!(server.join().unwrap().len(), 2);
}

#[test]
fn lease_loss_overrides_cached_final_phases_and_stop_unblocks_the_next_turn() {
    use swarmie_durable_tasks::{CommitOutcome, TaskChange, TaskRecord, TaskStatus, TaskStorage};
    let (entered, started) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let (endpoint, server) = provider(vec![
        Reply {
            status: 200,
            first: delta("Unconfirmed stream", false),
            tail: delta("", true),
            gate: Some((entered, released)),
        },
        answer("Explicit next turn"),
    ]);
    let profile = tempfile::tempdir().unwrap();
    let node = Node::open(profile.path().into()).unwrap();
    let channel = node.open_shared_channel().unwrap();
    let account = configure(&node, channel, &endpoint);
    let object = session(&node, channel, "lease-loss");
    start(
        &node,
        channel,
        90,
        &object["reference"],
        "run",
        json!([submission(&node, &account, "lease-loss", "lease-receipt")]),
    );
    started.recv_timeout(WAIT).unwrap();
    let index = profile.path().join("conversations.json");
    let metadata: Value = serde_json::from_slice(&std::fs::read(&index).unwrap()).unwrap();
    let id = serde_json::from_value(metadata[0]["task"].clone()).unwrap();
    let storage =
        swarmie_storage::sqlite::SqliteOwner::new(profile.path().join("conversations.sqlite"))
            .tasks()
            .unwrap();
    // Shorten the current assignment through the real storage contract. The next
    // checkpoint or renewal must reject the expired token and revoke the attempt.
    futures::executor::block_on(async {
        let task = storage.get(id).await.unwrap().unwrap();
        let assignment = task.assignment.unwrap();
        assert!(matches!(
            storage
                .commit(vec![TaskChange::Renew {
                    id,
                    attempt: assignment.attempt,
                    lease_ms: std::num::NonZeroU64::new(1).unwrap(),
                }])
                .await
                .unwrap(),
            CommitOutcome::Applied { .. }
        ));
    });
    thread::sleep(Duration::from_millis(20));
    release.send(()).unwrap();
    let result = response(&node, channel, 90);
    assert_eq!(result["ok"]["status"], "settled", "{result}");
    let task: TaskRecord = futures::executor::block_on(storage.get(id))
        .unwrap()
        .unwrap();
    assert_eq!(task.status, TaskStatus::Ready);
    assert!(task.assignment.is_none());
    for stale in [None, Some("stopped"), Some("unknown")] {
        if let Some(stale) = stale {
            let mut metadata: Value =
                serde_json::from_slice(&std::fs::read(&index).unwrap()).unwrap();
            metadata[0]["phase"] = json!(stale);
            std::fs::write(&index, serde_json::to_vec(&metadata).unwrap()).unwrap();
        }
        let snapshot = session(&node, channel, "lease-loss");
        assert_eq!(snapshot["state"]["phase"], "interrupted", "{snapshot}");
        assert_eq!(snapshot["state"]["canAcknowledge"], false);
    }
    let stopped = call(&node, channel, &object["reference"], "stop", json!([]));
    assert!(stopped.get("error").is_none(), "{stopped}");
    assert_eq!(
        session(&node, channel, "lease-loss")["state"]["phase"],
        "stopped"
    );
    let next = call(
        &node,
        channel,
        &object["reference"],
        "run",
        json!([submission(
            &node,
            &account,
            "lease-loss",
            "next-explicit-receipt"
        )]),
    );
    assert_eq!(next["ok"]["status"], "settled", "{next}");
    assert_eq!(
        session(&node, channel, "lease-loss")["state"]["phase"],
        "completed"
    );
    futures::executor::block_on(storage.close()).unwrap();
    assert_eq!(
        server.join().unwrap().len(),
        3,
        "the interrupted turn must not dispatch another model request"
    );
}

struct PrivateAttachment(*mut std::ffi::c_void);
impl PrivateAttachment {
    fn new(node: &Node) -> Self {
        let pointer = unsafe { swarmie_desktop_runtime::swarmie_native_start(node) };
        assert!(!pointer.is_null());
        Self(pointer)
    }
    fn request(&self, id: u64, op: &str, payload: Value) -> Value {
        let bytes = serde_json::to_vec(&json!({"id":id,"op":op,"payload":payload})).unwrap();
        assert!(unsafe {
            swarmie_desktop_runtime::swarmie_native_send(self.0, bytes.as_ptr(), bytes.len())
        });
        let mut output = zeroize::Zeroizing::new(vec![0; 400_000]);
        let deadline = Instant::now() + WAIT;
        loop {
            let size = unsafe {
                swarmie_desktop_runtime::swarmie_native_receive(
                    self.0,
                    output.as_mut_ptr(),
                    output.len(),
                )
            };
            assert!(size >= 0);
            if size > 0 {
                let response: Value = serde_json::from_slice(&output[..size as usize]).unwrap();
                if response["id"] == id {
                    return response;
                }
            }
            assert!(Instant::now() < deadline, "private request timed out");
            thread::sleep(Duration::from_millis(1));
        }
    }
    fn get(&self) -> Value {
        self.request(
            1,
            "get",
            json!({"service":"swarmie.vault.control","key":["main"]}),
        )["ok"]
            .clone()
    }
    fn call(&self, reference: &Value, method: &str, arguments: Value) -> Value {
        self.request(
            2,
            "call",
            json!({"reference":reference,"method":method,"arguments":arguments}),
        )
    }
}
impl Drop for PrivateAttachment {
    fn drop(&mut self) {
        unsafe {
            swarmie_desktop_runtime::swarmie_native_close(self.0);
        }
    }
}

#[test]
fn actual_client_guard_blocks_before_http_and_private_once_override_does_not_cover_the_next_attempt()
 {
    use std::sync::atomic::Ordering;
    let (endpoint, server, requests) = observed_provider(vec![answer("Explicitly allowed")]);
    let directory = tempfile::tempdir().unwrap();
    let node = Node::open(directory.path().into()).unwrap();
    let channel = node.open_shared_channel().unwrap();
    let private = PrivateAttachment::new(&node);
    let private_object = private.get();
    let account = configure(&node, channel, &endpoint);
    let id = uuid::Uuid::new_v4().to_string();
    let object = session(&node, channel, &id);
    let owner_conversation = object["state"]["ownerConversationId"].as_str().unwrap();
    let alias = session(&node, channel, owner_conversation);
    assert_ne!(
        alias["state"]["ownerConversationId"], object["state"]["ownerConversationId"],
        "a session key equal to another native identity must not alias its cards or approvals"
    );
    let token = format!("ghp_{}", "aB3dE5gH7jK9mN1pQ3sT5vW7yZ9bC1fG3hJ5");
    assert!(
        private
            .call(&private_object["reference"], "page", json!(["cards", 0]))
            .get("error")
            .is_none()
    );
    for (index, choice) in ["allow-once", "cancel"].iter().enumerate() {
        let mut input = submission(&node, &account, &id, &format!("guard-{index}"));
        input["draft"]["text"] = json!(format!("Inspect this synthetic token: {token}"));
        start(
            &node,
            channel,
            100 + index as u64,
            &object["reference"],
            "run",
            json!([input]),
        );
        let deadline = Instant::now() + WAIT;
        let card = loop {
            let snapshot = private.call(&private_object["reference"], "read", json!([]));
            assert!(snapshot.get("error").is_none(), "{snapshot}");
            if let Some(card) = snapshot["ok"]["cards"]
                .as_array()
                .and_then(|cards| cards.first())
            {
                break card.clone();
            }
            assert!(
                Instant::now() < deadline,
                "actual client did not publish its guard decision: {snapshot}"
            );
            thread::sleep(Duration::from_millis(2));
        };
        assert_eq!(card["kind"], "model");
        assert_eq!(card["conversation"], owner_conversation);
        assert!(!serde_json::to_string(&card).unwrap().contains(&token));
        assert_eq!(requests.load(Ordering::SeqCst), 1 + index);
        let result = private.call(
            &private_object["reference"],
            "decide",
            json!([card["id"], choice]),
        );
        assert!(result.get("error").is_none(), "{result}");
        let result = response(&node, channel, 100 + index as u64);
        assert_eq!(result["ok"]["status"], "settled", "{result}");
        assert_eq!(
            session(&node, channel, &id)["state"]["phase"],
            if index == 0 { "completed" } else { "failed" }
        );
    }
    let emitted = server.join().unwrap();
    assert_eq!(emitted.len(), 2);
    assert!(
        emitted[1].contains(&token),
        "the exact human-approved original request is sent"
    );
    assert_eq!(requests.load(Ordering::SeqCst), 2);
}

#[test]
fn recovered_turn_remains_guarded_after_its_public_observer_closes() {
    recovered_guarded_turn(false);
}

#[test]
fn stopping_a_recovered_guarded_turn_retires_its_private_decision_without_http_dispatch() {
    recovered_guarded_turn(true);
}

fn recovered_guarded_turn(stop: bool) {
    use std::sync::atomic::Ordering;
    let (endpoint, server, requests) = observed_provider(if stop {
        vec![]
    } else {
        vec![answer("Recovered with explicit permission")]
    });
    let directory = tempfile::tempdir().unwrap();
    let node = Node::open(directory.path().into()).unwrap();
    let channel = node.open_shared_channel().unwrap();
    let account = configure(&node, channel, &endpoint);
    let descriptor = node.descriptor();
    drop(node);
    let token = format!("ghp_{}", "aB3dE5gH7jK9mN1pQ3sT5vW7yZ9bC1fG3hJ5");
    admit_in_child(directory.path(), &descriptor, &account, false, &token);
    let admitted = saved_turn(directory.path(), "restart-admission");
    assert!(admitted.assignment.is_none());
    let node = Node::open(directory.path().into()).unwrap();
    let channel = node.open_shared_channel().unwrap();
    let object = session(&node, channel, "restart-admission");
    let private = PrivateAttachment::new(&node);
    let private_object = private.get();
    let conversation = uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_URL,
        format!(
            "{}/{}/restart-admission",
            descriptor["id"].as_str().unwrap(),
            descriptor["principal"].as_str().unwrap()
        )
        .as_bytes(),
    );
    let result = private.call(&private_object["reference"], "page", json!(["cards", 0]));
    assert!(result.get("error").is_none(), "{result}");
    let deadline = Instant::now() + WAIT;
    let card = loop {
        let snapshot = private.call(&private_object["reference"], "read", json!([]));
        assert!(snapshot.get("error").is_none(), "{snapshot}");
        if let Some(card) = snapshot["ok"]["cards"]
            .as_array()
            .and_then(|cards| cards.first())
        {
            break card.clone();
        }
        assert!(
            Instant::now() < deadline,
            "the recovered attempt bypassed its guard: {snapshot}"
        );
        thread::sleep(Duration::from_millis(2));
    };
    assert_eq!(card["conversation"], conversation.to_string());
    assert_eq!(
        requests.load(Ordering::SeqCst),
        1,
        "only model discovery may have reached HTTP"
    );
    assert!(!card.to_string().contains(&token));
    let observer = if stop {
        let result = call(&node, channel, &object["reference"], "stop", json!([]));
        assert!(result.get("error").is_none(), "{result}");
        let late = private.call(
            &private_object["reference"],
            "decide",
            json!([card["id"], "allow-once"]),
        );
        assert!(
            late.get("error").is_some(),
            "a physically stopped attempt cannot accept a late approval: {late}"
        );
        channel
    } else {
        node.close_channel(channel);
        let result = private.call(
            &private_object["reference"],
            "decide",
            json!([card["id"], "allow-once"]),
        );
        assert!(
            result.get("error").is_none(),
            "closing an observer must retain the task's native owner: {result}"
        );
        node.open_shared_channel().unwrap()
    };
    loop {
        let current = session(&node, observer, "restart-admission");
        if current["state"]["phase"] == if stop { "stopped" } else { "completed" } {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the recovered attempt did not settle: {current}"
        );
        thread::sleep(Duration::from_millis(2));
    }
    let settled = saved_turn(directory.path(), "restart-admission");
    assert_eq!(settled.spec.id, admitted.spec.id);
    let expected = if stop { 1 } else { 2 };
    let sent = server.join().unwrap();
    assert_eq!(sent.len(), expected);
    assert_eq!(requests.load(Ordering::SeqCst), expected);
    if !stop {
        assert!(sent[1].contains(&token));
    }
}

#[test]
fn a_signed_out_vault_tool_is_blocked_before_dispatch_and_the_turn_can_finish() {
    let tool = Reply {
        status: 200,
        first: format!(
            "{}\n",
            json!({"model":"fixture-model","message":{"role":"assistant","content":"","tool_calls":[{"id":"vault-unavailable-call","function":{"name":"vault_list","arguments":{}}}]},"done":false})
        ),
        tail: format!(
            "{}\n",
            json!({"model":"fixture-model","message":{"role":"assistant","content":""},"done":true,"done_reason":"stop","prompt_eval_count":3,"eval_count":5})
        ),
        gate: None,
    };
    let (endpoint, server) = provider(vec![tool, answer("Sign in to use the vault")]);
    let directory = tempfile::tempdir().unwrap();
    let node = Node::open(directory.path().into()).unwrap();
    let channel = node.open_shared_channel().unwrap();
    let account = configure(&node, channel, &endpoint);
    let object = session(&node, channel, "vault-unavailable");
    let input = submission(&node, &account, "vault-unavailable", "first");
    let result = call(&node, channel, &object["reference"], "run", json!([input]));
    assert_eq!(result["ok"]["status"], "settled", "{result}");
    let current = session(&node, channel, "vault-unavailable");
    assert_eq!(current["state"]["phase"], "completed");
    assert_eq!(current["state"]["canAcknowledge"], false);
    let messages = canonical_messages(directory.path(), &node.descriptor(), "vault-unavailable");
    assert!(messages.into_iter().any(|record| matches!(
        serde_json::from_value::<swarmie_agent_loop::conversation::Record>(record).unwrap(),
        swarmie_agent_loop::conversation::Record::Message(swarmie_agent_loop::Message::Tool {
            result: swarmie_agent_loop::ToolResult::Blocked(_),
            ..
        })
    )));
    assert_eq!(server.join().unwrap().len(), 3);
}

#[test]
fn two_conversations_publish_private_decisions_together_without_selecting_one_over_the_other() {
    let (endpoint, server) = provider(vec![]);
    let directory = tempfile::tempdir().unwrap();
    let node = Node::open(directory.path().into()).unwrap();
    let channel = node.open_shared_channel().unwrap();
    let account = configure(&node, channel, &endpoint);
    let private = PrivateAttachment::new(&node);
    let control = private.get();
    let mut owners = Vec::new();
    for (index, id) in ["first-private-chat", "second-private-chat"]
        .into_iter()
        .enumerate()
    {
        let observer = node.open_shared_channel().unwrap();
        let object = session(&node, observer, id);
        owners.push(object["state"]["ownerConversationId"].clone());
        let mut input = submission(&node, &account, id, &format!("guarded-{index}"));
        input["draft"]["text"] = json!(format!("ghp_{}", "aB3dE5gH7jK9mN1pQ3sT5vW7yZ9bC1fG3hJ5"));
        start(
            &node,
            observer,
            100 + index as u64,
            &object["reference"],
            "run",
            json!([input]),
        );
    }
    let deadline = Instant::now() + WAIT;
    let cards = loop {
        let snapshot = private.call(&control["reference"], "read", json!([]));
        assert!(snapshot.get("error").is_none(), "{snapshot}");
        let cards = snapshot["ok"]["cards"].as_array().unwrap();
        if cards.len() == 2 {
            for owner in &owners {
                assert_eq!(snapshot["ok"]["cardCounts"][owner.as_str().unwrap()], 1);
            }
            break cards.clone();
        }
        assert!(
            Instant::now() < deadline,
            "both conversations must remain visible: {snapshot}"
        );
        thread::sleep(Duration::from_millis(2));
    };
    for owner in owners {
        assert!(cards.iter().any(|card| card["conversation"] == owner));
    }
    for card in cards {
        let result = private.call(
            &control["reference"],
            "decide",
            json!([card["id"], "cancel"]),
        );
        assert!(result.get("error").is_none(), "{result}");
    }
    assert_eq!(
        server.join().unwrap().len(),
        1,
        "only model discovery may dispatch"
    );
}
