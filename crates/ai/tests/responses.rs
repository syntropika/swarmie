use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use swarmie_ai::{AuthProfile, Effect, Error, Failure, Model, Timeouts, WireMode};
use swarmie_model_catalog::{
    Capability, Catalog, Evidence, Layer, LimitClaim, Limits, ModelIdentity, Precision,
    Preparation, Requirements, Snapshot, SourceRecord,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
};
use zeroize::Zeroizing;

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

struct SelectedSource {
    identity: ModelIdentity,
    key: std::cell::RefCell<String>,
    calls: std::rc::Rc<std::cell::Cell<usize>>,
    profile: AuthProfile,
    fail: bool,
}

#[tokio::test]
async fn deferred_credentials_preserve_admitted_bytes_and_refuse_missing_or_foreign_custody() {
    let (listener, endpoint) = listener().await;
    let snapshot = snapshot(&endpoint);
    let model = Model::deferred(
        snapshot.clone(),
        swarmie_ai::Protocol::Responses,
        AuthProfile::ApiKey,
        Timeouts::default(),
    )
    .unwrap();
    let prepare = || {
        model
            .prepare(uuid::Uuid::new_v4(), payload(), WireMode::Complete)
            .unwrap()
            .admit(|_, _, _, _| Ok(()))
            .unwrap()
    };
    let error = prepare()
        .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
        .err()
        .unwrap();
    assert_eq!(error.effect, Effect::NotDispatched);
    let expired = swarmie_ai::Credentials::new(
        snapshot.identity().clone(),
        AuthProfile::ApiKey,
        Some(Zeroizing::new("expired".into())),
        Some(0),
    )
    .unwrap();
    assert_eq!(
        prepare().authenticate(expired).err().unwrap().failure,
        Failure::Expired
    );
    let mut foreign = snapshot.identity().clone();
    foreign.model = "another-model".into();
    let credentials = swarmie_ai::Credentials::new(
        foreign,
        AuthProfile::ApiKey,
        Some(Zeroizing::new("foreign".into())),
        None,
    )
    .unwrap();
    assert_eq!(
        prepare().authenticate(credentials).err().unwrap().effect,
        Effect::NotDispatched
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(10), listener.accept())
            .await
            .is_err()
    );
    let mut inspected = Vec::new();
    let prepared = model
        .prepare(uuid::Uuid::new_v4(), payload(), WireMode::Complete)
        .unwrap();
    let fingerprint = prepared.description().clone();
    let admitted = prepared
        .admit(|description, bytes, _, _| {
            assert_eq!(description, &fingerprint);
            inspected = bytes.to_vec();
            Ok(())
        })
        .unwrap();
    let admitted = admitted
        .authenticate(
            swarmie_ai::Credentials::new(
                snapshot.identity().clone(),
                AuthProfile::ApiKey,
                Some(Zeroizing::new("admitted-key".into())),
                Some(now() + 60_000),
            )
            .unwrap(),
        )
        .unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let bytes = read_request(&mut socket).await;
        reply(
            &mut socket,
            "200 OK",
            "application/json",
            &response().to_string(),
        )
        .await;
        bytes
    });
    admitted
        .dispatch(&tokio::runtime::Handle::current(), move |description| {
            assert_eq!(description, &fingerprint);
            Ok(())
        })
        .unwrap()
        .wait()
        .await
        .unwrap();
    let bytes = server.await.unwrap();
    let end = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .unwrap();
    assert!(String::from_utf8_lossy(&bytes[..end]).contains("Bearer admitted-key"));
    assert_eq!(&bytes[end + 4..], &inspected);
}

#[tokio::test]
async fn local_authority_revocation_and_stop_before_send_preserve_not_dispatched() {
    use futures::task::{LocalFutureObj, LocalSpawn, SpawnError};
    struct Executor;
    impl LocalSpawn for Executor {
        fn spawn_local_obj(&self, future: LocalFutureObj<'static, ()>) -> Result<(), SpawnError> {
            tokio::task::spawn_local(future);
            Ok(())
        }
    }
    tokio::task::LocalSet::new()
        .run_until(async {
            let (listener, endpoint) = listener().await;
            let snapshot = snapshot(&endpoint);
            let model = Model::deferred(
                snapshot.clone(),
                swarmie_ai::Protocol::Responses,
                AuthProfile::ApiKey,
                Timeouts::default(),
            )
            .unwrap();
            let prepare = || {
                model
                    .prepare(uuid::Uuid::new_v4(), payload(), WireMode::Complete)
                    .unwrap()
                    .admit(|_, _, _, _| Ok(()))
                    .unwrap()
                    .authenticate(
                        swarmie_ai::Credentials::new(
                            snapshot.identity().clone(),
                            AuthProfile::ApiKey,
                            Some(Zeroizing::new("key".into())),
                            None,
                        )
                        .unwrap(),
                    )
                    .unwrap()
            };
            let live = std::rc::Rc::new(std::cell::Cell::new(true));
            let captured = live.clone();
            let execution = prepare()
                .dispatch_local(&Executor, move |_| {
                    if captured.get() {
                        Ok(())
                    } else {
                        Err(Error {
                            failure: Failure::Admission,
                            effect: Effect::Unknown,
                        })
                    }
                })
                .unwrap();
            live.set(false);
            assert_eq!(
                execution.wait().await.err().unwrap().effect,
                Effect::NotDispatched
            );
            let execution = prepare().dispatch_local(&Executor, |_| Ok(())).unwrap();
            execution.stop_handle().cancel();
            assert_eq!(
                execution.wait().await.err().unwrap().effect,
                Effect::NotDispatched
            );
            assert!(
                tokio::time::timeout(Duration::from_millis(10), listener.accept())
                    .await
                    .is_err()
            );
        })
        .await;
}

