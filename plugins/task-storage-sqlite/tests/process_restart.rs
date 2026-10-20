use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

use swarmie_task_contracts::{
    CommitResult, StorageOwner, TaskLookup, TaskRecord, TaskSpec, TaskStatus, TaskStorage,
    TaskStorageInput, TaskStorageRef, TaskTransaction, TaskWrite,
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
