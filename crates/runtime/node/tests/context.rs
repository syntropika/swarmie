use futures::{
    FutureExt,
    task::{LocalFutureObj, LocalSpawn, SpawnError},
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{cell::RefCell, rc::Rc, time::Duration};
use swarmie_context::{Admission, CompactionRequest, Phase, Receipt, Snapshot};
use swarmie_node::composition::ServicesConfiguration;
use swarmie_providers::OperationId;
use swarmie_runtime::NodeId;
use tembra::Host;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Notify,
};

#[derive(Clone, Copy)]
struct Executor;

impl LocalSpawn for Executor {
    fn spawn_local_obj(&self, future: LocalFutureObj<'static, ()>) -> Result<(), SpawnError> {
        tokio::task::spawn_local(future);
        Ok(())
    }
}

struct Fixture {
    profile: tempfile::TempDir,
    host: Host,
    calls: Rc<RefCell<Vec<Value>>>,
    summary_entered: Rc<Notify>,
    summary_release: Rc<Notify>,
    summary_answer: Rc<RefCell<String>>,
    server: tokio::task::JoinHandle<()>,
}

impl Fixture {
    async fn new(context_limit: u64, output_limit: u64, hold: bool, first_answer: String) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let calls = Rc::new(RefCell::new(Vec::new()));
        let requests = calls.clone();
        let summary_entered = Rc::new(Notify::new());
        let entered = summary_entered.clone();
        let summary_release = Rc::new(Notify::new());
        let release = summary_release.clone();
        let summary_answer: Rc<RefCell<String>> = Rc::new(RefCell::new(
            "Retained goals and accepted decisions.".into(),
        ));
        let answer = summary_answer.clone();
        let server = tokio::task::spawn_local(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let (end, length) = loop {
                    let mut buffer = [0; 4096];
                    let count = socket.read(&mut buffer).await.unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&buffer[..count]);
                    if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]);
                        let length: usize = headers
                            .lines()
                            .filter_map(|line| line.split_once(':'))
                            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                            .unwrap()
                            .1
                            .trim()
                            .parse()
                            .unwrap();
                        if bytes.len() >= end + 4 + length {
                            break (end + 4, length);
                        }
                    }
                };
                let request: Value = serde_json::from_slice(&bytes[end..end + length]).unwrap();
                let summary = request["messages"][0]["content"]
                    .as_str()
                    .is_some_and(|text| {
                        text.starts_with("Create a concise continuation checkpoint")
                    });
                let streaming = request["stream"] == true;
                let first = requests.borrow().is_empty();
                requests.borrow_mut().push(request);
                if summary {
                    entered.notify_one();
                    if hold {
                        release.notified().await;
                    }
                }
                let answer = if summary {
                    answer.borrow().clone()
                } else if first {
                    first_answer.clone()
                } else {
                    "Continued with the complete suffix.".into()
                };
                let body = format!(
                    "{}\n",
                    json!({"model":"fixture-model","message":{"role":"assistant","content":answer},"done":true,"done_reason":"stop","prompt_eval_count":10,"eval_count":5})
                );
                let content_type = if streaming {
                    "application/x-ndjson"
                } else {
                    "application/json"
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                // An explicit Stop can close the request before the fixture replies.
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });
        let profile = tempfile::tempdir().unwrap();
        let claim = |tokens| json!({"tokens":tokens,"precision":"reported","evidence":{"source":"synthetic-native-context","revision":"1","layer":"configuration","observed_at_ms":0,"expires_at_ms":null}});
        std::fs::write(profile.path().join("providers.json"), serde_json::to_vec(&json!([{
            "account":{"id":"fixture-account","providerId":"ollama","providerLabel":"Local","label":"Fixture","models":[{"id":"fixture-model","label":"Fixture","efforts":[]}],"available":true},
            "provider":"ollama","endpoint":endpoint,"oauth":false,"oauth_identity":null,
            "discovered_limits":{"endpoint":endpoint,"models":{"fixture-model":{"context":[claim(context_limit)],"input":[],"output":[claim(output_limit)]}},"capabilities":{"fixture-model":{"tools":true,"streaming":true,"profile":"swarmie.text-tools.v1","evidence":{"source":"synthetic-native-context","revision":"1","layer":"configuration","observed_at_ms":0,"expires_at_ms":null}}}}
        }])).unwrap()).unwrap();
        let host = Self::open(profile.path()).await;
        Self {
            profile,
            host,
            calls,
            summary_entered,
            summary_release,
            summary_answer,
            server,
        }
    }

    async fn open(path: &std::path::Path) -> Host {
        let host = Host::new(Executor);
        let mut orbs = swarmie_node::composition::orbs(
            &host,
            ServicesConfiguration {
                environment: None,
                state: path.into(),
                node: NodeId::new("context-fixture").unwrap(),
                oauth_host: uuid::Uuid::new_v4(),
                principal: "fixture-user".into(),
                directory_root: None,
                git: None,
                agent_git_authority: None,
            },
            None,
        )
        .unwrap();
        orbs.push(swarmie_runtime::node_orb(
            NodeId::new("context-fixture").unwrap(),
        ));
        host.install(orbs).unwrap();
        swarmie_conversation_runtime::start(&host).await.unwrap();
        host
    }

    async fn call<A: Serialize + 'static, R: DeserializeOwned + 'static>(
        &self,
        service: &str,
        method: &str,
        arguments: A,
    ) -> tembra::Result<R> {
        call(&self.host, service, method, arguments).await
    }

    async fn turn(&self, text: &str) {
        self.call::<_, Value>("swarmie.conversation.session", "run", (json!({
            "conversationId":"context-session","submissionId":uuid::Uuid::new_v4().to_string(),"createConversation":true,
            "draft":{"text":text,"attachments":[],"account":"fixture-account","model":"fixture-model","effort":"","node":"context-fixture"},"mode":"queue"
        }),)).await.unwrap();
    }

    async fn inspect(&self) -> Snapshot {
        self.call(swarmie_context::SERVICE, "inspect", ())
            .await
            .unwrap()
    }

    async fn settled(&self) -> Snapshot {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let snapshot = self.inspect().await;
                if snapshot
                    .checkpoint
                    .compaction()
                    .is_some_and(|work| !work.is_pending() || work.stop_requested)
                {
                    return snapshot;
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        })
        .await
        .unwrap_or_else(|error| {
            panic!(
                "context task did not settle: {error}; metadata {}; calls {:?}",
                std::fs::read_to_string(self.profile.path().join("conversations.json")).unwrap(),
                self.calls.borrow()
            )
        })
    }

    async fn close(self) {
        self.host.close().await.unwrap();
        assert_eq!(self.host.kernel().diagnostics().resources, 0);
        self.server.abort();
    }
}

