mod support;

use futures::future::join_all;
use serde_json::json;
use std::{cell::RefCell, rc::Rc};
use support::*;
use swarmie_session_contracts::*;
use tembra::prelude::*;

#[test]
fn canonical_create_append_retry_restart_and_resource_closure() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("sessions.sqlite");
    let mut expected = None;
    let database = &database;
    let expected_ref = &mut expected;
    run(|host, _driver| async move {
        let store = open(&host, database.as_path()).await?;
        assert!(
            store
                .open(SessionLookup {
                    session_id: "absent".into()
                })
                .await?
                .is_none()
        );
        let created = create(&store, "chat").await?;
        assert_eq!(create(&store, "chat").await?, created);
        let request = batch("chat", "user:1", vec![message("m1", "user", "Hello")]);
        let receipt = store.append(request.clone()).await?;
        let before = store.committed().value()?;
        assert_eq!(store.append(request).await?, receipt);
        assert_eq!(store.committed().value()?, before);
        *expected_ref = Some(store.events(EventQuery::new("chat")).await?);
        host.close().await?;
        assert!(store.events(EventQuery::new("chat")).await.is_err());
        Ok(())
    })?;
    run(|host, _driver| async move {
        let store = open(&host, database.as_path()).await?;
        assert_eq!(
            store.events(EventQuery::new("chat")).await?,
            expected.unwrap()
        );
        assert_eq!(store.committed().value()?.commit_sequence, 2);
        host.close().await
    })
}

#[test]
fn concurrent_appends_cas_conflicts_and_identity_refusals_are_atomic() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    run(|host, _driver| async move {
        let store = open(&host, &directory.path().join("sessions.sqlite")).await?;
        create(&store, "chat").await?;
        let mut cas = batch("chat", "cas-a", vec![message("m1", "user", "A")]);
        cas.expected_head = Some(0);
        let mut other = batch("chat", "cas-b", vec![message("m2", "user", "B")]);
        other.expected_head = Some(0);
        let results = join_all([store.append(cas.clone()), store.append(other)]).await;
        assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(
            results[1].as_ref().unwrap_err().reason,
            "session_store_conflict"
        );
        let requests: Vec<_> = (0..64)
            .map(|n| {
                batch(
                    "chat",
                    format!("batch:{n}"),
                    vec![message(format!("msg:{n}"), "assistant", "Answer")],
                )
            })
            .collect();
        let receipts = join_all(requests.into_iter().map(|request| store.append(request)))
            .await
            .into_iter()
            .collect::<Result<Vec<_>>>()?;
        assert_eq!(
            receipts
                .iter()
                .map(|r| r.last_sequence)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            64
        );
        assert_eq!(store.append(cas.clone()).await?.last_sequence, 1);
        let before = store.events(EventQuery::new("chat")).await?;
        cas.events[0].payload["content"] = json!("Changed");
        assert_eq!(
            store.append(cas).await.unwrap_err().reason,
            "session_store_duplicate"
        );
        assert_eq!(
            store
                .append(batch("chat", "other-id", vec![message("m1", "user", "A")]))
                .await
                .unwrap_err()
                .reason,
            "session_store_duplicate"
        );
        assert_eq!(
            store
                .create(CreateSession {
                    session_id: "chat".into(),
                    metadata: json!({"other":true})
                })
                .await
                .unwrap_err()
                .reason,
            "session_store_duplicate"
        );
        assert_eq!(store.events(EventQuery::new("chat")).await?, before);
        host.close().await
    })
}

