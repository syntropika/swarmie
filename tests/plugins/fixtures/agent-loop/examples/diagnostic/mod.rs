mod host;
mod load;
mod metrics;
mod options;
mod validate;

use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use swarmie_agent_loop_fixture::model::Reply;
use swarmie_task_contracts::{TaskLookup, TaskOutcome};
use tembra::{Result, RuntimeError, core};

use metrics::{Driver, Snapshot};
use options::{Observer, Options};

pub fn run(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    run_replies(arguments, vec![Reply::double(21), Reply::text("42")])
}

/// Internal synthetic-workload seam. The executable always selects the standard tool turn.
/// Tests can select a pending provider to exercise real timeout recording without speed assumptions.
pub fn run_replies(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
    replies: Vec<Reply>,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let Some(options) = Options::parse(arguments)? else {
        println!("{}", options::HELP);
        return Ok(());
    };
    let output = create_output(&options.output)?;
    if options.load.is_some() {
        return load::run(&options, &output, replies);
    }
    let origin = Instant::now();
    let mut runs = Vec::new();
    for run_index in 1..=options.repetitions {
        let directory = output.join(format!("run-{run_index:04}"));
        std::fs::create_dir(&directory)?;
        let driver = Driver::new();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let run = runtime.block_on(driver.inner.run_until(measure(
            &options,
            &directory,
            run_index,
            origin,
            &driver,
            replies.clone(),
        )))?;
        write_json(&directory.join("result.json"), &run)?;
        emit(json!({"event":"result","schema_version":1,"result":run}))?;
        let valid = run["valid"] == true;
        runs.push(run);
        if !valid {
            break;
        }
    }
    let valid =
        runs.len() == options.repetitions as usize && runs.iter().all(|run| run["valid"] == true);
    let summary = json!({
        "schema_version":1, "valid":valid, "observer":options.observer.name(),
        "requested_repetitions":options.repetitions, "diagnostic_timeout_secs":options.timeout.as_secs(),
        "scheduler_poll_interval_ms":2, "store_observer_interval_ms":250,
        "diagnostic_not_ci_acceptance":true,
        "thread_cpu_clock":if cfg!(target_os="linux") {Some("CLOCK_THREAD_CPUTIME_ID")} else {None},
        "counter_scope":"all Driver requests in this Host, without caller attribution; not timer completions or Future polls",
        "exact_terminal_commit_time":null,
        "runs":runs,
    });
    write_json(&output.join("summary.json"), &summary)?;
    emit(json!({"event":"summary","schema_version":1,"summary":summary}))?;
    if !valid {
        return Err(
            io::Error::other("invalid run; inspect retained result.json and summary.json").into(),
        );
    }
    Ok(())
}

/// Creates only a fresh child in an existing external parent; never overwrites retained results.
fn create_output(output: &Path) -> io::Result<PathBuf> {
    let name = output
        .file_name()
        .ok_or_else(|| io::Error::other("output requires a fresh directory name"))?;
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let resolved = parent.canonicalize()?.join(name);
    let checkout = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(4)
        .ok_or_else(|| io::Error::other("fixture is outside its workspace layout"))?
        .canonicalize()?;
    if resolved.starts_with(checkout) {
        return Err(io::Error::other(
            "diagnostic output must be outside the source checkout",
        ));
    }
    std::fs::create_dir(&resolved)?;
    Ok(resolved)
}

fn write_json(path: &Path, value: &Value) -> io::Result<()> {
    // The root and each run directory are fresh. create_new also refuses accidental reuse.
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    writeln!(file)?;
    file.flush()
}

fn emit(value: Value) -> Result<()> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, &value).map_err(io_error)?;
    writeln!(stdout)
        .and_then(|()| stdout.flush())
        .map_err(io_error)
}

fn io_error(error: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::invalid("diagnostic_io", error.to_string())
}

fn phase(run_index: u32, phase: &str, boundary: &str, origin: Instant) -> Result<()> {
    emit(
        json!({"event":"phase","schema_version":1,"run_index":run_index,"phase":phase,
        "boundary":boundary,"process_elapsed_ns":origin.elapsed().as_nanos(),
        "runtime_thread_id":metrics::thread_id()}),
    )
}

async fn bounded<T>(
    timeout: Duration,
    future: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    tokio::time::timeout(timeout, future).await.map_err(|_| {
        RuntimeError::unavailable(
            "diagnostic_timeout",
            "cooperative diagnostic phase cap expired; this is not an acceptance deadline",
        )
    })?
}

