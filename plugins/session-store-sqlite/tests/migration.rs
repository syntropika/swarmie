//! Scale and actual process-death qualification of the v1 conversation-index upgrade.
mod support;

use rusqlite::{Connection, params};
use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use support::*;
use swarmie_session_contracts::*;
use tembra::prelude::*;

const BATCH: usize = 128;

fn seed_event(index: usize) -> NewEvent {
    let mut event = message(format!("event:{index}"), "user", &"x".repeat(256));
    event.correlation.conversation_id = Some(format!("conversation:{}", index % 2));
    event.correlation.turn_id = Some("turn".into());
    event
}

fn seed_batch(start: usize, end: usize) -> AppendEvents {
    batch(
        "history",
        format!("batch:{start}"),
        (start..end).map(seed_event).collect(),
    )
}

struct Fixture {
    rows: usize,
    json_bytes: usize,
    first_receipt: String,
}

/// Generate the prior physical layout with bounded batches and exact valid request receipts.
/// Raw fixture insertion avoids timing a million public appends as part of migration itself.
fn seed(path: &Path, count: usize) -> Result<Fixture> {
    run(|host, driver| async move {
        let store = open(&host, path, driver).await?;
        create(&store, "history").await?;
        host.close().await
    })?;
    let mut raw = Connection::open(path).unwrap();
    raw.execute_batch("DROP TRIGGER immutable_events_update; DROP INDEX session_events_conversation; DROP INDEX session_events_conversation_turn; ALTER TABLE session_events DROP COLUMN conversation_id; CREATE TRIGGER immutable_events_update BEFORE UPDATE ON session_events BEGIN SELECT RAISE(ABORT,'immutable event'); END; PRAGMA user_version=1;").unwrap();
    let store_id: String = raw
        .query_row("SELECT store_id FROM session_store", [], |row| row.get(0))
        .unwrap();
    let tx = raw.transaction().unwrap();
    let mut json_bytes = 0;
    let mut first_receipt = String::new();
    {
        let mut insert = tx.prepare("INSERT INTO session_events(session_id,sequence,event_id,turn_id,event) VALUES('history',?1,?2,?3,?4)").unwrap();
        let mut receipt = tx
            .prepare("INSERT INTO session_batches VALUES('history',?1,?2,?3)")
            .unwrap();
        for start in (0..count).step_by(BATCH) {
            let end = (start + BATCH).min(count);
            let request = seed_batch(start, end);
            for (offset, event) in request.events.iter().enumerate() {
                let sequence = (start + offset + 1) as u64;
                let envelope = SessionEvent {
                    session_id: "history".into(),
                    sequence,
                    event: event.clone(),
                };
                let encoded = canonical(&envelope)?;
                json_bytes += encoded.len();
                insert
                    .execute(params![
                        sequence,
                        event.event_id,
                        event.correlation.turn_id,
                        encoded
                    ])
                    .unwrap();
            }
            let encoded = canonical(&AppendReceipt {
                store_id: store_id.clone(),
                session_id: "history".into(),
                batch_id: request.batch_id.clone(),
                first_sequence: (start + 1) as u64,
                last_sequence: end as u64,
                commit_sequence: (start / BATCH + 2) as u64,
            })?;
            if start == 0 {
                first_receipt.clone_from(&encoded);
            }
            receipt
                .execute(params![request.batch_id, canonical(&request)?, encoded])
                .unwrap();
        }
    }
    tx.execute("UPDATE sessions SET head=?1", [count]).unwrap();
    tx.execute(
        "UPDATE session_store SET sequence=?1",
        [count.div_ceil(BATCH) + 1],
    )
    .unwrap();
    tx.commit().unwrap();
    raw.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .unwrap();
    Ok(Fixture {
        rows: count,
        json_bytes,
        first_receipt,
    })
}