async fn call<A: Serialize + 'static, R: DeserializeOwned + 'static>(
    host: &Host,
    service: &str,
    method: &str,
    arguments: A,
) -> tembra::Result<R> {
    let output = Rc::new(RefCell::new(Vec::<Value>::new()));
    let outgoing = output.clone();
    let connection = tembra::rpc::serve(
        host.clone(),
        tembra::rpc::Options::new("fixture-user".into(), |_, _| true),
        move |frame| {
            outgoing
                .borrow_mut()
                .push(serde_json::from_slice(&frame).unwrap());
            Ok(())
        },
    );
    let send = |id, op, payload| {
        connection
            .receive(&serde_json::to_vec(&json!({"id":id,"op":op,"payload":payload})).unwrap())
            .unwrap()
    };
    let reference = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            send(
                1,
                "get",
                json!({"service":service,"key":["context-session"]}),
            );
            let reply = rpc_response(&output, 1).await;
            if reply["error"]["reason"] != "disposing" {
                break reply;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let arguments = serde_json::to_value(arguments).unwrap();
    let arguments = if arguments.is_null() {
        json!([])
    } else {
        arguments
    };
    if let Some(error) = reference.get("error") {
        return Err(tembra::Error::invalid("fixture.get", error.to_string()));
    }
    send(
        2,
        "call",
        json!({"reference":reference["ok"]["reference"],"method":method,"arguments":arguments}),
    );
    let reply = rpc_response(&output, 2).await;
    if let Some(error) = reply.get("error") {
        return Err(tembra::Error::invalid("fixture.rpc", error.to_string()));
    }
    serde_json::from_value(reply["ok"].clone())
        .map_err(|error| tembra::Error::invalid("fixture.decode", error.to_string()))
}

async fn rpc_response(output: &Rc<RefCell<Vec<Value>>>, id: u64) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let index = output.borrow().iter().position(|value| value["id"] == id);
            if let Some(index) = index {
                return output.borrow_mut().remove(index);
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("native RPC did not respond")
}

#[tokio::test]
async fn manual_command_compacts_once_preserves_history_and_survives_restart() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mut fixture = Fixture::new(128_000, 4_000, false, "Original answer".into()).await;
            fixture.turn("Original user goal").await;
            fixture.turn("Keep this full suffix").await;
            let before = fixture.inspect().await;
            assert!(before.can_compact, "{before:?}");
            let request = CompactionRequest {
                operation: OperationId::new(),
                fence: before.fence.unwrap(),
            };
            let receipt: Receipt = fixture
                .call(
                    "swarmie.commands",
                    "invoke",
                    (swarmie_commands::Invocation {
                        command: swarmie_commands::CommandId::new(swarmie_commands::COMPACT)
                            .unwrap(),
                        operation: request.operation,
                        arguments: json!({"fence":request.fence}),
                    },),
                )
                .await
                .unwrap();
            assert_eq!(receipt.admission, Admission::Admitted);
            let snapshot = fixture.settled().await;
            assert_eq!(
                snapshot.checkpoint.compaction().unwrap().phase,
                Phase::Activated,
                "{snapshot:?}"
            );
            let duplicate: Receipt = fixture
                .call(swarmie_context::SERVICE, "compact", (request,))
                .await
                .unwrap();
            assert_eq!(duplicate.admission, Admission::Recognized);
            assert_eq!(fixture.calls.borrow().len(), 3);
            fixture.host.close().await.unwrap();
            fixture.host = Fixture::open(fixture.profile.path()).await;
            assert!(fixture.inspect().await.checkpoint.active().is_some());
            fixture.turn("New full suffix after restart").await;
            let calls = fixture.calls.borrow().clone();
            assert_eq!(calls.len(), 4);
            let next = calls.last().unwrap().to_string();
            assert!(next.contains("Retained goals and accepted decisions"));
            assert!(next.contains("Keep this full suffix"));
            assert!(next.contains("New full suffix after restart"));
            assert!(!next.contains("Original user goal"));
            drop(calls);
            let storage = swarmie_storage::sqlite::SqliteConversationStorage::open(
                fixture.profile.path().join("conversations.sqlite"),
            )
            .unwrap();
            let history = swarmie_conversations::Conversations::new(Rc::new(storage));
            let history_id = uuid::Uuid::new_v5(
                &uuid::Uuid::NAMESPACE_URL,
                b"context-fixture/fixture-user/context-session",
            );
            let transcript: String = fixture
                .host
                .root("fixture-user", move |context| {
                    async move {
                        let id = history_id.to_string().parse().unwrap();
                        let at = history.get(&context, id).await?.unwrap().head;
                        Ok(serde_json::to_string(
                            &history
                                .read(&context, swarmie_conversations::Read::new(at))
                                .await?,
                        )
                        .unwrap())
                    }
                    .boxed_local()
                })
                .await
                .unwrap();
            assert!(transcript.contains("Original user goal"));
            assert!(transcript.contains("Original answer"));
            fixture.close().await;
        })
        .await;
}