#[test]
fn bounded_prefix_pagination_survives_appends_and_turn_filters() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    run(|host, _driver| async move {
        let store = open(&host, &directory.path().join("sessions.sqlite")).await?;
        create(&store, "chat").await?;
        let events = (0..15)
            .map(|n| {
                let mut event = message(format!("m{n}"), "user", "Text");
                event.correlation.turn_id = Some(format!("turn:{}", n % 2));
                event
            })
            .collect();
        store.append(batch("chat", "initial", events)).await?;
        let mut query = EventQuery::new("chat");
        query.limit = 3;
        let first = store.events(query.clone()).await?;
        store
            .append(batch(
                "chat",
                "later",
                vec![message("later", "assistant", "Later")],
            ))
            .await?;
        let mut all = first.events;
        query.cursor = first.next;
        loop {
            let page = store.events(query.clone()).await?;
            assert_eq!(page.through_sequence, 15);
            all.extend(page.events);
            query.cursor = page.next;
            if query.cursor.is_none() {
                break;
            }
        }
        assert_eq!(
            all.iter().map(|e| e.sequence).collect::<Vec<_>>(),
            (1..=15).collect::<Vec<_>>()
        );
        query = EventQuery::new("chat");
        query.after_sequence = 15;
        assert_eq!(store.events(query).await?.events[0].event.event_id, "later");
        let mut query = EventQuery::new("chat");
        query.turn_id = Some("turn:1".into());
        query.limit = 2;
        let page = store.events(query.clone()).await?;
        assert_eq!(
            page.events.iter().map(|e| e.sequence).collect::<Vec<_>>(),
            vec![2, 4]
        );
        query.cursor = page.next;
        query.turn_id = Some("turn:0".into());
        assert_eq!(
            store.events(query).await.unwrap_err().reason,
            "session_store_cursor"
        );
        host.close().await
    })
}

#[test]
fn projections_preserve_compacted_history_unknown_versions_and_model_neutrality() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    run(|host, _driver| async move {
        let store = open(&host, &directory.path().join("sessions.sqlite")).await?;
        create(&store, "chat").await?;
        let mut future = message("future", "assistant", "Future payload");
        future.version = 2;
        future.payload = json!({"future":true});
        let mut tool = message("tool-result", "tool", "Result");
        tool.payload["tool_call_id"] = json!("call:1");
        tool.correlation.turn_id = Some("turn:1".into());
        let events = vec![
            event(
                "instructions",
                INSTRUCTIONS,
                json!({"scope":"system","content":"Be clear"}),
            ),
            message("user", "user", "Question"),
            event(
                "model",
                "plugin.model.completed",
                json!({"usage":{"input":12,"output":5},"cost":0.01}),
            ),
            tool,
            message("answer", "assistant", "Answer"),
            event(
                "compaction",
                COMPACTION,
                json!({"through_sequence":5,"policy":"fixture","summary":"Summary"}),
            ),
            future,
            event("plugin", "plugin.extra", json!({"extension":true})),
        ];
        store.append(batch("chat", "all", events)).await?;
        let raw = store.events(EventQuery::new("chat")).await?;
        for kind in [
            ProjectionKind::Conversation,
            ProjectionKind::Trajectory,
            ProjectionKind::ModelContext,
        ] {
            let actual = store
                .project(ProjectionQuery {
                    source: EventQuery::new("chat"),
                    kind,
                })
                .await?;
            assert_eq!(actual, project(raw.clone(), kind));
            assert!(actual.uninterpreted >= 1);
            match kind {
                ProjectionKind::Conversation => assert_eq!(actual.entries.len(), 5),
                ProjectionKind::Trajectory => assert_eq!(actual.entries.len(), 8),
                ProjectionKind::ModelContext => {
                    assert_eq!(actual.entries.len(), 4);
                    assert_eq!(actual.entries[1].event.payload["content"], "Question");
                }
            }
        }
        assert_eq!(store.events(EventQuery::new("chat")).await?, raw);
        let mut source = EventQuery::new("chat");
        source.limit = 2;
        let mut replay = vec![];
        loop {
            let page = store
                .project(ProjectionQuery {
                    source: source.clone(),
                    kind: ProjectionKind::Conversation,
                })
                .await?;
            replay.extend(page.entries);
            source.cursor = page.next;
            if source.cursor.is_none() {
                break;
            }
        }
        assert_eq!(replay, project(raw, ProjectionKind::Conversation).entries);
        host.close().await
    })
}

