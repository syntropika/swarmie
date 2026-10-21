//! Bounded load against one actual Host, scheduler and selected durable stores.

use std::{cell::RefCell, collections::BTreeMap, path::Path, rc::Rc, time::Instant};

use futures::{FutureExt, StreamExt, future::LocalBoxFuture, stream::FuturesUnordered};
use serde_json::{Value, json};
use swarmie_agent_contracts::StartTurn;
use swarmie_agent_loop_fixture::model::Reply;
use swarmie_task_contracts::{TaskLookup, TaskOutcome, TaskQuery};
use tembra::{Result, RuntimeError};

use super::{
    bounded, emit, error_json, host,
    metrics::{self, Driver, Snapshot},
    options::{Load, Options},
    validate, write_json,
};

struct Turn {
    request: StartTurn,
    record: Value,
    terminal: Option<(TaskLookup, TaskOutcome)>,
}

type Turns = Rc<RefCell<Vec<Turn>>>;

pub fn run(
    options: &Options,
    output: &Path,
    replies: Vec<Reply>,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let driver = Driver::new();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(
        driver
            .inner
            .run_until(measure(options, output, &driver, replies)),
    )?;
    write_json(&output.join("summary.json"), &result)?;
    emit(
        json!({"event":"load_result","schema_version":1,"valid":result["valid"],"statistics":result["statistics"]}),
    )?;
    if result["valid"] != true {
        return Err(std::io::Error::other(
            "invalid load run; retained summary includes failures and censored turns",
        )
        .into());
    }
    Ok(())
}