impl swarmie_ai::CredentialSource for SelectedSource {
    fn resolve<'a>(&'a self, _identity: &'a ModelIdentity) -> swarmie_ai::CredentialFuture<'a> {
        Box::pin(async move {
            self.calls.set(self.calls.get() + 1);
            if self.fail {
                return Err(Error {
                    failure: Failure::Credential,
                    effect: Effect::NotDispatched,
                });
            }
            swarmie_ai::Credentials::new(
                self.identity.clone(),
                self.profile,
                Some(Zeroizing::new(self.key.borrow().clone())),
                Some(now() + 60000),
            )
        })
    }
}

#[tokio::test]
async fn local_credential_resolution_precedes_frozen_preparation_and_never_runs_at_dispatch() {
    let (listener, endpoint) = listener().await;
    let snapshot = snapshot(&endpoint);
    let calls = std::rc::Rc::new(std::cell::Cell::new(0));
    let source = SelectedSource {
        identity: snapshot.identity().clone(),
        key: std::cell::RefCell::new("captured-key".into()),
        calls: calls.clone(),
        profile: AuthProfile::ApiKey,
        fail: false,
    };
    let model = Model::from_source(
        snapshot,
        swarmie_ai::Protocol::Responses,
        &source,
        Timeouts::default(),
    )
    .await
    .unwrap();
    *source.key.borrow_mut() = "later-key".into();
    let prepared = model
        .prepare(uuid::Uuid::new_v4(), payload(), WireMode::Complete)
        .unwrap();
    let mut inspected = Vec::new();
    let admitted = prepared
        .admit(|_, bytes, _, _| {
            inspected = bytes.to_vec();
            Ok(())
        })
        .unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let bytes = read_request(&mut socket).await;
        reply(
            &mut socket,
            "200 OK",
            "application/json",
            &response().to_string(),
        )
        .await;
        bytes
    });
    admitted
        .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
        .unwrap()
        .wait()
        .await
        .unwrap();
    let request = server.await.unwrap();
    let end = request
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .unwrap();
    let headers = String::from_utf8_lossy(&request[..end]);
    assert!(headers.contains("Bearer captured-key"));
    assert!(!headers.contains("later-key"));
    assert_eq!(request[end + 4..], inspected);
    assert_eq!(calls.get(), 1);
}

#[tokio::test]
async fn credential_failure_substitution_and_unknown_metadata_cannot_fall_back() {
    let snapshot = snapshot("https://example.test/responses");
    let calls = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut source = SelectedSource {
        identity: snapshot.identity().clone(),
        key: std::cell::RefCell::new("selected-key".into()),
        calls: calls.clone(),
        profile: AuthProfile::ApiKey,
        fail: false,
    };
    let error = Model::from_source(
        snapshot.clone(),
        swarmie_ai::Protocol::Anthropic,
        &source,
        Timeouts::default(),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(error.failure, Failure::Metadata);
    assert_eq!(calls.get(), 0);
    source.identity.model = "other-model".into();
    let error = Model::from_source(
        snapshot.clone(),
        swarmie_ai::Protocol::Responses,
        &source,
        Timeouts::default(),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(error.failure, Failure::Credential);
    source.fail = true;
    let error = Model::from_source(
        snapshot.clone(),
        swarmie_ai::Protocol::Responses,
        &source,
        Timeouts::default(),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(error.effect, Effect::NotDispatched);
    assert_eq!(error.failure, Failure::Credential);
    source.fail = false;
    source.identity = snapshot.identity().clone();
    source.profile = AuthProfile::ChatGpt;
    let error = Model::from_source(
        snapshot,
        swarmie_ai::Protocol::Responses,
        &source,
        Timeouts::default(),
    )
    .await
    .err()
    .unwrap();
    assert_eq!(error.failure, Failure::UnsupportedProfile);
    assert_eq!(calls.get(), 3);
}

fn snapshot(endpoint: &str) -> Arc<Snapshot> {
    snapshot_for(endpoint, swarmie_ai::Protocol::Responses.adapter(), true)
}

fn snapshot_for(endpoint: &str, adapter: &str, capabilities: bool) -> Arc<Snapshot> {
    let identity = ModelIdentity {
        provider: "fixture".into(),
        adapter: adapter.into(),
        endpoint: endpoint.into(),
        deployment: None,
        model: "test-model".into(),
        revision: None,
    };
    let evidence = Evidence {
        source: "synthetic-protocol-test".into(),
        revision: "1".into(),
        layer: Layer::Adapter,
        observed_at_ms: now(),
        expires_at_ms: None,
    };
    let claim = |tokens: u64| LimitClaim {
        tokens: tokens.try_into().unwrap(),
        precision: Precision::Reported,
        evidence: Evidence {
            layer: Layer::Provider,
            ..evidence.clone()
        },
    };
    let mut catalog = Catalog::new(1);
    catalog
        .replace_sources(vec![SourceRecord {
            identity: identity.clone(),
            limits: Limits {
                context: vec![claim(10000)],
                input: vec![claim(9000)],
                output: vec![claim(1000)],
            },
            tools: if capabilities {
                Capability::Supported {
                    evidence: evidence.clone(),
                    value: (),
                }
            } else {
                Capability::default()
            },
            streaming: if capabilities {
                Capability::Supported {
                    evidence,
                    value: (),
                }
            } else {
                Capability::default()
            },
            native_compaction: Capability::default(),
            native_output_maximum: Capability::default(),
        }])
        .unwrap();
    catalog
        .prepare(
            Preparation {
                identity,
                configuration_revision: "fixture-1".into(),
                requirements: Requirements::default(),
                allow_stale_for_ms: 0,
            },
            now(),
        )
        .unwrap()
}

fn model(endpoint: &str, timeouts: Timeouts) -> Model {
    Model::new(
        snapshot(endpoint),
        swarmie_ai::Protocol::Responses,
        AuthProfile::ApiKey,
        Some(Zeroizing::new("fixture-secret".into())),
        None,
        timeouts,
    )
    .unwrap()
}

fn payload() -> Value {
    json!({"model":"test-model", "input":[{"role":"developer","content":"Keep the full input."},
        {"role":"user","content":"hello"}], "max_output_tokens":100})
}

fn response() -> Value {
    json!({"id":"response-1", "model":"test-model", "status":"completed", "output":[
        {"type":"reasoning", "id":"reasoning-1", "encrypted_content":"opaque-value", "vendor_extension":{"keep":true}},
        {"type":"function_call", "id":"item-1", "call_id":"call-1", "name":"tool", "arguments":"{ \"key\": 1 }"},
        {"type":"future_output", "unknown_data":[1,2,3]}],
        "usage":{"input_tokens":12,"output_tokens":8,"input_tokens_details":{"cached_tokens":2}},
        "vendor_field":{"preserved":true}})
}

async fn listener() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1/responses", listener.local_addr().unwrap());
    (listener, endpoint)
}

async fn read_request(socket: &mut TcpStream) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 1024];
    loop {
        let count = socket.read(&mut buffer).await.unwrap();
        assert!(count > 0, "request ended before its body");
        bytes.extend_from_slice(&buffer[..count]);
        if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&bytes[..end]);
            let length: usize = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse().unwrap())
                })
                .unwrap();
            if bytes.len() >= end + 4 + length {
                return bytes;
            }
        }
    }
}