#[test]
fn state_observes_only_commits_and_reconnect_recovers_missed_history() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    run(|host, _driver| async move {
        let store = open(&host, &directory.path().join("sessions.sqlite")).await?;
        let received = Rc::new(RefCell::new(vec![]));
        let callback = received.clone();
        let subscription = store.committed().subscribe(move |snapshot| {
            callback.borrow_mut().push(snapshot.value()?);
            Ok(())
        })?;
        create(&store, "chat").await?;
        store
            .append(batch("chat", "first", vec![message("m1", "user", "A")]))
            .await?;
        assert_eq!(received.borrow().last().unwrap().commit_sequence, 2);
        assert!(
            store
                .append(batch("chat", "duplicate", vec![message("m1", "user", "A")]))
                .await
                .is_err()
        );
        assert_eq!(received.borrow().last().unwrap().commit_sequence, 2);
        drop(subscription);
        store
            .append(batch(
                "chat",
                "second",
                vec![message("m2", "assistant", "B")],
            ))
            .await?;
        let latest = Rc::new(RefCell::new(None));
        let callback = latest.clone();
        let _subscription = store.committed().subscribe(move |snapshot| {
            *callback.borrow_mut() = Some(snapshot.value()?);
            Ok(())
        })?;
        assert_eq!(latest.borrow().as_ref().unwrap().commit_sequence, 3);
        let mut query = EventQuery::new("chat");
        query.after_sequence = 1;
        assert_eq!(store.events(query).await?.events[0].event.event_id, "m2");
        host.close().await
    })
}

#[test]
fn failures_before_commit_rollback_all_rows_and_busy_writer_refuses_promptly() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("sessions.sqlite");
    let database = &database;
    run(|host, _driver| async move {
        let store = open(&host, database.as_path()).await?;
        create(&store, "chat").await?;
        let raw = rusqlite::Connection::open(database.as_path()).unwrap();
        raw.execute_batch("CREATE TRIGGER fail_second BEFORE INSERT ON session_events WHEN NEW.event_id='second' BEGIN SELECT RAISE(FAIL,'injected failure'); END;").unwrap();
        let before = store.committed().value()?;
        assert_eq!(
            store
                .append(batch(
                    "chat",
                    "failed",
                    vec![
                        message("first", "user", "A"),
                        message("second", "assistant", "B")
                    ]
                ))
                .await
                .unwrap_err()
                .reason,
            "session_store_io"
        );
        assert!(
            store
                .events(EventQuery::new("chat"))
                .await?
                .events
                .is_empty()
        );
        assert_eq!(store.committed().value()?, before);
        raw.execute_batch("DROP TRIGGER fail_second; BEGIN IMMEDIATE;")
            .unwrap();
        let started = std::time::Instant::now();
        assert_eq!(
            store
                .append(batch("chat", "busy", vec![message("m1", "user", "A")]))
                .await
                .unwrap_err()
                .reason,
            "session_store_io"
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        raw.execute_batch("ROLLBACK").unwrap();
        assert_eq!(
            store
                .append(batch("chat", "busy", vec![message("m1", "user", "A")]))
                .await?
                .last_sequence,
            1
        );
        assert!(raw.execute("DELETE FROM session_events", []).is_err());
        host.close().await
    })
}

