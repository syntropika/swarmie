use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

use swarmie_task_contracts::{
    CommitResult, StorageOwner, TaskLookup, TaskOutboxAcknowledgement, TaskOutboxCompaction,
    TaskOutboxConsumer, TaskOutboxEvent, TaskOutboxQuery, TaskRecord, TaskSpec, TaskStatus,
    TaskStorage, TaskStorageInput, TaskStorageRef, TaskTransaction, TaskWrite,
};
use swarmie_task_storage_sqlite::Configuration;
use tembra::{core, prelude::*};

async fn open(host: &Host, database: &Path) -> Result<TaskStorageRef> {
    host.install(
        swarmie_task_storage_sqlite::facet(&Configuration {
            database: database.into(),
        })?,
        core::LoadOptions::default(),
    )?
    .await?;
    host.get(TaskStorage, TaskStorageInput {}).await
}

struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start(database: &Path, mode: &str) -> Process {
    let mut process = Process(
        Command::new(std::env::current_exe().expect("current test executable"))
            .args(["--exact", "process_child", "--nocapture"])
            .env("SWARMIE_TASK_STORAGE_PROCESS_DATABASE", database)
            .env("SWARMIE_TASK_STORAGE_PROCESS_MODE", mode)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("spawn actual child process"),
    );
    let stdout = process.0.stdout.take().expect("child stdout");
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else {
                break;
            };
            if line == "task-storage-ready" {
                let _ = sender.send(());
                return;
            }
        }
    });
    receiver
        .recv_timeout(Duration::from_secs(30))
        .expect("child committed before process termination");
    process
}

#[test]
fn process_child() -> Result<()> {
    let Some(database) = std::env::var_os("SWARMIE_TASK_STORAGE_PROCESS_DATABASE") else {
        return Ok(());
    };
    let mode = std::env::var("SWARMIE_TASK_STORAGE_PROCESS_MODE").expect("child process mode");
    let driver = core::ManualDriver::new();
    let host = Host::with_driver("storage-child-process", "application", driver.clone())?;
    driver.run_until(async {
        let storage = open(&host, Path::new(&database)).await?;
        storage.claim_owner(StorageOwner { id: "child-process-owner".into() }).await?;
        let mut record = TaskRecord::new(TaskSpec {
            id: "surviving-task".into(), kind: "test.process".into(), definition_version: 1,
            input: serde_json::json!({"report":"durable"}), checkpoint: serde_json::json!({"operation_id":"surviving-task:effect"}), phase: "effect".into(),
        }, None);
        record.status = TaskStatus::Running;
        assert!(matches!(storage.commit(TaskTransaction { owner: "child-process-owner".into(), writes: vec![TaskWrite { expected_revision: None, record }] }).await?, CommitResult::Committed { .. }));
        if mode == "outbox-published" {
            storage.register_outbox_consumer(TaskOutboxConsumer { id:"session-projection".into() }).await?;
            let page=storage.read_outbox(TaskOutboxQuery { consumer_id:"session-projection".into(),after:None,limit:256 }).await?;
            assert_eq!(page.events.len(),1);
            publish_to_sink(Path::new(&database),&page.events[0]);
        }
        let _uncommitted = if mode == "uncommitted" {
            let connection = rusqlite::Connection::open(Path::new(&database)).expect("uncommitted transaction fixture");
            connection.execute_batch("BEGIN IMMEDIATE; UPDATE task_records SET record = 'uncommitted-corruption' WHERE id = 'surviving-task';").expect("uncommitted mutation");
            Some(connection)
        } else { None };
        println!("task-storage-ready");
        std::io::stdout().flush().expect("publish process boundary marker");
        // Keep the real Resource and any uncommitted SQLite transaction alive until the
        // parent terminates this process. No graceful teardown may run at this boundary.
        loop { std::thread::park(); }
    })
}

#[test]
fn killed_process_releases_owner_and_preserves_committed_checkpoint() -> Result<()> {
    for mode in ["committed", "uncommitted"] {
        let directory = tempfile::tempdir().expect("temporary process database directory");
        let database = directory.path().join("tasks.sqlite");
        let mut child = start(&database, mode);
        let driver = core::ManualDriver::new();
        driver.run_until(async {
            let competing =
                Host::with_driver("storage-competing-process", "application", driver.clone())?;
            assert_eq!(
                open(&competing, &database)
                    .await
                    .err()
                    .expect("active owner refusal")
                    .reason,
                "task_storage_owned"
            );
            competing.close().await?;
            child
                .0
                .kill()
                .expect("terminate child without Resource cleanup");
            assert!(!child.0.wait().expect("wait for killed child").success());
            let recovered =
                Host::with_driver("storage-recovery-process", "application", driver.clone())?;
            let storage = open(&recovered, &database).await?;
            storage
                .claim_owner(StorageOwner {
                    id: "successor-process-owner".into(),
                })
                .await?;
            let record = storage
                .get(TaskLookup {
                    id: "surviving-task".into(),
                })
                .await?
                .expect("committed task survived process death");
            assert_eq!(record.status, TaskStatus::Running);
            assert_eq!(record.revision, 1);
            assert_eq!(record.checkpoint["operation_id"], "surviving-task:effect");
            recovered.close().await
        })?;
    }
    Ok(())
}