async fn reply(socket: &mut TcpStream, status: &str, content_type: &str, body: &str) {
    let wire = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nX-Request-Id: fixture-transport-id\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    socket.write_all(wire.as_bytes()).await.unwrap();
}

fn admit(prepared: swarmie_ai::Prepared) -> swarmie_ai::Admitted {
    prepared
        .admit(|description, bytes, _, snapshot| {
            assert_eq!(description.body_bytes, bytes.len());
            assert_eq!(description.generation, snapshot.generation());
            Ok(())
        })
        .unwrap()
}

#[tokio::test]
async fn inspected_bytes_destination_and_opaque_fields_are_exact() {
    let (listener, endpoint) = listener().await;
    let fixture_response = response();
    let expected_response = fixture_response.clone();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let request = read_request(&mut socket).await;
        reply(
            &mut socket,
            "200 OK",
            "application/json",
            &fixture_response.to_string(),
        )
        .await;
        request
    });
    let model = model(&endpoint, Timeouts::default());
    let mut request = payload();
    request["vendor_request"] = json!({"kept":"exact"});
    let prepared = model
        .prepare(uuid::Uuid::new_v4(), request, WireMode::Complete)
        .unwrap();
    let description = prepared.description().clone();
    assert!(!format!("{description:?}").contains("fixture-secret"));
    assert!(!format!("{description:?}").contains("Keep the full input"));
    let mut inspected = Vec::new();
    let mut admitted_headers = reqwest::header::HeaderMap::new();
    let admitted = prepared
        .admit(|_, bytes, headers, _| {
            admitted_headers = headers.clone();
            inspected = bytes.to_vec();
            Ok(())
        })
        .unwrap();
    let outcome = admitted
        .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
        .unwrap()
        .wait()
        .await
        .unwrap();
    assert_eq!(outcome.response().unwrap(), &expected_response);
    let request = server.await.unwrap();
    let end = request.windows(4).position(|v| v == b"\r\n\r\n").unwrap();
    let headers = String::from_utf8_lossy(&request[..end]);
    assert!(headers.starts_with("POST /v1/responses HTTP/1.1"));
    assert!(headers.contains("Bearer fixture-secret"));
    assert_eq!(&request[end + 4..], inspected.as_slice());
    let sent: Value = serde_json::from_slice(&inspected).unwrap();
    assert_eq!(sent["vendor_request"]["kept"], "exact");
    assert_eq!(sent["stream"], false);
    assert_eq!(sent["truncation"], "disabled");
    let mut hash = Sha256::new();
    for part in [
        b"swarmie.protocol.v2".as_slice(),
        swarmie_ai::Protocol::Responses.adapter().as_bytes(),
        description.operation.as_bytes().as_slice(),
        endpoint.as_bytes(),
        description.generation.as_bytes(),
        &inspected,
    ] {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part);
    }
    let mut headers = admitted_headers.iter().collect::<Vec<_>>();
    headers.sort_by(|(left, _), (right, _)| left.as_str().cmp(right.as_str()));
    for (name, value) in headers {
        let value = if value.is_sensitive() {
            b"[sensitive]".as_slice()
        } else {
            value.as_bytes()
        };
        for part in [name.as_str().as_bytes(), value] {
            hash.update((part.len() as u64).to_be_bytes());
            hash.update(part);
        }
    }
    assert_eq!(description.fingerprint.as_slice(), &hash.finalize()[..]);
}