#[test]
fn canonical_sizes_bytes_and_portable_numbers_bound_memory_and_retry_equality() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    run(|host, _driver| async move {
        let store = open(&host, &directory.path().join("sessions.sqlite")).await?;
        create(&store, "chat").await?;
        let mut events = vec![];
        for n in 0..8 {
            events.push(message(
                format!("large:{n}"),
                "assistant",
                &"é\\\"".repeat(10_000),
            ));
        }
        store.append(batch("chat", "large", events)).await?;
        let mut query = EventQuery::new("chat");
        query.max_bytes = 100_000;
        let page = store.events(query.clone()).await?;
        assert!(wire_bytes(&page)? <= 100_000);
        assert!(page.next.is_some());
        assert!(page.events.len() < 8);
        query.max_bytes = 4096;
        assert_eq!(
            store.events(query).await.unwrap_err().reason,
            "session_store_too_large"
        );
        let before = store.committed().value()?;
        assert_eq!(
            store
                .append(batch(
                    "chat",
                    "oversized",
                    vec![message("too-large", "user", &"x".repeat(MAX_EVENT_BYTES))]
                ))
                .await
                .unwrap_err()
                .reason,
            "session_store_too_large"
        );
        assert_eq!(store.committed().value()?, before);
        let a = batch(
            "chat",
            "numeric",
            vec![event("number", "plugin.numeric", json!({"value":1.0}))],
        );
        let first = store.append(a.clone()).await?;
        let mut normalized = a;
        normalized.events[0].payload = json!({"value":1});
        assert_eq!(store.append(normalized).await?, first);
        assert!(canonical(&json!({"unsafe":9_007_199_254_740_992_u64})).is_err());
        host.close().await
    })
}

#[test]
fn schema_owner_validation_and_corruption_are_explicit() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("sessions.sqlite");
    let database = &database;
    run(|host, _driver| async move {
        let store = open(&host, database.as_path()).await?;
        create(&store, "chat").await?;
        let other = tembra::Host::with_driver("competitor", "application", _driver.clone())?;
        assert_eq!(
            open(&other, database.as_path()).await.err().unwrap().reason,
            "session_store_owned"
        );
        other.close().await?;
        for path in ["", ":memory:", "file:db"] {
            assert!(
                swarmie_session_store_sqlite::facet(&swarmie_session_store_sqlite::Configuration {
                    database: path.into()
                })
                .is_err()
            );
        }
        let raw = rusqlite::Connection::open(database.as_path()).unwrap();
        raw.execute_batch("PRAGMA user_version=99").unwrap();
        host.close().await
    })?;
    run(|host, _driver| async move {
        assert_eq!(
            open(&host, database.as_path()).await.err().unwrap().reason,
            "session_store_schema"
        );
        host.close().await
    })
}

#[test]
fn publication_failure_does_not_undo_sqlite_and_exact_retry_recovers_receipt() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    run(|host, _driver| async move {
        let store = open(&host, &directory.path().join("sessions.sqlite")).await?;
        create(&store, "chat").await?;
        let _failing = store.committed().subscribe(|snapshot| {
            if snapshot.value()?.commit_sequence > 1 {
                Err(RuntimeError::unavailable(
                    "fixture_observer",
                    "publication failure",
                ))
            } else {
                Ok(())
            }
        })?;
        let request = batch(
            "chat",
            "uncertain",
            vec![message("m1", "user", "Committed")],
        );
        let result = store.append(request.clone()).await;
        // Core may surface subscriber failure after adopting State. Either way the database
        // receipt and live watermark must describe the committed event, exactly once.
        if let Err(error) = result {
            assert_eq!(error.reason, "session_store_publication");
        }
        assert_eq!(store.committed().value()?.commit_sequence, 2);
        let receipt = store.append(request).await?;
        assert_eq!(receipt.last_sequence, 1);
        assert_eq!(store.events(EventQuery::new("chat")).await?.events.len(), 1);
        host.close().await
    })
}

#[test]
fn crash_child() -> Result<()> {
    use std::io::Write;
    let Some(path) = std::env::var_os("SWARMIE_SESSION_CRASH_DATABASE") else {
        return Ok(());
    };
    let mode = std::env::var("SWARMIE_SESSION_CRASH_MODE").unwrap();
    run(|host, _driver| async move {
        let path = std::path::Path::new(&path);
        let store = open(&host, path).await?;
        create(&store, "chat").await?;
        store
            .append(batch(
                "chat",
                "committed",
                vec![message("m1", "user", "Survives")],
            ))
            .await?;
        let raw = rusqlite::Connection::open(path).unwrap();
        if mode == "before_commit" {
            raw.execute_batch("BEGIN IMMEDIATE; UPDATE sessions SET head=2 WHERE id='chat'; UPDATE session_store SET sequence=3;").unwrap();
        }
        println!("session-crash-boundary");
        std::io::stdout().flush().unwrap();
        loop {
            std::thread::park();
        }
    })
}

