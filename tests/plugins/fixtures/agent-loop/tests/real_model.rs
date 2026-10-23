//! The actual model adapter streams a tool turn through the complete Agent composition.

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
    time::Duration,
};
use swarmie_agent_contracts::{AgentLoop, AgentLoopInput, TurnLimits};
use swarmie_agent_loop_fixture::{
    policy::{self, PolicyProbe},
    tool::{self, ToolProbe},
};
use swarmie_model_contracts::ModelIdentity;
use swarmie_model_provider_genai::{self as adapter, Connection, Credentials};
use swarmie_session_contracts::{EventQuery, MESSAGE, SessionStore, SessionStoreInput};
use swarmie_task_contracts::{
    DurableTasks, DurableTasksInput, TaskLookup, TaskOutcome, TaskStorage, TaskStorageInput,
};
use tembra::{Host, core};

pub mod conversation_support;
pub mod support;

/// Only the remote HTTP boundary is simulated; Contracts and managed providers remain real.
struct LocalCompletionServer {
    url: String,
    requests: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl LocalCompletionServer {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/v1/", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let received = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let worker = thread::spawn(move || {
            let mut replies = VecDeque::from([
                vec![
                    json!({"choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"local-call-42","type":"function","function":{"name":"fixture.double","arguments":"{\"value\":21}"}}]}}]}),
                    json!({"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":5,"completion_tokens":3,"total_tokens":8}}),
                ],
                vec![
                    json!({"choices":[{"index":0,"delta":{"role":"assistant","content":"The answer is 42."}}]}),
                    json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":3,"total_tokens":13}}),
                ],
            ]);
            while !stopping.load(Ordering::SeqCst) {
                let Ok((mut socket, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(2));
                    continue;
                };
                socket
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                socket
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let Some(request) = read_request(&mut socket) else {
                    continue;
                };
                received.lock().unwrap().push(request);
                let Some(frames) = replies.pop_front() else {
                    let _ = socket.write_all(b"HTTP/1.1 500 Unexpected request\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
                    continue;
                };
                let body = frames
                    .into_iter()
                    .map(|frame| format!("data: {frame}\n\n"))
                    .collect::<String>()
                    + "data: [DONE]\n\n";
                let reply = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes());
            }
        });
        Self {
            url,
            requests,
            stop,
            worker: Some(worker),
        }
    }
}

impl Drop for LocalCompletionServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.worker.take().unwrap().join().unwrap();
    }
}

fn read_request(socket: &mut TcpStream) -> Option<Value> {
    let mut data = Vec::new();
    let mut buffer = [0; 4096];
    while data.len() <= 512 * 1024 {
        let read = socket.read(&mut buffer).ok()?;
        if read == 0 {
            return None;
        }
        data.extend_from_slice(&buffer[..read]);
        let Some(end) = data.windows(4).position(|bytes| bytes == b"\r\n\r\n") else {
            continue;
        };
        let headers = String::from_utf8_lossy(&data[..end]);
        let length = headers.lines().find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .and_then(|length| length.trim().parse::<usize>().ok())
        })?;
        if data.len() >= end + 4 + length {
            return serde_json::from_slice(&data[end + 4..end + 4 + length]).ok();
        }
    }
    None
}

struct LocalCredentials;
impl Credentials for LocalCredentials {
    fn resolve(&self, name: &str) -> Option<String> {
        (name == "AGENT_LOOP_LOCAL_HTTP_TEST").then(|| "local-test-placeholder".into())
    }
}