#[tokio::test]
async fn streaming_mode_and_final_output_do_not_depend_on_observers() {
    let (listener, endpoint) = listener().await;
    let final_response = response();
    let expected = final_response.clone();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let request = read_request(&mut socket).await;
        let body = format!(
            "data: {{\"type\":\"response.output_text.delta\",\"delta\":\"preview\"}}\n\ndata: {}\n\n",
            json!({"type":"response.completed", "response":final_response})
        );
        reply(
            &mut socket,
            "200 OK",
            "text/event-stream; charset=utf-8",
            &body,
        )
        .await;
        request
    });
    let execution = admit(
        model(&endpoint, Timeouts::default())
            .prepare(uuid::Uuid::new_v4(), payload(), WireMode::Stream)
            .unwrap(),
    )
    .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
    .unwrap();
    let observer = execution.previews();
    drop(observer);
    assert_eq!(
        execution.wait().await.unwrap().response().unwrap(),
        &expected
    );
    let request = server.await.unwrap();
    let end = request.windows(4).position(|v| v == b"\r\n\r\n").unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&request[end + 4..]).unwrap()["stream"],
        true
    );
}

#[tokio::test]
async fn eof_without_a_terminal_response_is_unknown() {
    let (listener, endpoint) = listener().await;
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        reply(
            &mut socket,
            "200 OK",
            "text/event-stream",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n",
        )
        .await;
    });
    let error = admit(
        model(&endpoint, Timeouts::default())
            .prepare(uuid::Uuid::new_v4(), payload(), WireMode::Stream)
            .unwrap(),
    )
    .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
    .unwrap()
    .wait()
    .await
    .err()
    .unwrap();
    assert_eq!(
        error,
        Error {
            failure: Failure::Protocol,
            effect: Effect::Unknown
        }
    );
    server.await.unwrap();
}

#[tokio::test]
async fn dropping_execution_detaches_without_aborting_transport() {
    let (listener, endpoint) = listener().await;
    let (accepted_tx, accepted_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        accepted_tx.send(()).unwrap();
        release_rx.await.unwrap();
        reply(
            &mut socket,
            "200 OK",
            "application/json",
            &response().to_string(),
        )
        .await;
        let mut byte = [0];
        tokio::time::timeout(Duration::from_secs(2), socket.read(&mut byte))
            .await
            .unwrap()
            .unwrap()
    });
    let execution = admit(
        model(&endpoint, Timeouts::default())
            .prepare(uuid::Uuid::new_v4(), payload(), WireMode::Complete)
            .unwrap(),
    )
    .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
    .unwrap();
    accepted_rx.await.unwrap();
    let mut observer = execution.previews();
    drop(execution);
    assert!(
        tokio::time::timeout(Duration::from_millis(25), observer.changed())
            .await
            .is_err()
    );
    release_tx.send(()).unwrap();
    assert!(observer.changed().await.is_err());
    assert_eq!(server.await.unwrap(), 0);
}

#[tokio::test]
async fn stop_after_dispatch_settles_locally_without_retry() {
    let (listener, endpoint) = listener().await;
    let (accepted_tx, accepted_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        accepted_tx.send(()).unwrap();
        let mut byte = [0];
        assert_eq!(socket.read(&mut byte).await.unwrap(), 0);
        assert!(
            tokio::time::timeout(Duration::from_millis(40), listener.accept())
                .await
                .is_err()
        );
    });
    let execution = admit(
        model(&endpoint, Timeouts::default())
            .prepare(uuid::Uuid::new_v4(), payload(), WireMode::Stream)
            .unwrap(),
    )
    .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
    .unwrap();
    accepted_rx.await.unwrap();
    execution.stop();
    assert_eq!(
        execution.wait().await.err().unwrap(),
        Error {
            failure: Failure::Stopped,
            effect: Effect::Unknown
        }
    );
    server.await.unwrap();
}

#[tokio::test]
async fn redirects_and_server_failures_never_retry_or_follow() {
    for status in ["307 Temporary Redirect", "503 Service Unavailable"] {
        let (listener, endpoint) = listener().await;
        let status = status.to_owned();
        let expected: u16 = status[..3].parse().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            read_request(&mut socket).await;
            socket.write_all(b"HTTP/1.1 ").await.unwrap();
            let wire = format!(
                "{status}\r\nLocation: /other\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            socket.write_all(wire.as_bytes()).await.unwrap();
            assert!(
                tokio::time::timeout(Duration::from_millis(40), listener.accept())
                    .await
                    .is_err()
            );
        });
        let error = admit(
            model(&endpoint, Timeouts::default())
                .prepare(uuid::Uuid::new_v4(), payload(), WireMode::Complete)
                .unwrap(),
        )
        .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
        .unwrap()
        .wait()
        .await
        .err()
        .unwrap();
        assert_eq!(
            error,
            Error {
                failure: Failure::Http(expected),
                effect: Effect::Unknown
            }
        );
        server.await.unwrap();
    }
}

