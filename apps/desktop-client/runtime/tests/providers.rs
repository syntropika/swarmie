use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use swarmie_desktop_runtime::DesktopNode;

fn request(node: &DesktopNode, channel: u64, id: u64, op: &str, payload: Value) -> Value {
    node.frame(
        channel,
        serde_json::to_vec(&json!({"id":id,"op":op,"payload":payload})).unwrap(),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        for bytes in node.poll(channel).unwrap() {
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            if value["id"] == id {
                return value;
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    panic!("native RPC response timed out");
}

fn get(node: &DesktopNode, channel: u64, service: &str, key: &str) -> Value {
    let value = request(
        node,
        channel,
        1,
        "get",
        json!({"service":service,"key":[key]}),
    );
    assert!(value.get("error").is_none(), "{value}");
    value["ok"].clone()
}

fn call(
    node: &DesktopNode,
    channel: u64,
    reference: &Value,
    method: &str,
    arguments: Value,
) -> Value {
    request(
        node,
        channel,
        2,
        "call",
        json!({"reference":reference,"method":method,"arguments":arguments}),
    )
}

#[test]
fn shared_channels_are_independent_bounded_and_exclusively_replaceable() {
    let state = tempfile::tempdir().unwrap();
    let node = DesktopNode::open(state.path().into()).unwrap();
    let first = node.open_shared_channel().unwrap();
    let second = node.open_shared_channel().unwrap();
    let one = get(&node, first, "swarmie.providers.settings", "main");
    let two = get(&node, second, "swarmie.providers.settings", "main");
    node.close_channel(first);
    assert!(node.poll(first).is_err());
    let catalog = call(&node, second, &two["reference"], "list", json!([null]));
    assert_eq!(
        catalog["ok"]["choices"][0]["methods"][0]["id"],
        "oauth-browser"
    );
    assert!(catalog["ok"]["accounts"].as_array().unwrap().is_empty());
    assert!(one.get("reference").is_some());
    let mut channels = vec![second];
    for _ in 1..8 {
        channels.push(node.open_shared_channel().unwrap());
    }
    assert!(node.open_shared_channel().is_err());
    let replacement = node.open_channel().unwrap();
    for previous in channels {
        assert!(node.poll(previous).is_err());
    }
    assert!(node.poll(replacement).is_ok());
}

#[test]
fn native_provider_configuration_executes_and_restores_history_without_replaying_a_submission() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let calls = Arc::new(Mutex::new(Vec::<String>::new()));
    let captured = calls.clone();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(8);
        let replies = [
            (
                "application/json",
                r#"{"models":[{"name":"fixture-model"}]}"#,
            ),
            (
                "application/x-ndjson",
                include_str!("../../../../tests/fixtures/providers/ollama-response.ndjson"),
            ),
        ];
        for (content_type, body) in replies {
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "provider fixture timed out");
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut data = Vec::new();
            let (header_end, length) = loop {
                let mut buffer = [0; 4096];
                let count = stream.read(&mut buffer).unwrap();
                assert!(count > 0);
                data.extend_from_slice(&buffer[..count]);
                if let Some(at) = data.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&data[..at]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.split_once(':')
                                .filter(|(key, _)| key.eq_ignore_ascii_case("content-length"))
                                .map(|(_, value)| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    captured
                        .lock()
                        .unwrap()
                        .push(headers.lines().next().unwrap().to_owned());
                    break (at + 4, length);
                }
            };
            while data.len() < header_end + length {
                let mut buffer = [0; 4096];
                let count = stream.read(&mut buffer).unwrap();
                assert!(count > 0);
                data.extend_from_slice(&buffer[..count]);
            }
            write!(stream, "HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
    });
    let state = tempfile::tempdir().unwrap();
    let submission;
    {
        let node = DesktopNode::open(state.path().into()).unwrap();
        let channel = node.open_shared_channel().unwrap();
        let providers = get(&node, channel, "swarmie.providers.settings", "main");
        let configured = call(
            &node,
            channel,
            &providers["reference"],
            "configure",
            json!([{"providerId":"ollama","method":"local","url":endpoint}]),
        );
        assert!(configured.get("error").is_none(), "{configured}");
        submission = json!({"conversationId":"client-fixture","submissionId":"submission-fixture","createConversation":true,"draft":{"text":"Hello","attachments":[],"account":configured["ok"]["id"],"model":"fixture-model","effort":"","node":node.descriptor()["id"]},"mode":"queue"});
        let session = get(
            &node,
            channel,
            "swarmie.conversation.session",
            "client-fixture",
        );
        let result = call(
            &node,
            channel,
            &session["reference"],
            "run",
            json!([submission]),
        );
        assert!(result.get("error").is_none(), "{result}");
        let session = get(
            &node,
            channel,
            "swarmie.conversation.session",
            "client-fixture",
        );
        assert_eq!(session["state"]["phase"], "completed", "{session}");
        assert_eq!(
            session["state"]["messages"][1]["text"],
            "Hello from the provider."
        );
        assert_eq!(session["state"]["admitted"], json!(["submission-fixture"]));
        let duplicate = call(
            &node,
            channel,
            &session["reference"],
            "run",
            json!([submission]),
        );
        assert_eq!(duplicate["ok"]["status"], "settled", "{duplicate}");
    }
    server.join().unwrap();
    assert_eq!(
        *calls.lock().unwrap(),
        ["GET /api/tags HTTP/1.1", "POST /api/chat HTTP/1.1"]
    );
    let node = DesktopNode::open(state.path().into()).unwrap();
    let channel = node.open_shared_channel().unwrap();
    let session = get(
        &node,
        channel,
        "swarmie.conversation.session",
        "client-fixture",
    );
    let snapshot = call(&node, channel, &session["reference"], "snapshot", json!([]));
    assert!(snapshot.get("error").is_none(), "{snapshot}");
    let session = get(
        &node,
        channel,
        "swarmie.conversation.session",
        "client-fixture",
    );
    assert_eq!(session["state"]["phase"], "completed");
    assert_eq!(session["state"]["messages"].as_array().unwrap().len(), 2);
    let catalog = get(&node, channel, "swarmie.conversation.catalog", "main");
    let listed = call(&node, channel, &catalog["reference"], "list", json!([null]));
    assert_eq!(listed["ok"]["entries"][0]["id"], "client-fixture");
    assert!(listed["ok"]["entries"][0].get("receipts").is_none());
    assert!(listed["ok"]["entries"][0].get("principal").is_none());
    assert!(
        call(
            &node,
            channel,
            &session["reference"],
            "run",
            json!([submission])
        )["ok"]["status"]
            == "settled"
    );
    drop(node);
    let index = state.path().join("conversations.json");
    let mut metadata: Value = serde_json::from_slice(&std::fs::read(&index).unwrap()).unwrap();
    metadata[0]["phase"] = json!("working");
    metadata[0]["receipts"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"unconfirmed","digest":"fixture-digest"}));
    std::fs::write(&index, serde_json::to_vec(&metadata).unwrap()).unwrap();
    let node = DesktopNode::open(state.path().into()).unwrap();
    let channel = node.open_shared_channel().unwrap();
    let session = get(
        &node,
        channel,
        "swarmie.conversation.session",
        "client-fixture",
    );
    assert!(
        call(&node, channel, &session["reference"], "snapshot", json!([]))
            .get("error")
            .is_none()
    );
    let session = get(
        &node,
        channel,
        "swarmie.conversation.session",
        "client-fixture",
    );
    assert_eq!(
        session["state"]["phase"], "completed",
        "the confirmed durable task repairs stale presentation metadata"
    );
    assert_eq!(
        session["state"]["admitted"],
        json!(["submission-fixture"]),
        "preparation is not acceptance"
    );
    assert!(
        call(&node, channel, &session["reference"], "stop", json!([]))
            .get("error")
            .is_none()
    );
    let session = get(
        &node,
        channel,
        "swarmie.conversation.session",
        "client-fixture",
    );
    assert_eq!(
        session["state"]["phase"], "completed",
        "confirmed task completion repairs stale metadata without replay"
    );
    let mut next = submission;
    next["submissionId"] = json!("next-submission");
    next["draft"]["model"] = json!("unavailable-model");
    let refused = call(&node, channel, &session["reference"], "run", json!([next]));
    assert_eq!(refused["ok"]["status"], "refused");
    let session = get(
        &node,
        channel,
        "swarmie.conversation.session",
        "client-fixture",
    );
    assert_eq!(session["state"]["messages"].as_array().unwrap().len(), 2);
}