async fn measure(
    options: &Options,
    output: &Path,
    driver: &Rc<Driver>,
    replies: Vec<Reply>,
) -> Result<Value> {
    let load = options.load.as_ref().expect("load mode selected");
    let origin = Instant::now();
    let setup = Snapshot::take(driver, origin);
    let scripts = (0..load.turns + load.warmup)
        .map(|i| (format!("scripted-{i}"), replies.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut result = json!({"schema_version":1,"valid":false,"mode":if load.interval.is_some(){"scheduled_arrivals"}else{"closed_loop"},
        "configuration":{"turns":load.turns,"warmup_turns":load.warmup,"client_outstanding_cap":load.concurrency,
            "arrival_interval_ns":load.interval.map(|d|d.as_nanos()),"scheduler_max_parallel":load.max_parallel,
            "scheduler_poll_interval_ms":2,"phase_cap_secs":options.timeout.as_secs()},
        "phases":{},"error":null,"turns":[],"warmup":[],"observations":[],
        "scope":{"hosts":1,"task_stores":1,"session_stores":1,"bindings_stores":1,
            "driver":"Tokio current-thread LocalSet; async concurrency, no CPU-parallel agent execution",
            "observer":"DurableTasks.wait; observed terminal time includes observer scheduling delay",
            "outstanding":"generator calls started but not yet observed terminal/error; includes admission, not scheduler in-flight",
            "scheduler_queue_depth":null,"scheduler_in_flight":null,"persistence_lock_wait_ns":null,
            "commit_time_ns":null,"unavailable_metrics":"No internal scheduler/SQLite instrumentation; no inferred commit, lock or queue counts",
            "cpu":"Linux CLOCK_THREAD_CPUTIME_ID runtime thread and CLOCK_PROCESS_CPUTIME_ID whole process; null elsewhere",
            "driver_counts":"Host-wide Driver requests, not Future polls, commits or timer completions",
            "percentiles":"nearest-rank empirical completed observations only; no confidence bound or SLO; failures/censoring retained separately",
            "scheduled_arrivals":"planned times are independent of completion; actual offer processing may lag on the same runtime thread; cap-full arrivals shed without admission",
            "closed_loop":"completion-paced, subject to coordinated omission; no external offered-rate capacity claim"}});
    emit(json!({"event":"load_phase","phase":"setup","boundary":"begin"}))?;
    let h = match bounded(
        options.timeout,
        host::open_routed(driver.clone(), output, scripts, load.max_parallel),
    )
    .await
    {
        Ok(h) => h,
        Err(error) => {
            result["error"] = error_json("setup", &error);
            result["phases"]["setup"] = setup.window(&Snapshot::take(driver, origin));
            return Ok(result);
        }
    };
    result["store_identity"] = json!({"tasks":h.storage.outbox_status().await?.store_id,
        "sessions":h.sessions.publication_status().await?.durable.store_id,
        "bindings":"private binding route state; one selected bindings.sqlite Resource"});
    let turns: Turns = Rc::new(RefCell::new((0..load.turns + load.warmup).map(|i| {
        let mut request = h.request.clone();
        request.turn_id = format!("turn-{i}");
        request.session_id = format!("session-{i}");
        request.input_event_id = format!("input-{i}");
        request.model.model = format!("scripted-{i}");
        Turn { request, record: json!({"index":i,"state":"not_offered","scheduled_ns":null,"offered_ns":null,
            "admission_begin_ns":null,"admitted_ns":null,"observed_terminal_ns":null,"task_id":null,"error":null}), terminal:None }
    }).collect()));
    let prepared = bounded(options.timeout, async {
        let requests = turns
            .borrow()
            .iter()
            .map(|t| t.request.clone())
            .collect::<Vec<_>>();
        for request in requests {
            host::prepare(&h, &request).await?;
        }
        Ok(())
    })
    .await;
    result["phases"]["setup"] = setup.window(&Snapshot::take(driver, origin));
    emit(json!({"event":"load_phase","phase":"setup","boundary":"end"}))?;
    if let Err(error) = prepared {
        result["error"] = error_json("setup", &error);
    }
    if result["error"].is_null() {
        let warmup = Snapshot::take(driver, origin);
        let warmed = bounded(options.timeout, async {
            for i in 0..load.warmup {
                attempt(&h, turns.clone(), i, Instant::now()).await;
            }
            validate::ensure(
                turns.borrow()[..load.warmup]
                    .iter()
                    .all(|t| t.record["state"] == "completed"),
                "warmup failed",
            )
        })
        .await;
        result["phases"]["warmup"] = warmup.window(&Snapshot::take(driver, origin));
        if let Err(error) = warmed {
            result["error"] = error_json("warmup", &error);
        }
    }
    let observations = Rc::new(RefCell::new(Vec::<Value>::new()));
    if result["error"].is_null() {
        emit(json!({"event":"load_phase","phase":"execution","boundary":"begin"}))?;
        metrics::swarmie_diagnostic_execution_begin();
        let before = Snapshot::take(driver, origin);
        let measured_origin = Instant::now();
        let measured = bounded(
            options.timeout,
            offer(
                &h,
                turns.clone(),
                load,
                measured_origin,
                observations.clone(),
            ),
        )
        .await;
        let after = Snapshot::take(driver, origin);
        metrics::swarmie_diagnostic_execution_end();
        result["phases"]["execution"] = before.window(&after);
        result["statistics"] = statistics(
            &turns.borrow()[load.warmup..],
            before.window(&after)["wall_ns"].as_u64().unwrap_or(0),
        );
        emit(json!({"event":"load_phase","phase":"execution","boundary":"end"}))?;
        if let Err(error) = measured {
            result["error"] = error_json("execution", &error);
        }
    }
    // All publication and strong semantic checks are outside the measured window.
    let before = Snapshot::take(driver, origin);
    match bounded(options.timeout, validate::drain(&h)).await {
        Ok(publication) => result["publication"] = publication,
        Err(error) => record_error(&mut result, "publication_drain", &error),
    }
    result["phases"]["publication_drain"] = before.window(&Snapshot::take(driver, origin));
    let before = Snapshot::take(driver, origin);
    let checked = bounded(options.timeout, async {
        let completed = turns
            .borrow()
            .iter()
            .enumerate()
            .filter_map(|(i, t)| {
                t.terminal
                    .clone()
                    .map(|(key, outcome)| (i, t.request.clone(), key, outcome))
            })
            .collect::<Vec<_>>();
        for (i, request, key, outcome) in completed {
            match validate::turn(&h, &request, key, &outcome).await {
                Ok(invariants) => turns.borrow_mut()[i].record["invariants"] = invariants,
                Err(error) => {
                    turns.borrow_mut()[i].record["validation_error"] =
                        error_json("validation", &error)
                }
            }
        }
        // Read the complete stable terminal graph rather than silently checking only its first page.
        let mut query = TaskQuery::default();
        let mut count = 0;
        loop {
            let page = h.tasks.graph(query.clone()).await?;
            count += page.records.len();
            match page.next {
                Some(cursor) => query.cursor = Some(cursor),
                None => break,
            }
        }
        let admitted = turns
            .borrow()
            .iter()
            .filter(|t| !t.record["task_id"].is_null())
            .count();
        validate::ensure(
            count == admitted,
            "shared task graph count differs from admitted turns",
        )?;
        validate::ensure(
            h.model.requests.borrow().len() == 2 * admitted
                && h.tool.calls.borrow().len() == admitted
                && h.policy.calls.borrow().len() == admitted,
            "aggregate provider counts differ",
        )?;
        Ok(())
    })
    .await;
    if let Err(error) = checked {
        record_error(&mut result, "validation", &error);
    }
    result["phases"]["validation"] = before.window(&Snapshot::take(driver, origin));
    let before = Snapshot::take(driver, origin);
    if let Err(error) = bounded(options.timeout, h.host.close()).await {
        record_error(&mut result, "close", &error);
    }
    result["phases"]["close"] = before.window(&Snapshot::take(driver, origin));
    result["background_errors"] = json!(
        h.host
            .core()
            .errors()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    );
    result["lifecycle"] = json!({"backend_closes":h.model.backend_closes.get(),"generation_drops":h.model.dropped.get(),
        "premature_generation_drops":h.model.premature_drop.get(),"close_publication_failures":h.model.close_publication_failures.get(),"provider_cancel_calls":h.model.cancel_calls.get()});
    for turn in turns.borrow_mut().iter_mut() {
        if matches!(
            turn.record["state"].as_str(),
            Some("offered" | "admitting" | "admitted")
        ) {
            turn.record["state"] = json!("censored");
        }
    }
    let turns = turns.borrow();
    result["valid"] = json!(
        result["error"].is_null()
            && result["background_errors"]
                .as_array()
                .is_some_and(Vec::is_empty)
            && turns.iter().all(|t| t.record["state"] == "load_shed"
                || (t.record["state"] == "completed"
                    && t.record["validation_error"].is_null()
                    && t.record["invariants"]["exact_outcome"] == true))
            && h.model.premature_drop.get() == 0
            && h.model.close_publication_failures.get() == 0
    );
    result["warmup"] = json!(
        turns[..load.warmup]
            .iter()
            .map(|t| &t.record)
            .collect::<Vec<_>>()
    );
    result["turns"] = json!(
        turns[load.warmup..]
            .iter()
            .map(|t| &t.record)
            .collect::<Vec<_>>()
    );
    result["observations"] = json!(*observations.borrow());
    Ok(result)
}

fn record_error(result: &mut Value, phase: &str, error: &RuntimeError) {
    result[format!("{phase}_error")] = error_json(phase, error);
    if result["error"].is_null() {
        result["error"] = error_json(phase, error);
    }
}

async fn attempt(h: &host::Harness, turns: Turns, index: usize, origin: Instant) {
    let request = turns.borrow()[index].request.clone();
    {
        let mut turns = turns.borrow_mut();
        let record = &mut turns[index].record;
        if record["offered_ns"].is_null() {
            record["offered_ns"] = json!(origin.elapsed().as_nanos());
        }
        record["state"] = json!("admitting");
        record["admission_begin_ns"] = json!(origin.elapsed().as_nanos());
    }
    let submitted = match h.agent.start(request).await {
        Ok(receipt) => receipt,
        Err(error) => {
            let mut turns = turns.borrow_mut();
            turns[index].record["state"] = json!("admission_error");
            turns[index].record["error"] = error_json("admission", &error);
            return;
        }
    };
    let key = TaskLookup {
        id: submitted.task.id,
    };
    {
        let mut turns = turns.borrow_mut();
        turns[index].record["admitted_ns"] = json!(origin.elapsed().as_nanos());
        turns[index].record["task_id"] = json!(key.id);
        turns[index].record["state"] = json!("admitted");
    }
    let outcome = h.tasks.wait(key.clone()).await;
    let observed = origin.elapsed().as_nanos();
    let mut turns = turns.borrow_mut();
    let turn = &mut turns[index];
    match outcome {
        Ok(outcome) => {
            turn.record["observed_terminal_ns"] = json!(observed);
            turn.record["state"] = json!(if matches!(outcome, TaskOutcome::Completed { .. }) {
                "completed"
            } else {
                "terminal_unsuccessful"
            });
            turn.record["outcome"] = json!(outcome);
            turn.terminal = Some((key, outcome));
        }
        Err(error) => {
            turn.record["state"] = json!("observation_error");
            turn.record["error"] = error_json("wait", &error);
        }
    }
}

async fn offer(
    h: &host::Harness,
    turns: Turns,
    load: &Load,
    origin: Instant,
    observations: Rc<RefCell<Vec<Value>>>,
) -> Result<()> {
    let mut jobs: FuturesUnordered<LocalBoxFuture<'_, ()>> = FuturesUnordered::new();
    let mut next = 0;
    while next < load.turns || !jobs.is_empty() {
        if next < load.turns {
            let scheduled = load.interval.map(|interval| interval * next as u32);
            let arrival = async {
                if let Some(due) = scheduled {
                    tokio::time::sleep_until((origin + due).into()).await;
                }
            };
            if load.interval.is_some() || jobs.len() < load.concurrency {
                tokio::select! {
                    biased;
                    Some(()) = jobs.next(), if !jobs.is_empty() => {
                        observations.borrow_mut().push(json!({"elapsed_ns":origin.elapsed().as_nanos(),"outstanding_calls":jobs.len(),"event":"settled"}));
                    }
                    () = arrival => {
                        let index = load.warmup + next;
                        {
                            let mut turns = turns.borrow_mut();
                            let record = &mut turns[index].record;
                            record["scheduled_ns"] = json!(scheduled.map(|d|d.as_nanos()));
                            record["offered_ns"] = json!(origin.elapsed().as_nanos());
                            record["state"] = json!(if jobs.len() < load.concurrency {"offered"}else{"load_shed"});
                        }
                        if jobs.len() < load.concurrency { jobs.push(attempt(h, turns.clone(), index, origin).boxed_local()); }
                        observations.borrow_mut().push(json!({"elapsed_ns":origin.elapsed().as_nanos(),"outstanding_calls":jobs.len(),"event":"offer","index":index}));
                        next += 1;
                    }
                }
                continue;
            }
        }
        if jobs.next().await.is_some() {
            observations.borrow_mut().push(json!({"elapsed_ns":origin.elapsed().as_nanos(),"outstanding_calls":jobs.len(),"event":"settled"}));
        }
    }
    Ok(())
}

fn statistics(turns: &[Turn], elapsed_ns: u64) -> Value {
    let values = |start: &str| {
        turns
            .iter()
            .filter(|t| t.record["state"] == "completed")
            .filter_map(|t| {
                Some(
                    t.record["observed_terminal_ns"]
                        .as_u64()?
                        .saturating_sub(t.record[start].as_u64()?),
                )
            })
            .collect::<Vec<_>>()
    };
    let offers = turns
        .iter()
        .filter_map(|t| t.record["offered_ns"].as_u64())
        .collect::<Vec<_>>();
    let offer_span = offers
        .first()
        .zip(offers.last())
        .map(|(first, last)| last - first);
    let offered = turns
        .iter()
        .filter(|t| !t.record["offered_ns"].is_null())
        .count();
    let completed = turns
        .iter()
        .filter(|t| t.record["state"] == "completed")
        .count();
    json!({"planned":turns.len(),"offered":offered,"completed":completed,
        "admitted":turns.iter().filter(|t|!t.record["task_id"].is_null()).count(),
        "load_shed":turns.iter().filter(|t|t.record["state"] == "load_shed").count(),
        "terminal_unsuccessful":turns.iter().filter(|t|t.record["state"] == "terminal_unsuccessful").count(),
        "errors":turns.iter().filter(|t|!t.record["error"].is_null()).count(),
        "aborted":turns.iter().filter(|t| t.record["outcome"]["type"] == "aborted").count(),
        "not_completed":turns.len()-completed,"window_ns":elapsed_ns,
        "offer_span_ns":offer_span,
        "inter_offer_rate_per_second":offer_span.filter(|span| *span > 0).map(|span| (offered.saturating_sub(1)) as f64 * 1e9 / span as f64),
        "censored":turns.iter().filter(|t| matches!(t.record["state"].as_str(),Some("admitting"|"admitted"|"offered"|"censored"))).count(),
        "rates_scope":"offered_per_second and completed_per_second use the full admission-to-last-observed window; inter_offer_rate uses only first-to-last actual offer processing",
        "offered_per_second":offered as f64 * 1e9 / elapsed_ns.max(1) as f64,
        "completed_per_second":completed as f64 * 1e9 / elapsed_ns.max(1) as f64,
        "offered_to_observed_ns":percentiles(values("offered_ns")),
        "scheduled_to_observed_ns":percentiles(values("scheduled_ns")),
        "admitted_to_observed_ns":percentiles(values("admitted_ns"))})
}

fn percentiles(mut samples: Vec<u64>) -> Value {
    samples.sort_unstable();
    let rank = |p: usize| {
        samples
            .get((samples.len() * p).div_ceil(100).saturating_sub(1))
            .copied()
    };
    json!({"samples":samples.len(),"p50":rank(50),"p95":rank(95),"p99":rank(99),"max":samples.last(),
        "p95_has_at_least_20_samples":samples.len()>=20,"p99_has_at_least_100_samples":samples.len()>=100})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empirical_percentiles_keep_outliers_and_empty_samples_are_null() {
        assert_eq!(percentiles(vec![])["p99"], Value::Null);
        let value = percentiles(vec![1, 2, 3, 4, 999]);
        assert_eq!(value["p50"], 3);
        assert_eq!(value["p99"], 999);
        assert_eq!(value["p99_has_at_least_100_samples"], false);
    }

    #[test]
    fn cancelling_one_shared_turn_does_not_cancel_its_peer() {
        let root = tempfile::tempdir().unwrap();
        let driver = Driver::new();
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(
                driver
                    .inner
                    .run_until(bounded(std::time::Duration::from_secs(30), async {
                        let h = host::open_routed(
                            driver.clone(),
                            root.path(),
                            BTreeMap::from([
                                ("pending".into(), vec![Reply::Hang]),
                                ("peer".into(), vec![Reply::double(21), Reply::text("42")]),
                            ]),
                            16,
                        )
                        .await?;
                        let mut pending = h.request.clone();
                        pending.model.model = "pending".into();
                        host::prepare(&h, &pending).await?;
                        let mut peer = pending.clone();
                        peer.turn_id = "peer".into();
                        peer.session_id = "session-peer".into();
                        peer.input_event_id = "input-peer".into();
                        peer.model.model = "peer".into();
                        host::prepare(&h, &peer).await?;
                        let pending_task = h.agent.start(pending.clone()).await?;
                        let peer_task = h.agent.start(peer.clone()).await?;
                        while !h
                            .model
                            .requests
                            .borrow()
                            .iter()
                            .any(|r| r.model.model == "pending")
                        {
                            tokio::time::sleep(std::time::Duration::from_millis(2)).await;
                        }
                        h.agent
                            .cancel(swarmie_agent_contracts::TurnLookup {
                                session_id: pending.session_id,
                                conversation_id: pending.conversation_id,
                                turn_id: pending.turn_id,
                            })
                            .await?;
                        let key = TaskLookup {
                            id: pending_task.task.id,
                        };
                        assert!(matches!(
                            h.tasks.wait(key.clone()).await?,
                            TaskOutcome::Aborted { .. }
                        ));
                        let record = h.storage.get(key).await?.unwrap();
                        assert!(record.cancellation_requested);
                        let peer_key = TaskLookup {
                            id: peer_task.task.id,
                        };
                        let outcome = h.tasks.wait(peer_key.clone()).await?;
                        validate::drain(&h).await?;
                        let checked = validate::turn(&h, &peer, peer_key, &outcome).await?;
                        assert_eq!(checked["exact_outcome"], true);
                        assert_eq!(h.tool.calls.borrow().len(), 1);
                        h.host.close().await?;
                        assert_eq!(h.model.premature_drop.get(), 0);
                        Ok(())
                    })),
            )
            .unwrap();
    }
}