#[tokio::test]
async fn complete_total_stream_header_and_stream_idle_deadlines_are_distinct() {
    for case in 0..3 {
        let (listener, endpoint) = listener().await;
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            read_request(&mut socket).await;
            if case != 1 {
                let content_type = if case == 0 {
                    "application/json"
                } else {
                    "text/event-stream"
                };
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nTransfer-Encoding: chunked\r\n\r\n").as_bytes()).await.unwrap();
            }
            tokio::time::sleep(Duration::from_millis(180)).await;
        });
        let mut timeouts = Timeouts {
            completion: Duration::from_secs(2),
            headers: Duration::from_secs(2),
            idle: Duration::from_secs(2),
            ..Timeouts::default()
        };
        let mode = if case == 0 {
            WireMode::Complete
        } else {
            WireMode::Stream
        };
        match case {
            0 => timeouts.completion = Duration::from_millis(40),
            1 => timeouts.headers = Duration::from_millis(40),
            _ => timeouts.idle = Duration::from_millis(40),
        }
        let error = admit(
            model(&endpoint, timeouts)
                .prepare(uuid::Uuid::new_v4(), payload(), mode)
                .unwrap(),
        )
        .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
        .unwrap()
        .wait()
        .await
        .err()
        .unwrap();
        assert_eq!(error.failure, Failure::Timeout);
        server.await.unwrap();
    }
}

#[tokio::test]
async fn admission_refusal_drop_and_expiry_send_nothing() {
    let (listener, endpoint) = listener().await;
    let model = Model::new(
        snapshot(&endpoint),
        swarmie_ai::Protocol::Responses,
        AuthProfile::ApiKey,
        Some(Zeroizing::new("key".into())),
        Some(now() + 150),
        Timeouts::default(),
    )
    .unwrap();
    drop(
        model
            .prepare(uuid::Uuid::new_v4(), payload(), WireMode::Complete)
            .unwrap(),
    );
    let refused = model
        .prepare(uuid::Uuid::new_v4(), payload(), WireMode::Complete)
        .unwrap()
        .admit(|_, _, _, _| {
            Err(Error {
                failure: Failure::Admission,
                effect: Effect::NotDispatched,
            })
        });
    assert!(refused.is_err());
    let admitted = admit(
        model
            .prepare(uuid::Uuid::new_v4(), payload(), WireMode::Complete)
            .unwrap(),
    );
    tokio::time::sleep(Duration::from_millis(170)).await;
    assert_eq!(
        admitted
            .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
            .err()
            .unwrap()
            .effect,
        Effect::NotDispatched
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(30), listener.accept())
            .await
            .is_err()
    );
}

#[test]
fn invalid_configuration_and_unqualified_profiles_fail_before_io() {
    let snapshot = snapshot("https://provider.example/responses");
    for profile in [AuthProfile::ChatGpt, AuthProfile::LegacyCodex] {
        let error = Model::new(
            snapshot.clone(),
            swarmie_ai::Protocol::Responses,
            profile,
            Some(Zeroizing::new("key".into())),
            None,
            Timeouts::default(),
        )
        .err()
        .unwrap();
        assert_eq!(error.failure, Failure::UnsupportedProfile);
    }
    let error = Model::new(
        snapshot,
        swarmie_ai::Protocol::Responses,
        AuthProfile::ApiKey,
        Some(Zeroizing::new("key\nsecret".into())),
        None,
        Timeouts::default(),
    )
    .err()
    .unwrap();
    assert!(!format!("{error:?}").contains("secret"));
    assert_eq!(error.effect, Effect::NotDispatched);
}

#[test]
fn model_mode_limits_hidden_context_and_unknown_item_variants_refuse() {
    let model = model("https://provider.example/responses", Timeouts::default());
    for change in [
        json!({"model":"other"}),
        json!({"stream":true}),
        json!({"max_output_tokens":0}),
        json!({"max_output_tokens":1001}),
        json!({"store":true}),
        json!({"background":true}),
        json!({"conversation":"hidden"}),
        json!({"input":[{"type":"item_reference", "id":"hidden"}]}),
        json!({"tools":[{"type":"web_search"}]}),
        json!({"previous_response_id":"hidden"}),
        json!({"truncation":"auto"}),
        json!({"input":[{"type":"unknown_item","opaque":"preserve or refuse"}]}),
    ] {
        let mut input = payload();
        input
            .as_object_mut()
            .unwrap()
            .extend(change.as_object().unwrap().clone());
        assert!(
            model
                .prepare(uuid::Uuid::new_v4(), input, WireMode::Complete)
                .is_err()
        );
    }
}

#[test]
fn reasoning_and_complete_tool_arguments_survive_request_preparation() {
    let model = model("https://provider.example/responses", Timeouts::default());
    let input = json!([
        {"type":"reasoning", "id":"rs_1", "summary":[], "encrypted_content":"opaque", "vendor_extension":"kept"},
        {"type":"function_call", "call_id":"call_1", "name":"tool", "arguments":"{ \"value\": 2 }"},
        {"type":"function_call_output", "call_id":"call_1", "output":"result"}]);
    let mut request = payload();
    request["input"] = input.clone();
    let prepared = model
        .prepare(uuid::Uuid::new_v4(), request, WireMode::Complete)
        .unwrap();
    prepared
        .admit(|_, bytes, _, _| {
            let body: Value = serde_json::from_slice(bytes).unwrap();
            assert_eq!(body["input"], input);
            Ok(())
        })
        .unwrap();
}

