//! Real-process interruption and recovery through the public Agent Contract.
use futures::FutureExt;
use std::{path::Path, time::Duration};
use swarmie_agent_contracts::*;
use swarmie_agent_loop_fixture::{hooks, model::Reply, tool::ToolConfiguration};
use swarmie_model_contracts::{GenerateResult, Message, Role};
use swarmie_task_contracts::{
    TaskOutcome, TaskQuery, TaskRecord, TaskStatus, TaskStorage, TaskStorageInput,
};
use tembra::{Host, core};
use tembra_hooks::HookArgs;
pub mod support;
use support::*;

fn process(root: &Path, mode: &str) -> std::process::Child {
    std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "agent_child_process", "--nocapture"])
        .env("SWARMIE_AGENT_FIXTURE_ROOT", root)
        .env("SWARMIE_AGENT_FIXTURE_MODE", mode)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap()
}

fn crash_during_tool(root: &Path, mode: &str) {
    let mut child = process(root, mode);
    let started = std::time::Instant::now();
    let log = root.join("effects.log");
    while !std::fs::read_to_string(&log).is_ok_and(|content| !content.is_empty()) {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("child exited before effect: {status}");
        }
        if started.elapsed() > Duration::from_secs(15) {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!(
                "tool was not reached: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    child
        .kill()
        .expect("kill the real process during an admitted tool operation");
    assert!(!child.wait().unwrap().success());
}

fn complete_process(root: &Path, mode: &str) {
    let output = process(root, mode).wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Control middleware pauses only after the real terminal has physically settled and
/// committed its provider receipt. Observation delivery cannot be used as an execution gate.
fn model_barrier(
    log: Option<std::path::PathBuf>,
    transform: bool,
) -> tembra::Result<tembra::NativeFacet> {
    hooks::contribution(
        wrap_model_call()?,
        "response-control-barrier",
        0,
        move |args: HookArgs<ModelCall, GenerateResult>, _| {
            let log = log.clone();
            async move {
                let (call, next) = args.into_parts();
                let mut result = next
                    .expect("model middleware owns its continuation")
                    .call(call)
                    .await?;
                if transform && let GenerateResult::Response { response } = &mut result {
                    response.message =
                        Message::text(Role::Assistant, "middleware recovered answer");
                }
                if let Some(log) = log {
                    use std::io::Write;
                    let mut file = std::fs::File::create(log).unwrap();
                    writeln!(
                        file,
                        "provider receipt confirmed before effective middleware result"
                    )
                    .unwrap();
                    file.sync_all().unwrap();
                    futures::future::pending::<()>().await;
                }
                Ok(result)
            }
            .boxed_local()
        },
    )
}

#[test]
fn real_process_restart_replays_safe_tool_with_original_operation_identity() {
    let directory = tempfile::tempdir().unwrap();
    crash_during_tool(directory.path(), "safe-crash");
    complete_process(directory.path(), "safe-resume");
    let effects = std::fs::read_to_string(directory.path().join("effects.log")).unwrap();
    let ids = effects.lines().collect::<Vec<_>>();
    assert_eq!(ids.len(), 2);
    assert_eq!(ids[0], ids[1]);
}

#[test]
fn real_process_restart_parks_unsafe_tool_without_repeating_effect() {
    let directory = tempfile::tempdir().unwrap();
    crash_during_tool(directory.path(), "unsafe-crash");
    complete_process(directory.path(), "unsafe-resume");
    let effects = std::fs::read_to_string(directory.path().join("effects.log")).unwrap();
    assert_eq!(effects.lines().count(), 1);
}

#[test]
fn agent_child_process() {
    let Ok(mode) = std::env::var("SWARMIE_AGENT_FIXTURE_MODE") else {
        return;
    };
    let root = std::path::PathBuf::from(std::env::var_os("SWARMIE_AGENT_FIXTURE_ROOT").unwrap());
    run(|driver| async move {
        if mode == "inspect-saved-model" {
            let host = Host::with_driver("checkpoint-inspection", "application", driver)?;
            host.install(
                swarmie_task_storage_sqlite::facet(&swarmie_task_storage_sqlite::Configuration {
                    database: root.join("tasks.sqlite"),
                    outbox: Default::default(),
                    outbox_legacy_owners: Default::default(),
                })?,
                core::LoadOptions::default(),
            )?
            .await?;
            let storage = host.get(TaskStorage, TaskStorageInput {}).await?;
            let page = storage.scan(TaskQuery::default()).await?;
            assert_eq!(page.records.len(), 1);
            let record = &page.records[0];
            assert_eq!(record.status, TaskStatus::Running);
            assert_eq!(record.phase, "model");
            assert_eq!(record.checkpoint["version"], 3);
            assert_eq!(record.checkpoint["conversation_id"], "conversation");
            assert!(record.checkpoint["request_event"].is_string());
            assert!(record.checkpoint["response_event"].is_null());
            std::fs::write(
                root.join("saved-checkpoint.json"),
                serde_json::to_vec(record).unwrap(),
            )
            .unwrap();
            return host.close().await;
        }
        let crashing = mode.ends_with("crash");
        let unsafe_replay = mode.starts_with("unsafe");
        if mode == "replacement-resume" {
            let fixture = open_replacement(driver, &root).await?;
            let completed = fixture.terminal("process-turn").await?;
            let saved: TaskRecord =
                serde_json::from_slice(&std::fs::read(root.join("saved-checkpoint.json")).unwrap())
                    .unwrap();
            assert_eq!(completed.task.id, saved.id);
            assert_eq!(
                completed.task.checkpoint["response_event"],
                saved.checkpoint["response_event"]
            );
            assert!(completed.task.attempts > saved.attempts);
            assert!(
                matches!(&completed.task.outcome, Some(TaskOutcome::Completed { value }) if value["implementation"] == "fixture.replacement-agent-loop")
            );
            assert!(fixture.model.requests.borrow().is_empty());
            assert_eq!(completed.task.phase, "model");
            return fixture.host.close().await;
        }
        let contributions = if mode == "model-transform-resume" {
            vec![model_barrier(None, true)?]
        } else {
            vec![]
        };
        let fixture = if mode == "model-transform-resume" {
            open_with_contributions(
                driver,
                &root,
                vec![Reply::text("must not regenerate")],
                contributions,
            )
            .await?
        } else {
            open(
                driver,
                &root,
                if mode == "model-unknown-crash" {
                    vec![Reply::Hang]
                } else if mode.starts_with("model") {
                    vec![Reply::text("confirmed before process interruption")]
                } else if crashing {
                    vec![Reply::double(21), Reply::text("done")]
                } else {
                    vec![Reply::text("recovered")]
                },
                ToolConfiguration {
                    replay: if unsafe_replay {
                        ReplayPolicy::Unsafe
                    } else {
                        ReplayPolicy::Safe
                    },
                    effect_log: Some(root.join("effects.log")),
                    hang_after_effect: crashing,
                    ..Default::default()
                },
                true,
            )
            .await?
        };
        if mode == "model-unknown-crash" {
            *fixture.model.admission_log.borrow_mut() = Some(root.join("effects.log"));
            fixture.start("process-turn", Default::default()).await?;
            fixture.terminal("process-turn").await?;
            panic!("the parent should kill the process during generation");
        } else if mode == "model-unknown-resume" {
            loop {
                let record = fixture
                    .agent
                    .inspect(Harness::lookup("process-turn"))
                    .await?
                    .unwrap();
                if record.task.status == TaskStatus::Parked {
                    break;
                }
                assert!(
                    !record.task.is_terminal(),
                    "unknown model attempt must remain recoverable: {record:?}"
                );
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
            assert!(fixture.model.requests.borrow().is_empty());
        } else if mode == "model-crash" || mode == "model-transform-crash" {
            fixture
                .host
                .install(
                    model_barrier(
                        Some(root.join("effects.log")),
                        mode == "model-transform-crash",
                    )?,
                    core::LoadOptions::default(),
                )?
                .await?;
            fixture.start("process-turn", Default::default()).await?;
            fixture.terminal("process-turn").await?;
            panic!("the parent should kill the process inside model control middleware");
        } else if mode == "model-resume" || mode == "model-transform-resume" {
            let record = fixture.terminal("process-turn").await?;
            assert!(
                matches!(record.task.outcome, Some(TaskOutcome::Completed { .. })),
                "{record:?}"
            );
            assert!(
                fixture.model.requests.borrow().is_empty(),
                "a committed model response must not be regenerated"
            );
            let saved: TaskRecord =
                serde_json::from_slice(&std::fs::read(root.join("saved-checkpoint.json")).unwrap())
                    .unwrap();
            assert_eq!(record.task.id, saved.id);
            assert_eq!(
                record.task.checkpoint["request_event"],
                saved.checkpoint["request_event"]
            );
            assert!(record.task.attempts > saved.attempts);
            assert!(fixture.tool.calls.borrow().is_empty());
            // Completion keeps the last effect-admission checkpoint. The terminal outcome
            // references the confirmed response produced while that model root was owned.
            let Some(TaskOutcome::Completed { value }) = &record.task.outcome else {
                unreachable!()
            };
            assert!(value["response_event_id"].is_string());
            let events = fixture.events("process-turn").await?;
            assert_eq!(
                events
                    .iter()
                    .filter(|entry| entry.event.kind == "swarmie.agent.model-provider-confirmed")
                    .count(),
                1
            );
            assert_eq!(
                events
                    .iter()
                    .filter(|entry| entry.event.kind == "swarmie.agent.model-confirmed")
                    .count(),
                1
            );
            if mode == "model-transform-resume" {
                assert!(events.iter().any(|entry| {
                    entry.event.kind == swarmie_session_contracts::MESSAGE
                        && entry
                            .event
                            .payload
                            .to_string()
                            .contains("middleware recovered answer")
                }));
            }
        } else if crashing {
            fixture.start("process-turn", Default::default()).await?;
            fixture.terminal("process-turn").await?;
            panic!("the parent should kill the process inside the tool");
        } else if unsafe_replay {
            loop {
                let record = fixture
                    .agent
                    .inspect(Harness::lookup("process-turn"))
                    .await?
                    .unwrap();
                if record.task.status == TaskStatus::Parked {
                    break;
                }
                assert!(
                    !record.task.is_terminal(),
                    "unsafe uncertainty must remain recoverable: {record:?}"
                );
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
            assert!(fixture.tool.calls.borrow().is_empty());
            assert!(fixture.model.requests.borrow().is_empty());
        } else {
            let record = fixture.terminal("process-turn").await?;
            assert!(
                matches!(record.task.outcome, Some(TaskOutcome::Completed { .. })),
                "{record:?}"
            );
            assert_eq!(fixture.tool.calls.borrow().len(), 1);
            assert_eq!(fixture.model.requests.borrow().len(), 1);
            assert!(
                !record
                    .task
                    .checkpoint
                    .to_string()
                    .contains("Compute the result.")
            );
        }
        fixture.host.close().await
    });
}

#[test]
fn real_process_restart_reuses_confirmed_model_response_from_saved_checkpoint() {
    let directory = tempfile::tempdir().unwrap();
    crash_during_tool(directory.path(), "model-crash");
    complete_process(directory.path(), "inspect-saved-model");
    complete_process(directory.path(), "model-resume");
}

#[test]
fn real_process_restart_preserves_unknown_model_attempt_without_resubmitting() {
    let directory = tempfile::tempdir().unwrap();
    crash_during_tool(directory.path(), "model-unknown-crash");
    complete_process(directory.path(), "model-unknown-resume");
}

#[test]
fn compatible_native_implementation_replacement_resumes_confirmed_finalization_checkpoint() {
    let directory = tempfile::tempdir().unwrap();
    crash_during_tool(directory.path(), "model-crash");
    complete_process(directory.path(), "inspect-saved-model");
    complete_process(directory.path(), "replacement-resume");
}

#[test]
fn real_process_restart_replays_middleware_after_provider_confirmation_without_repeating_model() {
    let directory = tempfile::tempdir().unwrap();
    crash_during_tool(directory.path(), "model-transform-crash");
    complete_process(directory.path(), "inspect-saved-model");
    complete_process(directory.path(), "model-transform-resume");
}
