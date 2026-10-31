#![cfg(unix)]

use futures::task::{LocalFutureObj, LocalSpawn, SpawnError};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use swarmie_git::{
    Authority, Cancellation, CommandOutput, CommandSpec, Git, GitService, NativeProcess,
    ProcessExecutor, Request, Response,
};
use swarmie_node::composition::ServicesConfiguration;
use swarmie_runtime::NodeId;
use tembra::Host;

fn install_fixture(host: &Host, configuration: ServicesConfiguration) -> tembra::Result<()> {
    let identity = swarmie_runtime::node_orb(configuration.node.clone());
    let mut definitions = swarmie_node::composition::orbs(host, configuration, None)?;
    definitions.push(identity);
    host.install(definitions)
}

#[derive(Clone, Copy)]
struct Executor;
impl LocalSpawn for Executor {
    fn spawn_local_obj(&self, future: LocalFutureObj<'static, ()>) -> Result<(), SpawnError> {
        tokio::task::spawn_local(future);
        Ok(())
    }
}
struct Granted;
impl Authority for Granted {
    fn authorize_discovery(&self, _: &str, _: &Path) -> swarmie_git::Result<()> {
        Ok(())
    }
    fn authorize(&self, _: &str, _: &swarmie_git::Intent) -> swarmie_git::Result<()> {
        Ok(())
    }
}
#[derive(Default)]
struct Gate {
    armed: AtomicBool,
    entered: AtomicBool,
    released: AtomicBool,
    churn: std::sync::Mutex<Option<PathBuf>>,
    status_calls: std::sync::atomic::AtomicUsize,
}
impl ProcessExecutor for Gate {
    fn run(
        &self,
        command: &CommandSpec,
        cancellation: &Cancellation,
    ) -> swarmie_git::Result<CommandOutput> {
        if self.armed.swap(false, Ordering::SeqCst) {
            self.entered.store(true, Ordering::SeqCst);
            let deadline = Instant::now() + Duration::from_secs(10);
            while !self.released.load(Ordering::SeqCst) {
                if Instant::now() >= deadline {
                    return Err(swarmie_git::Error::NotStarted(
                        "fixture gate timed out".into(),
                    ));
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        if command.args.iter().any(|arg| arg == "status")
            && let Some(path) = self.churn.lock().unwrap().as_ref()
        {
            let count = self.status_calls.fetch_add(1, Ordering::SeqCst);
            std::fs::write(
                path.join(format!("churn-{count}.txt")),
                "arrived during validation\n",
            )?;
        }
        NativeProcess.run(command, cancellation)
    }
}

fn initialize(path: &Path) {
    assert!(
        std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(path)
            .status()
            .unwrap()
            .success()
    );
}
fn binding(git: &GitService, root: &Path) -> Value {
    let Response::Repository {
        repository: Some(target),
    } = git
        .git()
        .execute(
            "fixture-user",
            Request::Discover {
                path: root.to_string_lossy().into(),
            },
            &Cancellation::default(),
        )
        .unwrap()
    else {
        panic!("fixture checkout is unavailable");
    };
    json!({"version":1,"nodeId":"fixture-node","principal":"fixture-user","path":target.root,"owner":"git","identity":serde_json::to_string(&(&target.root,&target.git_dir,&target.common_dir)).unwrap()})
}
fn submission(binding: &Value, mode: &str, id: &str) -> Value {
    json!({"conversationId":"concurrent-first","submissionId":id,"createConversation":true,"draft":{"text":"Inspect the captured checkout","attachments":[],"account":"fixture-account","model":"fixture-model","effort":"","node":"fixture-node"},"mode":mode,"executionContext":binding})
}

async fn response(output: &Rc<std::cell::RefCell<Vec<Value>>>, id: u64) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let index = output.borrow().iter().position(|value| value["id"] == id);
        if let Some(index) = index {
            return output.borrow_mut().remove(index);
        }
        assert!(
            Instant::now() < deadline,
            "native fixture response timed out"
        );
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

async fn concurrent_first(mode: &str, churn: bool, sources: bool) {
    let profile = tempfile::tempdir().unwrap();
    let first_root = tempfile::tempdir().unwrap();
    let second_root = tempfile::tempdir().unwrap();
    initialize(first_root.path());
    initialize(second_root.path());
    let selected = if sources {
        let skill = first_root.path().join(".agents/skills/fixture");
        std::fs::create_dir_all(skill.join("references")).unwrap();
        std::fs::write(
            first_root.path().join("AGENTS.md"),
            "Use the project instruction marker.",
        )
        .unwrap();
        std::fs::write(skill.join("SKILL.md"), "---\nname: fixture\ndescription: Read the fixture resource\n---\nRead references/check.md for the resource marker.").unwrap();
        std::fs::write(
            skill.join("references/check.md"),
            "native skill resource marker",
        )
        .unwrap();
        Some(
            swarmie_context::environment::Environment::new(
                swarmie_context::environment::Scope::new(first_root.path(), first_root.path())
                    .unwrap(),
                Default::default(),
            )
            .unwrap()
            .inspect()
            .unwrap()
            .select("fixture")
            .unwrap(),
        )
    } else {
        None
    };
    let gate = Arc::new(Gate::default());
    let git = GitService::new(
        Git::new(
            gate.clone(),
            Arc::new(
                swarmie_git::DirectoryJournal::open(profile.path().join("operations")).unwrap(),
            ),
            Arc::new(Granted),
        ),
        profile.path().join("git"),
    )
    .unwrap();
    let first_binding = binding(&git, first_root.path());
    let second_binding = binding(&git, second_root.path());
    if churn {
        *gate.churn.lock().unwrap() = Some(first_root.path().into());
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        for turn in 0..if sources { 5 } else { 1 } {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "fixture model was not called");
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => panic!("{error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut buffer = [0; 4096];
                let count = stream.read(&mut buffer).unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&buffer[..count]);
                if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&bytes[..end]);
                    let length: usize = header
                        .lines()
                        .filter_map(|line| line.split_once(':'))
                        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                        .unwrap()
                        .1
                        .trim()
                        .parse()
                        .unwrap();
                    if bytes.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let mut streaming = true;
            if sources {
                let end = bytes
                    .windows(4)
                    .position(|part| part == b"\r\n\r\n")
                    .unwrap()
                    + 4;
                let request: Value = serde_json::from_slice(&bytes[end..]).unwrap();
                streaming = request["stream"] == true;
                let encoded = request.to_string();
                if turn == 3 {
                    assert!(!streaming, "summary must use complete responses");
                    assert!(
                        !encoded.contains("project instruction marker"),
                        "current instructions entered the historical summary: {request}"
                    );
                } else {
                    assert!(encoded.contains("project instruction marker"), "{request}");
                    assert!(encoded.contains("skill_read"), "{request}");
                    assert!(encoded.contains("Read references/check.md"), "{request}");
                    if turn == 1 {
                        assert!(
                            encoded.contains("native skill resource marker"),
                            "{request}"
                        );
                    }
                }
            }
            let message = if sources && turn == 0 {
                let selected = selected.as_ref().unwrap();
                json!({"role":"assistant","content":"","tool_calls":[{"id":"skill-read-call","function":{"name":"skill_read","arguments":{"id":selected.id,"revision":selected.revision,"path":"references/check.md"}}}]})
            } else {
                json!({"role":"assistant","content":"First checkout retained"})
            };
            let body = format!(
                "{}\n",
                json!({"model":"fixture-model","message":message,"done":true,"done_reason":"stop","prompt_eval_count":3,"eval_count":4})
            );
            let content_type = if streaming {
                "application/x-ndjson"
            } else {
                "application/json"
            };
            write!(stream,"HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
    });
    let metadata_claim = |tokens| json!({"tokens":tokens,"precision":"reported","evidence":{"source":"fixture-configuration","revision":"1","layer":"configuration","observed_at_ms":0,"expires_at_ms":null}});
    std::fs::write(profile.path().join("providers.json"), serde_json::to_vec(&json!([{"account":{"id":"fixture-account","providerId":"ollama","providerLabel":"Local","label":"Fixture","models":[{"id":"fixture-model","label":"Fixture","efforts":[]}],"available":true},"provider":"ollama","endpoint":endpoint,"oauth":false,"oauth_identity":null,"discovered_limits":{"endpoint":endpoint,"models":{"fixture-model":{"context":[metadata_claim(128000)],"input":[],"output":[metadata_claim(8192)]}},"capabilities":{"fixture-model":{"tools":true,"streaming":true,"profile":"swarmie.text-tools.v1","evidence":{"source":"fixture-configuration","revision":"1","layer":"configuration","observed_at_ms":0,"expires_at_ms":null}}}}}])).unwrap()).unwrap();
    let host = Host::new(Executor);
    install_fixture(
        &host,
        ServicesConfiguration {
            environment: sources.then(Default::default),
            state: PathBuf::from(profile.path()),
            node: NodeId::new("fixture-node").unwrap(),
            oauth_host: uuid::Uuid::new_v4(),
            principal: "fixture-user".into(),
            directory_root: None,
            git: Some(git),
            agent_git_authority: None,
        },
    )
    .unwrap();
    swarmie_conversation_runtime::start(&host).await.unwrap();
    let output = Rc::new(std::cell::RefCell::new(Vec::<Value>::new()));
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
    let send = |id, op: &str, payload: Value| {
        connection
            .receive(&serde_json::to_vec(&json!({"id":id,"op":op,"payload":payload})).unwrap())
            .unwrap()
    };
    send(
        1,
        "get",
        json!({"service":"swarmie.conversation.session","key":["concurrent-first"]}),
    );
    let remote = response(&output, 1).await;
    let reference = remote["ok"]["reference"].clone();
    assert!(reference.is_number(), "{remote}");
    if sources {
        send(
            20,
            "call",
            json!({"reference":reference,"method":"project_skills","arguments":[first_binding]}),
        );
        let catalog = response(&output, 20).await;
        assert_eq!(catalog["ok"]["skills"][0]["name"], "fixture", "{catalog}");
        send(
            21,
            "call",
            json!({"reference":reference,"method":"execute","arguments":[uuid::Uuid::new_v4(),{"id":"outside","name":"skill_read","arguments":{"id":"invented","revision":"invented","path":"SKILL.md"}}]}),
        );
        let denied = response(&output, 21).await;
        assert!(denied.to_string().contains("blocked"), "{denied}");
    }
    gate.armed.store(true, Ordering::SeqCst);
    send(2, "call", {
        let mut input = submission(&first_binding, "queue", "first");
        if sources {
            input["draft"]["text"] = json!("/skill:fixture Inspect the captured checkout");
        }
        json!({"reference":reference,"method":"run","arguments":[input]})
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while !gate.entered.load(Ordering::SeqCst) {
        assert!(
            Instant::now() < deadline,
            "first native validation did not enter its gate; responses: {:?}",
            output.borrow()
        );
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    assert!(
        !profile.path().join("conversations.json").exists(),
        "the competing first request must arrive before durable binding"
    );
    send(
        3,
        "call",
        json!({"reference":reference,"method":"run","arguments":[submission(&second_binding,mode,"second")]}),
    );
    let early = tokio::time::timeout(Duration::from_millis(25), response(&output, 3)).await;
    gate.released.store(true, Ordering::SeqCst);
    let first = response(&output, 2).await;
    let second = match early {
        Ok(result) => result,
        Err(_) => response(&output, 3).await,
    };
    assert_eq!(
        first["ok"]["status"], "settled",
        "a replacement checkout must not cancel the first preparation: {first}"
    );
    assert_eq!(
        second["ok"]["status"], "refused",
        "a waiting first submission cannot replace the established checkout: {second}"
    );
    let index: Value =
        serde_json::from_slice(&std::fs::read(profile.path().join("conversations.json")).unwrap())
            .unwrap();
    assert_eq!(index[0]["executionContext"], first_binding);
    assert_eq!(index[0]["phase"], "completed");
    assert_eq!(index[0]["receipts"].as_array().unwrap().len(), 1);
    assert_eq!(index[0]["receipts"][0]["id"], "first");
    if churn {
        assert_eq!(
            gate.status_calls.load(Ordering::SeqCst),
            0,
            "identity admission and active retention must not observe content"
        );
    }
    if sources {
        for (id, receipt) in [(30, "second-source-turn"), (40, "after-source-compaction")] {
            if id == 40 {
                send(
                    32,
                    "call",
                    json!({"reference":reference,"method":"context_inspect","arguments":[]}),
                );
                let inspection = response(&output, 32).await;
                assert_eq!(inspection["ok"]["can_compact"], true, "{inspection}");
                send(
                    33,
                    "call",
                    json!({"reference":reference,"method":"context_compact","arguments":[{"operation":uuid::Uuid::new_v4(),"fence":inspection["ok"]["fence"]}]}),
                );
                let receipt = response(&output, 33).await;
                assert_eq!(receipt["ok"]["admission"]["kind"], "admitted", "{receipt}");
                let deadline = Instant::now() + Duration::from_secs(5);
                loop {
                    send(
                        34,
                        "call",
                        json!({"reference":reference,"method":"context_inspect","arguments":[]}),
                    );
                    let inspected = response(&output, 34).await;
                    if inspected["ok"]["checkpoint"]["compaction"]["phase"]["kind"] == "activated" {
                        break;
                    }
                    assert!(
                        Instant::now() < deadline,
                        "source compaction did not activate: {inspected}"
                    );
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            }
            let mut input = submission(&first_binding, "queue", receipt);
            input["draft"]["text"] = json!("/skill:fixture Continue with the captured sources");
            send(
                id,
                "call",
                json!({"reference":reference,"method":"run","arguments":[input]}),
            );
            let settled = response(&output, id).await;
            assert_eq!(settled["ok"]["status"], "settled", "{settled}");
        }
    }
    server.join().unwrap();
    connection.close();
    host.close().await.unwrap();
}

#[tokio::test]
#[cfg(unix)]
async fn simultaneous_first_submissions_cannot_replace_a_checkout_or_steer_its_preparation() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for mode in ["queue", "steer"] {
                concurrent_first(mode, false, false).await;
            }
        })
        .await;
}

#[tokio::test]
async fn content_churn_cannot_refuse_identity_only_conversation_admission() {
    tokio::task::LocalSet::new()
        .run_until(concurrent_first("queue", true, false))
        .await;
}

#[tokio::test]
async fn native_project_sources_and_skill_resources_use_the_owning_tembra_session() {
    tokio::task::LocalSet::new()
        .run_until(concurrent_first("queue", false, true))
        .await;
}