#[tokio::test]
async fn revoked_authority_and_stop_before_transport_send_nothing() {
    let (listener, endpoint) = listener().await;
    let model = model(&endpoint, Timeouts::default());
    let execution = admit(
        model
            .prepare(uuid::Uuid::new_v4(), payload(), WireMode::Complete)
            .unwrap(),
    )
    .dispatch(&tokio::runtime::Handle::current(), |_| {
        Err(Error {
            failure: Failure::Admission,
            effect: Effect::Unknown,
        })
    })
    .unwrap();
    assert_eq!(
        execution.wait().await.err().unwrap(),
        Error {
            failure: Failure::Admission,
            effect: Effect::NotDispatched
        }
    );
    let execution = admit(
        model
            .prepare(uuid::Uuid::new_v4(), payload(), WireMode::Stream)
            .unwrap(),
    )
    .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
    .unwrap();
    execution.stop();
    assert_eq!(
        execution.wait().await.err().unwrap(),
        Error {
            failure: Failure::Stopped,
            effect: Effect::NotDispatched
        }
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(30), listener.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn malformed_and_failed_responses_and_byte_limits_remain_unknown() {
    for case in 0..4 {
        let (listener, endpoint) = listener().await;
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            read_request(&mut socket).await;
            let mut value = response();
            match case {
                0 => value["status"] = "failed".into(),
                1 => {
                    value.as_object_mut().unwrap().remove("usage");
                }
                2 => value["model"] = "different-model".into(),
                _ => {}
            }
            reply(
                &mut socket,
                "200 OK",
                "application/json",
                &value.to_string(),
            )
            .await;
        });
        let mut timeouts = Timeouts::default();
        if case == 3 {
            timeouts.max_response_bytes = 4;
        }
        let execution = admit(
            model(&endpoint, timeouts)
                .prepare(uuid::Uuid::new_v4(), payload(), WireMode::Complete)
                .unwrap(),
        )
        .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
        .unwrap();
        let error = execution.wait().await.err().unwrap();
        assert_eq!(error.effect, Effect::Unknown);
        assert_eq!(
            error.failure,
            if case == 3 {
                Failure::Limit
            } else if case == 0 {
                Failure::Provider
            } else {
                Failure::Protocol
            }
        );
        server.await.unwrap();
    }
}

#[tokio::test]
async fn incomplete_terminal_retains_partial_content_and_usage() {
    let (listener, endpoint) = listener().await;
    let mut value = response();
    value["status"] = "incomplete".into();
    value["incomplete_details"] = json!({"reason":"max_output_tokens"});
    let expected = value.clone();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        let body = format!(
            "data: {}\n\n",
            json!({"type":"response.incomplete", "response":value})
        );
        reply(&mut socket, "200 OK", "text/event-stream", &body).await;
    });
    let execution = admit(
        model(&endpoint, Timeouts::default())
            .prepare(uuid::Uuid::new_v4(), payload(), WireMode::Stream)
            .unwrap(),
    )
    .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
    .unwrap();
    assert_eq!(
        execution.wait().await.unwrap().response().unwrap(),
        &expected
    );
    server.await.unwrap();
}

#[tokio::test]
async fn runtime_loss_provides_no_replay_authority() {
    let (listener, endpoint) = listener().await;
    let stopped = std::thread::spawn(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = runtime.handle().clone();
        drop(runtime);
        handle
    })
    .join()
    .unwrap();
    let execution = admit(
        model(&endpoint, Timeouts::default())
            .prepare(uuid::Uuid::new_v4(), payload(), WireMode::Complete)
            .unwrap(),
    )
    .dispatch(&stopped, |_| Ok(()))
    .unwrap();
    assert_eq!(
        execution.wait().await.err().unwrap(),
        Error {
            failure: Failure::Runtime,
            effect: Effect::Unknown
        }
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(30), listener.accept())
            .await
            .is_err()
    );
}

#[test]
fn another_adapter_and_missing_capability_evidence_are_not_qualified() {
    let other = snapshot_for(
        "https://provider.example/responses",
        "another-adapter",
        true,
    );
    let error = Model::new(
        other,
        swarmie_ai::Protocol::Responses,
        AuthProfile::ApiKey,
        Some(Zeroizing::new("key".into())),
        None,
        Timeouts::default(),
    )
    .err()
    .unwrap();
    assert_eq!(error.failure, Failure::Metadata);
    let snapshot = snapshot_for(
        "https://provider.example/responses",
        swarmie_ai::Protocol::Responses.adapter(),
        false,
    );
    let model = Model::new(
        snapshot,
        swarmie_ai::Protocol::Responses,
        AuthProfile::ApiKey,
        Some(Zeroizing::new("key".into())),
        None,
        Timeouts::default(),
    )
    .unwrap();
    assert!(
        model
            .prepare(uuid::Uuid::new_v4(), payload(), WireMode::Stream)
            .is_err()
    );
    let mut request = payload();
    request["tools"] = json!([{"type":"function", "name":"tool", "parameters":{"type":"object"}}]);
    assert!(
        model
            .prepare(uuid::Uuid::new_v4(), request, WireMode::Complete)
            .is_err()
    );
}