#[tokio::test]
async fn stop_fences_a_dispatched_summary_without_replaying_it() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let fixture = Fixture::new(128_000, 4_000, true, "Original answer".into()).await;
            fixture.turn("Original user goal").await;
            fixture.turn("Keep the suffix").await;
            let request = CompactionRequest {
                operation: OperationId::new(),
                fence: fixture.inspect().await.fence.unwrap(),
            };
            let _: Receipt = fixture
                .call(swarmie_context::SERVICE, "compact", (request,))
                .await
                .unwrap();
            tokio::time::timeout(Duration::from_secs(3), fixture.summary_entered.notified())
                .await
                .unwrap();
            let stopped: Receipt = fixture
                .call(swarmie_context::SERVICE, "stop", (request.operation,))
                .await
                .unwrap();
            assert_ne!(stopped.snapshot.fence.unwrap(), request.fence);
            fixture.summary_release.notify_one();
            let snapshot = fixture.settled().await;
            assert_ne!(
                snapshot.checkpoint.compaction().unwrap().phase,
                Phase::Activated
            );
            let count = fixture.calls.borrow().len();
            let duplicate: Receipt = fixture
                .call(swarmie_context::SERVICE, "compact", (request,))
                .await
                .unwrap();
            assert_eq!(duplicate.admission, Admission::Recognized);
            assert_eq!(fixture.calls.borrow().len(), count);
            assert!(
                fixture
                    .call::<_, Receipt>(
                        swarmie_context::SERVICE,
                        "compact",
                        (CompactionRequest {
                            operation: OperationId::new(),
                            ..request
                        },)
                    )
                    .await
                    .is_err()
            );
            fixture.close().await;
        })
        .await;
}