#[test]
fn process_death_before_and_after_commit_preserves_only_confirmed_rows() -> Result<()> {
    use std::{
        io::{BufRead, BufReader},
        process::{Command, Stdio},
        sync::mpsc,
        time::Duration,
    };
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    for mode in ["before_commit", "after_commit"] {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("sessions.sqlite");
        let mut child = Child(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "crash_child", "--nocapture"])
                .env("SWARMIE_SESSION_CRASH_DATABASE", &database)
                .env("SWARMIE_SESSION_CRASH_MODE", mode)
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        );
        let output = child.0.stdout.take().unwrap();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                if line.unwrap() == "session-crash-boundary" {
                    let _ = sender.send(());
                    break;
                }
            }
        });
        receiver
            .recv_timeout(Duration::from_secs(20))
            .expect("child commit boundary");
        child.0.kill().unwrap();
        assert!(!child.0.wait().unwrap().success());
        run(|host, _driver| async move {
            let store = open(&host, database.as_path()).await?;
            let page = store.events(EventQuery::new("chat")).await?;
            assert_eq!(page.events.len(), 1);
            assert_eq!(page.through_sequence, 1);
            assert_eq!(store.committed().value()?.commit_sequence, 2);
            assert_eq!(
                store
                    .append(batch(
                        "chat",
                        "committed",
                        vec![message("m1", "user", "Survives")]
                    ))
                    .await?
                    .last_sequence,
                1
            );
            assert_eq!(
                store
                    .append(batch(
                        "chat",
                        "successor",
                        vec![message("m2", "assistant", "Continues")]
                    ))
                    .await?
                    .last_sequence,
                2
            );
            host.close().await
        })?;
    }
    Ok(())
}

#[test]
#[ignore = "explicit release benchmark through just session-benchmark"]
fn session_volume_benchmark() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    run(|host, _driver| async move {
        let store = open(&host, &directory.path().join("sessions.sqlite")).await?;
        create(&store, "large").await?;
        let started = std::time::Instant::now();
        for n in 0..200 {
            let events = (0..100)
                .map(|i| message(format!("message:{n}:{i}"), "assistant", &"x".repeat(256)))
                .collect();
            store
                .append(batch("large", format!("batch:{n}"), events))
                .await?;
        }
        let append = started.elapsed();
        let replay_started = std::time::Instant::now();
        let mut query = EventQuery::new("large");
        let mut count = 0;
        let mut pages = 0;
        let mut peak = 0;
        loop {
            let page = store.events(query.clone()).await?;
            peak = peak.max(wire_bytes(&page)?);
            count += page.events.len();
            pages += 1;
            query.cursor = page.next;
            if query.cursor.is_none() {
                break;
            }
        }
        assert_eq!(count, 20_000);
        assert!(peak <= MAX_PAGE_BYTES);
        println!(
            "session_benchmark events={count} pages={pages} append_ms={} replay_ms={} peak_page_bytes={peak}",
            append.as_millis(),
            replay_started.elapsed().as_millis()
        );
        host.close().await
    })
}