fn validate_rows(path: &Path, fixture: &Fixture) {
    let raw = Connection::open(path).unwrap();
    assert_eq!(
        raw.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        2
    );
    let (rows, bytes): (usize, usize) = raw
        .query_row(
            "SELECT count(*),sum(length(CAST(event AS BLOB))) FROM session_events",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((rows, bytes), (fixture.rows, fixture.json_bytes));
    assert_eq!(
        raw.query_row(
            "SELECT receipt FROM session_batches WHERE batch_id='batch:0'",
            [],
            |row| row.get::<_, String>(0)
        )
        .unwrap(),
        fixture.first_receipt
    );
    assert_eq!(
        raw.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    assert!(
        raw.execute("UPDATE session_events SET conversation_id='changed'", [])
            .is_err()
    );
}

async fn validate_replay(store: &SessionStoreRef, fixture: &Fixture, full: bool) -> Result<()> {
    let mut query = EventQuery::new("history");
    let mut count = 0;
    loop {
        let page = store.events(query.clone()).await?;
        for event in page.events {
            assert_eq!(event.event, seed_event(event.sequence as usize - 1));
            count += 1;
        }
        query.cursor = page.next;
        if !full || query.cursor.is_none() {
            break;
        }
    }
    if full {
        assert_eq!(count, fixture.rows);
    }
    assert_eq!(
        canonical(&store.append(seed_batch(0, BATCH.min(fixture.rows))).await?)?,
        fixture.first_receipt
    );
    query = EventQuery::new("history");
    query.conversation_id = Some("conversation:1".into());
    let page = store
        .project(ProjectionQuery {
            source: query,
            kind: ProjectionKind::ModelContext,
        })
        .await?;
    assert!(!page.entries.is_empty());
    assert!(page.entries.iter().all(|event| event.sequence % 2 == 0
        && event.event.correlation.conversation_id.as_deref() == Some("conversation:1")));
    Ok(())
}

struct KilledChild(Child);

impl Drop for KilledChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn migration_child() -> Result<()> {
    let Some(path) = std::env::var_os("SWARMIE_SESSION_MIGRATION_FIXTURE") else {
        return Ok(());
    };
    run(|host, driver| async move {
        open(&host, Path::new(&path), driver).await?;
        host.close().await
    })
}

#[test]
fn process_death_during_backfill_rolls_back_schema_rows_and_triggers() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("sessions.sqlite");
    let fixture = seed(&path, 4096)?;
    let raw = Connection::open(&path).unwrap();
    // This fixture-only SQL trigger is reached by a real backfill update. Force dirty pages
    // into WAL, then retain a CPU window for the parent to kill the process before commit.
    // Nothing is added to the production provider for fault injection.
    raw.execute_batch("CREATE TABLE migration_probe(value BLOB); CREATE TRIGGER migration_pause AFTER UPDATE ON session_events WHEN NEW.sequence=2048 BEGIN INSERT INTO migration_probe SELECT zeroblob(32768) FROM (WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<256) SELECT x FROM n); SELECT sum(x) FROM (WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<100000000) SELECT x FROM n); END; PRAGMA wal_checkpoint(TRUNCATE);").unwrap();
    drop(raw);
    let mut child = KilledChild(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "migration_child", "--nocapture"])
            .env("SWARMIE_SESSION_MIGRATION_FIXTURE", &path)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let wal = PathBuf::from(format!("{}-wal", path.display()));
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "migration must still be in flight"
        );
        if std::fs::metadata(&wal).is_ok_and(|m| m.len() > 4 * 1024 * 1024) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "backfill did not reach fixture update"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let raw = Connection::open(&path).unwrap();
    raw.busy_timeout(Duration::ZERO).unwrap();
    assert_eq!(
        raw.execute_batch("BEGIN IMMEDIATE")
            .unwrap_err()
            .sqlite_error_code(),
        Some(rusqlite::ErrorCode::DatabaseBusy)
    );
    child.0.kill().unwrap();
    assert!(!child.0.wait().unwrap().success());
    drop(raw);
    let raw = Connection::open(&path).unwrap();
    assert_eq!(
        raw.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))
            .unwrap(),
        1
    );
    assert!(
        raw.prepare("SELECT conversation_id FROM session_events")
            .is_err()
    );
    assert_eq!(
        raw.query_row("SELECT count(*) FROM migration_probe", [], |row| row
            .get::<_, usize>(0))
            .unwrap(),
        0
    );
    assert!(
        raw.execute("UPDATE session_events SET turn_id='changed'", [])
            .is_err()
    );
    raw.execute_batch("DROP TRIGGER migration_pause; DROP TABLE migration_probe;")
        .unwrap();
    drop(raw);
    run(|host, driver| {
        let path = &path;
        let fixture = &fixture;
        async move {
            let store = open(&host, path, driver).await?;
            validate_replay(&store, fixture, true).await?;
            host.close().await
        }
    })?;
    validate_rows(&path, &fixture);
    Ok(())
}

#[test]
#[ignore = "explicit million-event release measurement through just session-migration-benchmark"]
fn million_event_migration_benchmark() -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("sessions.sqlite");
    let seeded = Instant::now();
    let fixture = seed(&path, 1_000_000)?;
    let seed_ms = seeded.elapsed().as_millis();
    let before = std::fs::metadata(&path).unwrap().len();
    let stop = Arc::new(AtomicBool::new(false));
    let watching = stop.clone();
    let watched = path.clone();
    let monitor = std::thread::spawn(move || {
        let wal = PathBuf::from(format!("{}-wal", watched.display()));
        let mut peak = 0;
        let mut writer_blocked = false;
        while !watching.load(Ordering::Relaxed) {
            peak = peak.max(std::fs::metadata(&wal).map_or(0, |m| m.len()));
            if peak > 1024 * 1024 && !writer_blocked {
                let raw = Connection::open(&watched).unwrap();
                raw.busy_timeout(Duration::ZERO).unwrap();
                match raw.execute_batch("BEGIN IMMEDIATE; COMMIT;") {
                    Err(error)
                        if error.sqlite_error_code() == Some(rusqlite::ErrorCode::DatabaseBusy) =>
                    {
                        writer_blocked = true
                    }
                    Err(error) => panic!("unexpected competing writer failure: {error}"),
                    Ok(()) => {}
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        (peak, writer_blocked)
    });
    let mut migration_ms = 0;
    let result = run(|host, driver| {
        let path = &path;
        let fixture = &fixture;
        let migration_ms = &mut migration_ms;
        async move {
            let started = Instant::now();
            let store = open(&host, path, driver).await?;
            *migration_ms = started.elapsed().as_millis();
            validate_replay(&store, fixture, false).await?;
            host.close().await
        }
    });
    stop.store(true, Ordering::Relaxed);
    let (peak_wal_bytes, writer_blocked) = monitor.join().unwrap();
    result?;
    validate_rows(&path, &fixture);
    assert!(writer_blocked, "migration excludes competing writers");
    let after = std::fs::metadata(&path).unwrap().len();
    println!(
        "session_migration_benchmark events={} seed_ms={seed_ms} migration_ms={migration_ms} database_before_bytes={before} database_after_bytes={after} peak_wal_bytes={peak_wal_bytes} writer_blocked={writer_blocked}",
        fixture.rows
    );
    Ok(())
}