// This deliberately independent durable sink models only an idempotent publication receipt.
// It is not a SessionStore implementation and shares no transaction with TaskStorage.
fn publish_to_sink(database: &Path, event: &TaskOutboxEvent) -> usize {
    let sink = rusqlite::Connection::open(database.with_extension("receipts.sqlite"))
        .expect("open publication sink");
    sink.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; CREATE TABLE IF NOT EXISTS receipts(store_id TEXT NOT NULL,sequence INTEGER NOT NULL,ordinal INTEGER NOT NULL,task_id TEXT NOT NULL,PRIMARY KEY(store_id,sequence,ordinal)) WITHOUT ROWID;").expect("durable publication schema");
    sink.execute(
        "INSERT OR IGNORE INTO receipts VALUES(?1,?2,?3,?4)",
        rusqlite::params![
            event.id.store_id,
            event.id.sequence,
            event.id.ordinal,
            event.task_id
        ],
    )
    .expect("idempotent publication receipt")
}

#[test]
fn killed_process_before_publication_or_ack_replays_atomic_transition_intent() -> Result<()> {
    for mode in ["outbox-committed", "outbox-published"] {
        let directory = tempfile::tempdir().expect("temporary outbox process database");
        let database = directory.path().join("tasks.sqlite");
        let mut child = start(&database, mode);
        child
            .0
            .kill()
            .expect("terminate immediately at durable process boundary");
        assert!(!child.0.wait().expect("wait for crash").success());
        let driver = core::ManualDriver::new();
        driver.run_until(async {
            let host = Host::with_driver("outbox-recovery-process", "application", driver.clone())?;
            let storage = open(&host, &database).await?;
            let state = storage
                .register_outbox_consumer(TaskOutboxConsumer {
                    id: "session-projection".into(),
                })
                .await?;
            assert!(
                state.acknowledged.is_none(),
                "publication and delivery never imply durable acknowledgement"
            );
            assert_eq!(state.delivered.is_some(), mode == "outbox-published");
            assert_eq!(
                storage
                    .compact_outbox(TaskOutboxCompaction { limit: 4096 })
                    .await?
                    .removed,
                0
            );
            let page = storage
                .read_outbox(TaskOutboxQuery {
                    consumer_id: "session-projection".into(),
                    after: None,
                    limit: 256,
                })
                .await?;
            assert_eq!(
                page.events.len(),
                1,
                "committed task and outbox row survive the same crash"
            );
            let event = &page.events[0];
            assert_eq!(event.task_id, "surviving-task");
            assert_eq!(event.id.sequence, 1);
            assert_eq!(event.id.ordinal, 0);
            assert_eq!(
                publish_to_sink(&database, event),
                usize::from(mode == "outbox-committed")
            );
            assert_eq!(
                publish_to_sink(&database, event),
                0,
                "duplicate publication is idempotent"
            );
            storage
                .acknowledge_outbox(TaskOutboxAcknowledgement {
                    consumer_id: "session-projection".into(),
                    through: event.id.clone(),
                })
                .await?;
            host.close().await?;
            let host =
                Host::with_driver("outbox-ack-recovery-process", "application", driver.clone())?;
            let storage = open(&host, &database).await?;
            let resumed = storage
                .register_outbox_consumer(TaskOutboxConsumer {
                    id: "session-projection".into(),
                })
                .await?;
            assert_eq!(resumed.acknowledged, Some(event.id.clone()));
            assert!(
                storage
                    .read_outbox(TaskOutboxQuery {
                        consumer_id: "session-projection".into(),
                        after: None,
                        limit: 256
                    })
                    .await?
                    .events
                    .is_empty()
            );
            let sink = rusqlite::Connection::open(database.with_extension("receipts.sqlite"))
                .expect("read sink");
            let count: u64 = sink
                .query_row("SELECT count(*) FROM receipts", [], |row| row.get(0))
                .expect("sink count");
            assert_eq!(count, 1);
            assert_eq!(
                storage
                    .compact_outbox(TaskOutboxCompaction { limit: 4096 })
                    .await?
                    .removed,
                1
            );
            host.close().await
        })?;
    }
    Ok(())
}