async fn measure(
    options: &Options,
    directory: &Path,
    run_index: u32,
    origin: Instant,
    driver: &Rc<Driver>,
    replies: Vec<Reply>,
) -> Result<Value> {
    let mut result = json!({"schema_version":1,"run_index":run_index,"observer":options.observer.name(),
        "valid":false,"window":null,"phases":{},"invariants":null,"error":null});
    phase(run_index, "setup", "begin", origin)?;
    let setup_start = Snapshot::take(driver, origin);
    let harness = bounded(
        options.timeout,
        host::open(driver.clone(), directory, replies),
    )
    .await;
    result["phases"]["setup"] = setup_start.window(&Snapshot::take(driver, origin));
    phase(run_index, "setup", "end", origin)?;
    let h = match harness {
        Ok(harness) => harness,
        Err(error) => {
            result["error"] = error_json("setup", &error);
            return Ok(result);
        }
    };

    phase(run_index, "execution", "begin", origin)?;
    metrics::swarmie_diagnostic_execution_begin();
    let execution_start = Snapshot::take(driver, origin);
    let mut trace = json!({"reads":if options.observer == Observer::Wait {Value::Null} else {json!(0)},
        "read_bounds":[],"requested_wake_delays_ns":[],"observer_begin_ns":null,
        "exact_commit_time_ns":null,"interval_is_maximum_lag":false});
    let observed = bounded(
        options.timeout,
        observe(&h, options.observer, driver, &execution_start, &mut trace),
    )
    .await;
    let execution_end = Snapshot::take(driver, origin);
    metrics::swarmie_diagnostic_execution_end();
    result["window"] = execution_start.window(&execution_end);
    result["observer_details"] = trace;
    phase(run_index, "execution", "end", origin)?;

    match observed {
        Ok((key, outcome)) => {
            phase(run_index, "validation", "begin", origin)?;
            let validation_start = Snapshot::take(driver, origin);
            let validated = bounded(options.timeout, validate::validate(&h, key, &outcome)).await;
            result["phases"]["validation"] =
                validation_start.window(&Snapshot::take(driver, origin));
            phase(run_index, "validation", "end", origin)?;
            match validated {
                Ok(invariants) => result["invariants"] = invariants,
                Err(error) => result["error"] = error_json("validation", &error),
            }
        }
        Err(error) => result["error"] = error_json("execution", &error),
    }
    result["persistence"] = h.persistence()?;
    phase(run_index, "close", "begin", origin)?;
    let close_start = Snapshot::take(driver, origin);
    let closed = bounded(options.timeout, h.host.close()).await;
    result["phases"]["close"] = close_start.window(&Snapshot::take(driver, origin));
    phase(run_index, "close", "end", origin)?;
    if let Err(error) = closed {
        result["close_error"] = error_json("close", &error);
        if result["error"].is_null() {
            result["error"] = result["close_error"].clone();
        }
    }
    result["valid"] = json!(result["error"].is_null());
    Ok(result)
}

fn error_json(phase: &str, error: &RuntimeError) -> Value {
    json!({"phase":phase,"reason":error.reason,"message":error.to_string()})
}

async fn observe(
    h: &host::Harness,
    observer: Observer,
    driver: &Driver,
    start: &Snapshot,
    trace: &mut Value,
) -> Result<(TaskLookup, TaskOutcome)> {
    let submitted = h.agent.start(h.request.clone()).await?;
    let inspected = h
        .agent
        .inspect(h.lookup())
        .await?
        .ok_or_else(|| RuntimeError::invalid("diagnostic_missing", "Agent turn absent"))?;
    validate::ensure(
        inspected.task.id == submitted.task.id,
        "start/inspect task identity mismatch",
    )?;
    let key = TaskLookup {
        id: submitted.task.id,
    };
    trace["observer_begin_ns"] = json!(start.elapsed_ns());
    let outcome = match observer {
        Observer::Wait => h.tasks.wait(key.clone()).await?,
        Observer::Store => loop {
            let begin = start.elapsed_ns();
            let record =
                h.storage.get(key.clone()).await?.ok_or_else(|| {
                    RuntimeError::invalid("diagnostic_missing", "source task absent")
                })?;
            let end = start.elapsed_ns();
            let reads = trace["reads"].as_u64().unwrap_or(0) + 1;
            trace["reads"] = json!(reads);
            trace["read_bounds"].as_array_mut().expect("diagnostic read array").push(json!({"begin_ns":begin,"end_ns":end,"revision":record.revision,"terminal":record.is_terminal()}));
            if record.is_terminal() {
                break record.outcome.ok_or_else(|| {
                    RuntimeError::invalid("diagnostic_outcome", "terminal task lacks outcome")
                })?;
            }
            let interval = Duration::from_millis(250);
            let due = Instant::now() + interval;
            core::Driver::sleep(driver, interval).await;
            trace["requested_wake_delays_ns"]
                .as_array_mut()
                .expect("diagnostic delay array")
                .push(json!(
                    Instant::now().saturating_duration_since(due).as_nanos()
                ));
        },
    };
    Ok((key, outcome))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cooperative_cap_returns_explicit_error_for_a_pending_operation() {
        let error = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(bounded::<()>(
                Duration::from_millis(1),
                futures::future::pending(),
            ))
            .unwrap_err();
        assert_eq!(error.reason, "diagnostic_timeout");
        assert_eq!(error_json("execution", &error)["phase"], "execution");
    }

    #[test]
    fn existing_or_checkout_output_is_refused_without_overwrite() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("sentinel"), "preserved").unwrap();
        assert!(create_output(root.path()).is_err());
        assert_eq!(
            std::fs::read_to_string(root.path().join("sentinel")).unwrap(),
            "preserved"
        );
        let checkout_output = Path::new(env!("CARGO_MANIFEST_DIR")).join("must-not-be-created");
        assert!(create_output(&checkout_output).is_err());
        assert!(!checkout_output.exists());
    }
}