#[test]
fn actual_genai_streaming_tool_turn_preserves_http_calls_and_session_transcript() {
    let server = LocalCompletionServer::new();
    support::run(|driver| async {
        let directory = tempfile::tempdir().unwrap();
        let host = Host::with_driver("real-model-agent-acceptance", "application", driver.clone())?;
        let tool_probe = ToolProbe::default();
        let policy_probe = PolicyProbe::default();
        let model_configuration = adapter::Configuration {
            connections: vec![Connection {
                id: "local".into(),
                provider: "openai".into(),
                models: vec!["fixture-model".into()],
                base_url: Some(server.url.clone()),
                credential_env: Some("AGENT_LOOP_LOCAL_HTTP_TEST".into()),
                timeout_ms: 5000,
                max_concurrent: 2,
                defaults: Default::default(),
            }],
            limits: Default::default(),
        };
        for facet in [
            swarmie_task_storage_sqlite::facet(&swarmie_task_storage_sqlite::Configuration {
                database: directory.path().join("tasks.sqlite"),
                outbox: Default::default(),
            })?,
            swarmie_session_store_sqlite::facet(
                &swarmie_session_store_sqlite::Configuration {
                    database: directory.path().join("sessions.sqlite"),
                },
                driver.clone(),
            )?,
            adapter::facet_with_credentials(
                &model_configuration,
                driver.clone(),
                Rc::new(LocalCredentials),
            )?,
            tool::facet(Default::default(), tool_probe.clone())?,
            policy::facet(true, policy_probe.clone())?,
            swarmie_durable_tasks::facet(
                &swarmie_durable_tasks::Configuration {
                    poll_interval_ms: 2,
                    ..Default::default()
                },
                driver.clone(),
            )?,
        ] {
            host.install(facet, core::LoadOptions::default())?.await?;
        }
        let storage = host.get(TaskStorage, TaskStorageInput {}).await?;
        let sessions = host.get(SessionStore, SessionStoreInput {}).await?;
        host.install(
            swarmie_task_session_integration::facet(
                &swarmie_task_session_integration::Configuration {
                    database: directory.path().join("bindings.sqlite"),
                    task_store_id: storage.outbox_status().await?.store_id,
                    session_store_id: sessions.publication_status().await?.durable.store_id,
                    consumer_id: "actual-model-agent-history".into(),
                    auto_publish: false,
                    poll_interval_ms: 2,
                },
                driver.clone(),
            )?,
            core::LoadOptions::default(),
        )?
        .await?;
        host.install(
            swarmie_agent_loop_default::facet(driver)?,
            core::LoadOptions::default(),
        )?
        .await?;
        conversation_support::seed(
            &sessions,
            "session",
            "conversation",
            "input",
            "Compute 21 doubled.",
        )
        .await?;
        let mut request = conversation_support::request("session", "conversation", "turn", "input");
        request.model = ModelIdentity {
            connection: "local".into(),
            model: "fixture-model".into(),
        };
        request.limits = TurnLimits::default();
        let agent = host.get(AgentLoop, AgentLoopInput {}).await?;
        let started = agent.start(request).await?;
        let tasks = host.get(DurableTasks, DurableTasksInput {}).await?;
        let outcome = tasks
            .wait(TaskLookup {
                id: started.task.id.clone(),
            })
            .await?;
        assert!(
            matches!(outcome, TaskOutcome::Completed { .. }),
            "{outcome:?}"
        );
        let terminal = agent
            .inspect(conversation_support::lookup(
                "session",
                "conversation",
                "turn",
            ))
            .await?
            .unwrap();
        assert_eq!(terminal.task.checkpoint["step"], 1);
        assert_eq!(terminal.task.checkpoint["tool_calls"], 1);
        assert_eq!(tool_probe.calls.borrow().len(), 1);
        assert_eq!(tool_probe.calls.borrow()[0].tool_call_id, "local-call-42");
        assert_eq!(tool_probe.calls.borrow()[0].arguments, json!({"value":21}));
        assert_eq!(policy_probe.calls.borrow().len(), 1);
        let mut query = EventQuery::new("session");
        query.conversation_id = Some("conversation".into());
        let events = sessions.events(query).await?.events;
        let messages = events
            .iter()
            .filter(|entry| entry.event.kind == MESSAGE)
            .map(|entry| &entry.event.payload)
            .collect::<Vec<_>>();
        assert_eq!(
            messages
                .iter()
                .map(|message| message["role"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["user", "assistant", "tool", "assistant"]
        );
        assert_eq!(messages[1]["content"][0]["call"]["id"], "local-call-42");
        assert_eq!(messages[2]["tool_call_id"], "local-call-42");
        assert_eq!(messages[2]["content"][0]["value"], json!({"value":42}));
        assert_eq!(messages[3]["content"][0]["text"], "The answer is 42.");
        assert_eq!(
            events
                .iter()
                .filter(|entry| entry.event.kind == "swarmie.agent.model-provider-confirmed")
                .count(),
            2
        );
        let received = server.requests.lock().unwrap().clone();
        assert_eq!(received.len(), 2);
        assert!(received.iter().all(|request| request["stream"] == true));
        assert_eq!(received[0]["model"], "fixture-model");
        assert!(
            received[0]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tool| tool["function"]["name"] == "fixture.double")
        );
        assert!(
            received[1]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|message| message["role"] == "tool"
                    && message["tool_call_id"] == "local-call-42"
                    && message["content"].as_str().unwrap().contains("42"))
        );
        host.close().await
    });
}