#[tokio::test]
async fn automatic_compaction_resumes_the_same_turn_with_the_complete_latest_user_message() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let fixture = Fixture::new(2_200, 100, false, "Original answer".into()).await;
            fixture.turn(&"Original source. ".repeat(300)).await;
            let suffix = "Latest user input. ".repeat(320);
            fixture.turn(&suffix).await;
            let snapshot = fixture.inspect().await;
            assert_eq!(
                snapshot.checkpoint.compaction().map(|work| &work.phase),
                Some(&Phase::Activated),
                "{snapshot:?}"
            );
            let calls = fixture.calls.borrow().clone();
            assert_eq!(calls.len(), 3, "{calls:?}");
            assert!(
                calls[1]["messages"][0]["content"]
                    .as_str()
                    .unwrap()
                    .starts_with("Create a concise")
            );
            let messages = calls[2]["messages"].as_array().unwrap();
            assert!(messages.iter().any(|message| message["content"] == suffix));
            assert!(messages.iter().any(|message| {
                message["content"]
                    .as_str()
                    .is_some_and(|text| text.contains("Retained goals and accepted decisions"))
            }));
            drop(calls);
            let index: Value = serde_json::from_slice(
                &std::fs::read(fixture.profile.path().join("conversations.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(index[0]["phase"], "completed", "{index}");
            fixture.close().await;
        })
        .await;
}

#[tokio::test]
async fn a_detached_compact_command_and_steer_share_one_summary_and_keep_the_suffix() {
    tokio::task::LocalSet::new().run_until(async {
        let fixture = Fixture::new(128_000, 4_000, true, "Original answer".into()).await;
        fixture.turn("Original user goal").await;
        fixture.turn("Keep this full suffix").await;
        let request = CompactionRequest { operation: OperationId::new(), fence: fixture.inspect().await.fence.unwrap() };
        let _: Receipt = fixture.call(swarmie_context::SERVICE, "compact", (request,)).await.unwrap();
        tokio::time::timeout(Duration::from_secs(3), fixture.summary_entered.notified()).await.unwrap();
        let joined: Receipt = fixture.call(swarmie_context::SERVICE, "compact", (CompactionRequest { operation: OperationId::new(), ..request },)).await.unwrap();
        assert_eq!(joined.admission, Admission::Joined);
        assert_eq!(joined.operation, request.operation);
        let host = fixture.host.clone();
        let steer = tokio::task::spawn_local(async move {
            call::<_, Value>(&host, "swarmie.conversation.session", "run", (json!({
                "conversationId":"context-session","submissionId":uuid::Uuid::new_v4().to_string(),"createConversation":false,
                "draft":{"text":"Steer added during compaction","attachments":[],"account":"fixture-account","model":"fixture-model","effort":"","node":"context-fixture"},"mode":"steer"
            }),)).await
        });
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(!steer.is_finished());
        fixture.summary_release.notify_one();
        steer.await.unwrap().unwrap();
        let snapshot = fixture.inspect().await;
        assert_eq!(snapshot.checkpoint.compaction().unwrap().phase, Phase::Activated);
        let calls = fixture.calls.borrow().clone();
        assert_eq!(calls.len(), 4);
        let next = calls[3].to_string();
        assert!(next.contains("Keep this full suffix"));
        assert!(next.contains("Steer added during compaction"));
        assert!(next.contains("Retained goals and accepted decisions"));
        drop(calls);
        assert_eq!(snapshot.fence, Some(request.fence));
        fixture.call::<_, Value>("swarmie.conversation.session", "run", (json!({
            "conversationId":"context-session","submissionId":uuid::Uuid::new_v4().to_string(),"createConversation":false,
            "draft":{"text":"A later explicit steer","attachments":[],"account":"fixture-account","model":"fixture-model","effort":"","node":"context-fixture"},"mode":"steer"
        }),)).await.unwrap();
        assert_eq!(fixture.inspect().await.fence, Some(request.fence), "Steer must preserve the conversation admission fence");
        assert_eq!(fixture.calls.borrow().len(), 5);
        fixture.close().await;
    }).await;
}

