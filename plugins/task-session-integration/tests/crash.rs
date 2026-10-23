mod support;

use std::process::Command;

use swarmie_session_contracts::*;
use swarmie_task_contracts::*;
use swarmie_task_session_contracts::*;
use tembra::prelude::*;

use support::*;

#[test]
fn crash_worker() -> Result<()> {
    let Some(path) = std::env::var_os("SWARMIE_BRIDGE_CRASH_DIRECTORY") else {
        return Ok(());
    };
    let stage = std::env::var("SWARMIE_BRIDGE_CRASH_STAGE").unwrap();
    run(|driver| async move {
        let path = std::path::PathBuf::from(path);
        let h = Harness::open(&path, driver, TaskOutboxLimits::default(), false).await?;
        let request = h.request("crash-task", "chat", "conversation").await?;
        if stage == "before-binding" {
            std::process::exit(42);
        }
        h.bindings.prepare(request.clone()).await?;
        if stage == "after-binding" || stage == "before-task" {
            std::process::exit(42);
        }
        if stage == "after-task" {
            // Force admission confirmation to fail after the real TaskStorage commit. Exit
            // immediately, without Host/Resource cleanup, preserving the pending intent.
            rusqlite::Connection::open(path.join("bindings.sqlite")).unwrap().execute_batch("CREATE TRIGGER stop_activation BEFORE UPDATE ON task_session_bindings BEGIN SELECT RAISE(FAIL,'crash boundary'); END;").unwrap();
            assert_eq!(
                h.bindings.admit(key(&request)).await.unwrap_err().reason,
                "binding_io"
            );
            assert!(
                h.tasks
                    .get(TaskLookup {
                        id: "crash-task".into()
                    })
                    .await?
                    .is_some()
            );
            std::process::exit(42);
        }
        h.bindings.admit(key(&request)).await?;
        if stage == "after-append" || stage == "after-ack" {
            for id in ["crash-second", "crash-third"] {
                let request = h.request(id, "chat", "conversation").await?;
                h.bindings.prepare(request.clone()).await?;
                h.bindings.admit(key(&request)).await?;
            }
        }
        if stage == "after-append" {
            // The real publisher appends, receives the real Session receipt and attempts ACK.
            // SQLite refuses only that ACK; abrupt process death then tests exact redelivery.
            rusqlite::Connection::open(path.join("tasks.sqlite")).unwrap().execute_batch("CREATE TRIGGER stop_ack BEFORE UPDATE OF ack_sequence,ack_ordinal ON task_outbox_consumers WHEN NEW.ack_sequence>OLD.ack_sequence OR NEW.ack_ordinal>OLD.ack_ordinal BEGIN SELECT RAISE(FAIL,'crash boundary'); END;").unwrap();
            assert!(
                h.publisher
                    .publish(PublishRequest { limit: 256 })
                    .await
                    .is_err()
            );
            assert!(
                h.sessions
                    .open(SessionLookup {
                        session_id: "chat".into()
                    })
                    .await?
                    .unwrap()
                    .head
                    > 0
            );
            assert!(
                h.storage.outbox_status().await?.consumers[0]
                    .acknowledged
                    .is_none()
            );
            std::process::exit(42);
        }
        if stage == "after-ack" {
            // The child observes publish success, including its internal ACK reply. It then
            // exits before returning a normal result to the parent; recovery uses durable ACK.
            assert!(
                h.publisher
                    .publish(PublishRequest { limit: 256 })
                    .await?
                    .published
                    >= 3
            );
            std::process::exit(42);
        }
        panic!("unknown crash stage");
    })
}

#[test]
fn abrupt_process_death_reconciles_every_admission_and_publication_boundary() -> Result<()> {
    for stage in [
        "before-binding",
        "after-binding",
        "before-task",
        "after-task",
        "after-append",
        "after-ack",
    ] {
        let directory = tempfile::tempdir().unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "crash_worker", "--nocapture"])
            .env("SWARMIE_BRIDGE_CRASH_DIRECTORY", directory.path())
            .env("SWARMIE_BRIDGE_CRASH_STAGE", stage)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(42),
            "{stage}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let connection =
            rusqlite::Connection::open(directory.path().join("bindings.sqlite")).unwrap();
        connection
            .execute_batch("DROP TRIGGER IF EXISTS stop_activation")
            .unwrap();
        drop(connection);
        let connection = rusqlite::Connection::open(directory.path().join("tasks.sqlite")).unwrap();
        connection
            .execute_batch("DROP TRIGGER IF EXISTS stop_ack")
            .unwrap();
        drop(connection);
        run(|driver| async move {
            let h =
                Harness::open(directory.path(), driver, TaskOutboxLimits::default(), false).await?;
            let request = h.request("crash-task", "chat", "conversation").await?;
            let prior = h.bindings.lookup(key(&request)).await?;
            assert_eq!(prior.is_none(), stage == "before-binding");
            if stage == "after-binding" || stage == "before-task" {
                assert!(
                    h.tasks
                        .get(TaskLookup {
                            id: "crash-task".into()
                        })
                        .await?
                        .is_none()
                );
            }
            h.bindings.prepare(request.clone()).await?;
            let binding = h.bindings.admit(key(&request)).await?;
            assert_eq!(binding.admission, BindingAdmission::Active);
            let before = h.sessions.events(EventQuery::new("chat")).await?.events;
            if stage == "after-append" || stage == "after-ack" {
                assert!(
                    before.len() >= 3,
                    "every confirmed fact precedes the cumulative ACK"
                );
            }
            drain(&h.publisher).await?;
            let after = h.sessions.events(EventQuery::new("chat")).await?.events;
            assert!(after.len() >= before.len());
            for old in before {
                assert_eq!(
                    after
                        .iter()
                        .filter(|event| event.event.event_id == old.event.event_id)
                        .count(),
                    1
                );
            }
            let mut ids = std::collections::BTreeSet::new();
            assert!(
                after
                    .iter()
                    .all(|event| ids.insert(event.event.event_id.clone()))
            );
            let retained = h.storage.outbox_status().await?;
            assert_eq!(retained.pending_events, 0);
            assert_eq!(after.len() as u64, retained.retained_events);
            drain(&h.publisher).await?;
            assert_eq!(
                h.sessions.events(EventQuery::new("chat")).await?.events,
                after
            );
            h.host.close().await
        })?;
    }
    Ok(())
}