#[test]
fn generated_typescript_runs_against_rust_sqlite_through_actual_tembra_executor() -> Result<()> {
    use swarmie_counter::{DynamicConsumer, DynamicConsumerInput};
    use tembra_executor_js::{BundledJsArtifact, Executor, ExecutorOptions};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let inventory: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("target/plugins/artifacts.json"))
            .expect("run just tembra-plugins before session acceptance"),
    )
    .unwrap();
    let artifact = root.join(
        inventory["session-client"]
            .as_str()
            .expect("session-client generated artifact"),
    );
    let artifact = BundledJsArtifact::from_json(&std::fs::read_to_string(artifact).unwrap())?;
    let directory = tempfile::tempdir().unwrap();
    run(|host, driver| async move {
        let store = open(&host, &directory.path().join("sessions.sqlite")).await?;
        let executor = Executor::new(driver, ExecutorOptions::default())?;
        let definition = executor
            .prepare(host.core().id(), artifact, "default".into())
            .await?;
        host.core()
            .load_executable(vec![definition], tembra::core::LoadOptions::default())?
            .await?;
        let client = host.get(DynamicConsumer, DynamicConsumerInput {}).await?;
        assert_eq!(client.run().await?, 1);
        assert_eq!(client.run().await?, 1);
        let page = store.events(EventQuery::new("typescript")).await?;
        assert_eq!(page.events.len(), 1);
        assert_eq!(
            page.events[0].event.payload["content"][0]["text"],
            "Hello from TypeScript"
        );
        assert!(executor.take_errors().is_empty());
        host.close().await
    })
}

#[tembra::contract(
    remote,
    name = "fixture.session-gateway",
    version = 1,
    host = "session-server"
)]
trait SessionGateway {
    #[state]
    fn committed(&self) -> StoreHead;
    async fn append(&self, request: AppendEvents) -> Result<AppendReceipt>;
    async fn events(&self, query: EventQuery) -> Result<EventPage>;
}

struct Gateway {
    store: SessionStoreRef,
    disconnect: Rc<RefCell<Option<tembra::core::rpc::Connection>>>,
}

impl SessionGatewayProvider for Gateway {
    fn committed(&self) -> Readable<StoreHead> {
        self.store.committed()
    }
    async fn append(&self, request: AppendEvents, context: Context) -> Result<AppendReceipt> {
        let receipt = self.store.with_context(&context).append(request).await?;
        if let Some(connection) = self.disconnect.borrow_mut().take() {
            connection.close(tembra::core::rpc::CloseReason::closed());
        }
        Ok(receipt)
    }
    async fn events(&self, query: EventQuery, context: Context) -> Result<EventPage> {
        self.store.with_context(&context).events(query).await
    }
}

#[test]
fn actual_rpc_disconnect_after_commit_retries_exact_batch_and_recovers_state() -> Result<()> {
    use tembra::core::{self, rpc::*};
    let directory = tempfile::tempdir().unwrap();
    run(|server, driver| async move {
        let store = open(&server, &directory.path().join("sessions.sqlite")).await?;
        create(&store, "chat").await?;
        let disconnect = Rc::new(RefCell::new(None));
        let factory = disconnect.clone();
        server
            .install(
                NativeFacet::new("session-gateway", vec![], move |_, setup| {
                    let disconnect = factory.clone();
                    setup.serve(SessionGateway, move |_, _, declaration| {
                        Ok(Gateway {
                            store: declaration.get(SessionStore, SessionStoreInput {})?,
                            disconnect: disconnect.clone(),
                        })
                    })
                })?,
                core::LoadOptions::default(),
            )?
            .await?;
        server.core().set_rpc_identity(RootOrigin::new(
            "session-server",
            "11111111111111111111111111111111",
        )?)?;
        let request = batch(
            "chat",
            "rpc-request",
            vec![message(
                "rpc-message",
                "user",
                "Persisted before connection loss",
            )],
        );
        for attempt in 0..2 {
            let client = Host::with_driver(
                format!("session-client:{attempt}"),
                "application",
                driver.clone(),
            )?;
            client.core().set_rpc_identity(RootOrigin::new(
                format!("session-client:{attempt}"),
                format!("{:032x}", attempt + 2),
            )?)?;
            let (left, right, _control) =
                memory_link(driver.clone(), MemoryLinkOptions::default())?;
            let outgoing = client.core().connect_client(
                left,
                "session-server",
                ConnectionOptions::default(),
            )?;
            let incoming = server.core().accept_client(
                right,
                "application".into(),
                ConnectionOptions::default(),
            )?;
            client.core().set_route("session-server", outgoing)?;
            let remote = client.get(SessionGateway, SessionGatewayInput {}).await?;
            if attempt == 0 {
                *disconnect.borrow_mut() = Some(incoming);
                assert_eq!(
                    remote.append(request.clone()).await.unwrap_err().reason,
                    "unknown_outcome"
                );
                assert_eq!(store.events(EventQuery::new("chat")).await?.events.len(), 1);
            } else {
                assert_eq!(remote.append(request.clone()).await?.last_sequence, 1);
                let page = remote.events(EventQuery::new("chat")).await?;
                assert_eq!(page.events.len(), 1);
                assert_eq!(remote.committed().value()?.commit_sequence, 2);
            }
            client.close().await?;
        }
        server.close().await
    })
}

