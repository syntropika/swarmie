//! Reproducible, opt-in real SQLite scheduling measurements without timing assertions.

use std::{
    rc::Rc,
    time::{Duration, Instant},
};

use futures::{StreamExt, stream};
use swarmie_durable_tasks_fixture::{
    facet,
    hooks::{self, Observer},
    task,
};
use swarmie_task_contracts::*;
use tembra::{core, prelude::*};
use tembra_native::TokioDriver;

const TASKS: usize = 32;

fn percentile(values: &mut [u128], percentile: usize) -> u128 {
    values.sort_unstable();
    values[(values.len() - 1) * percentile / 100]
}

async fn measure(driver: Rc<TokioDriver>, with_observer: bool, sample: usize) -> Result<()> {
    let directory = tempfile::tempdir().unwrap();
    let host = Host::with_driver("lifecycle-performance", "application", driver.clone())?;
    host.install(
        swarmie_task_storage_sqlite::facet(&swarmie_task_storage_sqlite::Configuration {
            database: directory.path().join("tasks.sqlite"),
            outbox: Default::default(),
        })?,
        core::LoadOptions::default(),
    )?
    .await?;
    host.install(facet(1)?, core::LoadOptions::default())?
        .await?;
    let observer = Rc::new(Observer::default());
    if with_observer {
        host.install(
            hooks::facet("performance", 0, observer.clone())?,
            core::LoadOptions::default(),
        )?
        .await?;
    }
    host.install(
        swarmie_durable_tasks::facet(
            &swarmie_durable_tasks::Configuration {
                max_parallel: 16,
                poll_interval_ms: 2,
                ..Default::default()
            },
            driver,
        )?,
        core::LoadOptions::default(),
    )?
    .await?;
    let tasks = host.get(DurableTasks, DurableTasksInput {}).await?;
    let started = Instant::now();
    let measurements = stream::iter(0..TASKS)
        .map(|index| {
            let tasks = tasks.clone();
            async move {
                let id = format!("performance-{index:04}");
                let mut spec = task(&id);
                spec.phase = "finish".into();
                let submitted = Instant::now();
                tasks.create(spec).await?;
                let submission = submitted.elapsed().as_micros();
                let outcome = tasks.wait(TaskLookup { id }).await?;
                assert!(matches!(outcome, TaskOutcome::Completed { .. }));
                Ok::<_, RuntimeError>((submission, submitted.elapsed().as_micros()))
            }
        })
        .buffer_unordered(16)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>>>()?;
    let terminal_us = started.elapsed().as_micros();
    let mut submission: Vec<_> = measurements.iter().map(|entry| entry.0).collect();
    let mut terminal: Vec<_> = measurements.iter().map(|entry| entry.1).collect();
    let task_elapsed = started.elapsed();
    let lifecycle = host.get(TaskLifecycle, TaskLifecycleInput {}).await?;
    let at_terminal = lifecycle.diagnostics().await?;
    assert!(at_terminal.queued <= 256);
    let drain_started = Instant::now();
    let final_diagnostics = loop {
        let diagnostics = lifecycle.diagnostics().await?;
        if diagnostics.queued == 0
            && !diagnostics.active
            && diagnostics.enqueued + diagnostics.skipped + diagnostics.dropped
                == (TASKS * 4) as u64
        {
            break diagnostics;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    };
    assert_eq!(final_diagnostics.failed, 0);
    assert_eq!(final_diagnostics.abandoned, 0);
    assert_eq!(
        final_diagnostics.enqueued + final_diagnostics.skipped + final_diagnostics.dropped,
        (TASKS * 4) as u64
    );
    assert_eq!(final_diagnostics.enqueued, final_diagnostics.completed);
    assert_eq!(
        observer.events.borrow().len() as u64,
        final_diagnostics.completed
    );
    let observer_drain_us = drain_started.elapsed().as_micros();
    let storage = host.get(TaskStorage, TaskStorageInput {}).await?;
    storage
        .register_outbox_consumer(TaskOutboxConsumer {
            id: "performance-audit".into(),
        })
        .await?;
    let page = storage
        .read_outbox(TaskOutboxQuery {
            consumer_id: "performance-audit".into(),
            after: None,
            limit: 256,
        })
        .await?;
    assert_eq!(
        page.events.len(),
        TASKS * 3,
        "every confirmed write has an outbox event; physical settlement has none"
    );
    let outbox_bytes: usize = page
        .events
        .iter()
        .map(|event| serde_json::to_vec(event).unwrap().len())
        .sum();
    println!(
        "lifecycle_performance {}",
        serde_json::json!({
            "mode": if with_observer { "native_observer" } else { "absent_observer" },
            "sample": sample,
            "tasks": TASKS,
            "outbox_events": page.events.len(),
            "outbox_event_bytes": outbox_bytes,
            "terminal_us": terminal_us,
            "tasks_per_second": TASKS as f64 / task_elapsed.as_secs_f64(),
            "submission_p50_us": percentile(&mut submission, 50),
            "submission_p95_us": percentile(&mut submission, 95),
            "terminal_p50_us": percentile(&mut terminal, 50),
            "terminal_p95_us": percentile(&mut terminal, 95),
            "observer_drain_us": observer_drain_us,
            "queued_at_terminal": at_terminal.queued,
            "diagnostics": final_diagnostics,
        })
    );
    host.close().await
}

/// Run separately from other build/test work, with --ignored --nocapture --test-threads=1.
#[test]
#[ignore = "opt-in scheduling benchmark; prints measurements without timing pass/fail thresholds"]
fn lifecycle_scheduling_measurements() {
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(async {
            tokio::time::timeout(Duration::from_secs(120), async {
                for sample in 0..3 {
                    for with_observer in [false, true] {
                        measure(driver.clone(), with_observer, sample).await?;
                    }
                }
                Ok::<_, RuntimeError>(())
            })
            .await
            .expect("performance workload timed out")
            .unwrap();
        }));
}