#[tokio::test]
async fn complete_calls_do_not_inherit_stream_header_or_idle_deadlines() {
    let (listener, endpoint) = listener().await;
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        tokio::time::sleep(Duration::from_millis(40)).await;
        let body = response().to_string();
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        socket.write_all(headers.as_bytes()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(40)).await;
        socket.write_all(body.as_bytes()).await.unwrap();
    });
    let timeouts = Timeouts {
        completion: Duration::from_secs(2),
        headers: Duration::from_millis(5),
        idle: Duration::from_millis(5),
        ..Timeouts::default()
    };
    let execution = admit(
        model(&endpoint, timeouts)
            .prepare(uuid::Uuid::new_v4(), payload(), WireMode::Complete)
            .unwrap(),
    )
    .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
    .unwrap();
    assert_eq!(
        execution.wait().await.unwrap().response().unwrap(),
        &response()
    );
    server.await.unwrap();
}

#[tokio::test]
async fn typed_responses_share_admission_and_keep_usage_after_observer_detach() {
    let (listener, endpoint) = listener().await;
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let request = read_request(&mut socket).await;
        let body = format!(
            "data: {}\n\n",
            json!({"type":"response.completed","response":response()})
        );
        reply(&mut socket, "200 OK", "text/event-stream", &body).await;
        request
    });
    let prepared = model(&endpoint, Timeouts::default())
        .prepare(
            uuid::Uuid::new_v4(),
            swarmie_ai::CompletionRequest::new("Continue.").max_tokens(100),
            WireMode::Stream,
        )
        .unwrap();
    let mut bytes = Vec::new();
    let admitted = prepared
        .admit(|_, body, headers, _| {
            bytes = body.to_vec();
            assert_eq!(headers["authorization"], "Bearer fixture-secret");
            assert_eq!(headers["accept"], "text/event-stream");
            Ok(())
        })
        .unwrap();
    let execution = admitted
        .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
        .unwrap();
    drop(execution.previews());
    let outcome = execution.wait().await.unwrap();
    assert!(outcome.completion().unwrap().is_ok());
    assert_eq!(outcome.provider_request_id(), Some("fixture-transport-id"));
    assert_eq!(
        outcome
            .completion()
            .unwrap()
            .unwrap()
            .provider_request_id
            .as_deref(),
        Some("fixture-transport-id")
    );
    assert_eq!(outcome.usage().unwrap().input_tokens, Some(12));
    assert_eq!(outcome.usage().unwrap().output_tokens, Some(8));
    let request = server.await.unwrap();
    let end = request
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .unwrap();
    assert_eq!(request[end + 4..], bytes);
}

#[test]
fn typed_history_and_options_refuse_instead_of_repairing_or_adding_hidden_state() {
    use swarmie_ai::codecs::message::{
        AssistantContent, AssistantMessage, CallId, Message, ToolName,
    };
    let model = model("https://example.test/v1/responses", Timeouts::default());
    let name = ToolName::new("lookup").unwrap();
    let call = AssistantContent::tool_call("call-1", name.clone(), json!({"key":1}));
    let unanswered = Message::Assistant(AssistantMessage::new(vec![call]));
    let orphan = Message::tool_result(CallId::from_wire("missing-call"), name, "result");
    for message in [unanswered, orphan] {
        let mut request = swarmie_ai::CompletionRequest::new("Continue.").max_tokens(100);
        request.chat_history.push(Arc::new(message));
        assert_eq!(
            model
                .prepare(uuid::Uuid::new_v4(), request, WireMode::Complete)
                .err()
                .unwrap()
                .failure,
            Failure::Request
        );
    }
    for params in [
        json!({"store":true}),
        json!({"stream":true}),
        json!({"truncation":"auto"}),
        json!({"previous_response_id":"hidden"}),
        json!({"max_output_tokens":101}),
    ] {
        let mut request = swarmie_ai::CompletionRequest::new("Continue.").max_tokens(100);
        request.additional_params = Some(params);
        assert_eq!(
            model
                .prepare(uuid::Uuid::new_v4(), request, WireMode::Complete)
                .err()
                .unwrap()
                .failure,
            Failure::Request
        );
    }
    let request = swarmie_ai::CompletionRequest::new("Continue.")
        .max_tokens(100)
        .model("other-model");
    assert!(
        model
            .prepare(uuid::Uuid::new_v4(), request, WireMode::Complete)
            .is_err()
    );
}

#[tokio::test]
async fn typed_responses_continuation_preserves_native_arguments_ciphertext_and_extensions() {
    use swarmie_ai::codecs::{
        completion::ToolDefinition,
        message::{Message, ToolName, ToolResultContent},
    };
    let mut expected = response();
    expected["output"]
        .as_array_mut()
        .unwrap()
        .retain(|item| item["type"] != "future_output");
    expected["output"][0]["summary"] = json!([]);
    let reply_document = expected.clone();
    let (listener, endpoint) = listener().await;
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        read_request(&mut socket).await;
        reply(
            &mut socket,
            "200 OK",
            "application/json",
            &reply_document.to_string(),
        )
        .await;
    });
    let tool = ToolDefinition::new(
        ToolName::new("tool").unwrap(),
        "Look up a key.",
        json!({"type":"object"}),
    );
    let model = model(&endpoint, Timeouts::default());
    let outcome = model
        .prepare(
            uuid::Uuid::new_v4(),
            swarmie_ai::CompletionRequest::new("Continue.")
                .max_tokens(100)
                .tool(tool.clone()),
            WireMode::Complete,
        )
        .unwrap()
        .admit(|_, _, _, _| Ok(()))
        .unwrap()
        .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
        .unwrap()
        .wait()
        .await
        .unwrap();
    let completion = outcome.completion().unwrap().unwrap();
    assert_eq!(completion.raw, expected);
    let results = completion
        .tool_calls()
        .map(|call| call.result(vec![ToolResultContent::text("result")]))
        .collect::<Vec<_>>();
    let mut next = swarmie_ai::CompletionRequest::new("Continue.")
        .max_tokens(100)
        .tool(tool);
    next.chat_history = vec![
        Arc::new(Message::user("Continue.")),
        Arc::new(completion.message().unwrap()),
        Arc::new(Message::tool_results(results)),
    ];
    model
        .prepare(uuid::Uuid::new_v4(), next, WireMode::Complete)
        .unwrap()
        .admit(|_, body, _, _| {
            let body: Value = serde_json::from_slice(body).unwrap();
            let input = body["input"].as_array().unwrap();
            let reasoning = input
                .iter()
                .find(|item| item["type"] == "reasoning")
                .unwrap();
            assert_eq!(reasoning["encrypted_content"], "opaque-value");
            assert_eq!(reasoning["vendor_extension"], json!({"keep":true}));
            let call = input
                .iter()
                .find(|item| item["type"] == "function_call")
                .unwrap();
            assert_eq!(call["arguments"], "{ \"key\": 1 }");
            Ok(())
        })
        .unwrap();
    server.await.unwrap();
}