#[test]
fn invalid_batches_and_corrupted_rows_never_become_valid_history() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    run(|host, _driver| async move {
        let database = directory.path().join("sessions.sqlite");
        let store = open(&host, &database).await?;
        create(&store, "chat").await?;
        let before = store.committed().value()?;
        for request in [
            batch("chat", "empty", vec![]),
            batch(
                "chat",
                "repeated",
                vec![message("same", "user", "A"), message("same", "user", "B")],
            ),
            batch(
                "chat",
                "over-limit",
                (0..129)
                    .map(|n| message(format!("m{n}"), "user", "A"))
                    .collect(),
            ),
            batch(
                "chat",
                "bad-compaction",
                vec![event(
                    "c",
                    COMPACTION,
                    json!({"through_sequence":1,"policy":"fixture","summary":"Premature"}),
                )],
            ),
        ] {
            assert!(store.append(request).await.is_err());
        }
        assert_eq!(store.committed().value()?, before);
        assert!(
            store
                .events(EventQuery::new("chat"))
                .await?
                .events
                .is_empty()
        );
        store
            .append(batch("chat", "valid", vec![message("m1", "user", "A")]))
            .await?;
        let raw = rusqlite::Connection::open(&database).unwrap();
        raw.execute_batch(
            "DROP TRIGGER immutable_events_update; UPDATE session_events SET event='not-json';",
        )
        .unwrap();
        assert_eq!(
            store
                .events(EventQuery::new("chat"))
                .await
                .unwrap_err()
                .reason,
            "session_store_corrupt"
        );
        host.close().await
    })
}

#[test]
fn cursor_rejects_different_database_and_byte_splits_replay_every_event() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    let first_database = directory.path().join("first.sqlite");
    let second_database = directory.path().join("second.sqlite");
    let cursor = Rc::new(RefCell::new(None));
    let first = cursor.clone();
    run(|host, _driver| async move {
        let store = open(&host, &first_database).await?;
        create(&store, "chat").await?;
        for n in 0..3 {
            store
                .append(batch(
                    "chat",
                    format!("batch{n}"),
                    vec![message(format!("m{n}"), "user", &"x".repeat(60_000))],
                ))
                .await?;
        }
        let mut query = EventQuery::new("chat");
        query.max_bytes = 70_000;
        let page = store.events(query.clone()).await?;
        *first.borrow_mut() = page.next.clone();
        let mut count = page.events.len();
        query.cursor = page.next;
        while query.cursor.is_some() {
            let page = store.events(query.clone()).await?;
            assert!(wire_bytes(&page)? <= 70_000);
            count += page.events.len();
            query.cursor = page.next;
        }
        assert_eq!(count, 3);
        host.close().await
    })?;
    run(|host, _driver| async move {
        let store = open(&host, &second_database).await?;
        create(&store, "chat").await?;
        let mut query = EventQuery::new("chat");
        query.cursor = cursor.borrow().clone();
        assert_eq!(
            store.events(query).await.unwrap_err().reason,
            "session_store_cursor"
        );
        host.close().await
    })
}