#[tokio::test]
async fn an_unusable_confirmed_summary_is_retained_without_activation_or_regeneration() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let fixture = Fixture::new(128_000, 4_000, false, "Original answer".into()).await;
            fixture.turn("Original user goal").await;
            fixture.turn("Full suffix").await;
            *fixture.summary_answer.borrow_mut() = "   ".into();
            let request = CompactionRequest {
                operation: OperationId::new(),
                fence: fixture.inspect().await.fence.unwrap(),
            };
            let _: Receipt = fixture
                .call(swarmie_context::SERVICE, "compact", (request,))
                .await
                .unwrap();
            let snapshot = fixture.settled().await;
            assert!(matches!(
                snapshot.checkpoint.compaction().unwrap().phase,
                Phase::Rejected { .. }
            ));
            assert!(snapshot.checkpoint.active().is_none());
            let duplicate: Receipt = fixture
                .call(swarmie_context::SERVICE, "compact", (request,))
                .await
                .unwrap();
            assert_eq!(duplicate.admission, Admission::Recognized);
            assert_eq!(fixture.calls.borrow().len(), 3);
            fixture.close().await;
        })
        .await;
}

#[tokio::test]
async fn restart_recognizes_an_association_without_a_created_task_without_dispatch() {
    tokio::task::LocalSet::new().run_until(async {
        let mut fixture = Fixture::new(128_000, 4_000, false, "Original answer".into()).await;
        fixture.turn("Original goal").await;
        fixture.turn("Full suffix").await;
        let snapshot = fixture.inspect().await;
        let request = CompactionRequest {operation: OperationId::new(), fence: snapshot.fence.unwrap()};
        let head = snapshot.inspection.unwrap().at;
        let mut checkpoint = swarmie_context::Checkpoint::default();
        checkpoint.request(swarmie_context::Request {
            operation: request.operation,
            required: false,
            plan: swarmie_context::Plan {
                through: swarmie_conversations::HistoryPoint {conversation:head.conversation,entry:head.entry.unwrap()},
                previous: None,
                configuration: swarmie_context::Configuration([1;32]),
                strategy: swarmie_context::Strategy::Textual {model:swarmie_context::Configuration([1;32])},
            },
        }).unwrap();
        fixture.host.close().await.unwrap();
        // This fixture represents process loss between the metadata association
        // and task creation. No task or physical request existed for this ID.
        let path = fixture.profile.path().join("conversations.json");
        let mut metadata: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        metadata[0]["context"] = json!({"checkpoint":checkpoint,"fence":request.fence,"operation_fence":request.fence,"task":swarmie_durable_tasks::TaskId::new(),"automatic":false});
        std::fs::write(path, serde_json::to_vec(&metadata).unwrap()).unwrap();
        fixture.host = Fixture::open(fixture.profile.path()).await;
        let receipt: Receipt = fixture.call(swarmie_context::SERVICE, "compact", (request,)).await.unwrap();
        assert!(matches!(receipt.admission, Admission::Refused {..}));
        assert!(matches!(receipt.snapshot.checkpoint.compaction().unwrap().phase, Phase::Failed {..}));
        let again: Receipt = fixture.call(swarmie_context::SERVICE, "compact", (request,)).await.unwrap();
        assert_eq!(again.admission, Admission::Recognized);
        assert_eq!(fixture.calls.borrow().len(), 2);
        fixture.close().await;
    }).await;
}