#[derive(Default)]
struct TraceState {
    calls: usize,
    open: bool,
    closed: bool,
    fields: Vec<(String, String)>,
}

#[derive(Clone, Default)]
struct TraceProbe(std::sync::Arc<std::sync::Mutex<TraceState>>);

struct Fields<'a>(&'a mut Vec<(String, String)>);

impl tracing::field::Visit for Fields<'_> {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0.push((field.to_string(), format!("{value:?}")));
    }
}

impl<S> tracing_subscriber::Layer<S> for TraceProbe
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attributes: &tracing::span::Attributes<'_>,
        _: &tracing::span::Id,
        _: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if attributes
            .metadata()
            .fields()
            .field("gen_ai.operation.name")
            .is_some()
        {
            let mut state = self.0.lock().unwrap();
            state.calls += 1;
            state.open = true;
            attributes.record(&mut Fields(&mut state.fields));
        }
    }

    fn on_record(
        &self,
        id: &tracing::span::Id,
        values: &tracing::span::Record<'_>,
        context: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if context
            .span(id)
            .unwrap()
            .metadata()
            .fields()
            .field("gen_ai.operation.name")
            .is_some()
        {
            values.record(&mut Fields(&mut self.0.lock().unwrap().fields));
        }
    }

    fn on_close(&self, id: tracing::span::Id, context: tracing_subscriber::layer::Context<'_, S>) {
        if context
            .span(&id)
            .unwrap()
            .metadata()
            .fields()
            .field("gen_ai.operation.name")
            .is_some()
        {
            let mut state = self.0.lock().unwrap();
            state.open = false;
            state.closed = true;
        }
    }
}

#[tokio::test]
async fn optional_telemetry_span_covers_physical_transport_and_the_original_wire_mode() {
    use tracing::instrument::WithSubscriber;
    use tracing_subscriber::prelude::*;
    for mode in [WireMode::Complete, WireMode::Stream] {
        let probe = TraceProbe::default();
        let subscriber = tracing_subscriber::registry().with(probe.clone());
        let seen = probe.clone();
        async move {
            let (listener, endpoint) = listener().await;
            let server_probe = probe.clone();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                read_request(&mut socket).await;
                assert_eq!(
                    server_probe.0.lock().unwrap().open,
                    cfg!(feature = "telemetry")
                );
                let (content_type, body) = if mode == WireMode::Stream {
                    (
                        "text/event-stream",
                        format!(
                            "data: {}\n\n",
                            json!({"type":"response.completed","response":response()})
                        ),
                    )
                } else {
                    ("application/json", response().to_string())
                };
                reply(&mut socket, "200 OK", content_type, &body).await;
            });
            let prepared = model(&endpoint, Timeouts::default())
                .prepare(
                    uuid::Uuid::new_v4(),
                    swarmie_ai::CompletionRequest::new("private-prompt").max_tokens(100),
                    mode,
                )
                .unwrap();
            assert_eq!(probe.0.lock().unwrap().calls, 0);
            let outcome = prepared
                .admit(|_, _, _, _| Ok(()))
                .unwrap()
                .dispatch(&tokio::runtime::Handle::current(), |_| Ok(()))
                .unwrap()
                .wait()
                .await
                .unwrap();
            assert!(outcome.completion().unwrap().is_ok());
            server.await.unwrap();
        }
        .with_subscriber(subscriber)
        .await;
        let state = seen.0.lock().unwrap();
        assert_eq!(state.calls, usize::from(cfg!(feature = "telemetry")));
        assert!(!state.open);
        assert_eq!(state.closed, cfg!(feature = "telemetry"));
        assert!(!format!("{:?}", state.fields).contains("private-prompt"));
        assert!(!format!("{:?}", state.fields).contains("fixture-secret"));
        if cfg!(feature = "telemetry") {
            let last = |field: &str| {
                state
                    .fields
                    .iter()
                    .rev()
                    .find(|(name, _)| name == field)
                    .unwrap()
                    .1
                    .clone()
            };
            assert_eq!(
                last("gen_ai.request.stream"),
                (mode == WireMode::Stream).to_string()
            );
            assert_eq!(last("gen_ai.usage.input_tokens"), "12");
            assert!(last("swarmie.provider_request_id").contains("fixture-transport-id"));
        }
    }
}
