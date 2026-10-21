//! Test-only resumable publication workflow using actual Tembra operations and Registries.

use std::{fs::OpenOptions, io::Write};

use swarmie_task_contracts::{
    TaskDefinition, TaskInvocation, TaskSpec, TaskTransition, WaitPolicy, task_definitions,
};
use tembra::{core, prelude::*};

pub mod adversarial;
pub mod concurrency;
pub mod gate;
pub mod volatile_storage;

/// A fixture-owned Contract with the canonical task handler method schema.
#[tembra::contract(local, name = "swarmie.acceptance-durable-workflow", version = 1)]
pub trait DurableWorkflow {
    #[portable]
    async fn step(&self, invocation: TaskInvocation) -> Result<TaskTransition>;
}

/// Creates a workflow at the first resumable phase.
pub fn task(id: &str) -> TaskSpec {
    TaskSpec {
        id: id.into(),
        kind: "fixture.workflow".into(),
        definition_version: 1,
        input: serde_json::json!({}),
        checkpoint: serde_json::json!({}),
        phase: "prepare".into(),
    }
}

/// Lifecycle-owned handler registration; version changes preserve the same canonical method.
pub fn facet(version: u32) -> Result<NativeFacet> {
    NativeFacet::new("fixture.durable-workflow", vec![], move |_, setup| {
        setup.serve_with_options(
            DurableWorkflow,
            move |_, _, declaration| {
                declaration.core_mut().register_method(
                    &task_definitions()?,
                    "fixture.workflow",
                    serde_json::to_value(TaskDefinition {
                        kind: "fixture.workflow".into(),
                        version,
                    })
                    .map_err(|error| {
                        RuntimeError::invalid("fixture_metadata", error.to_string())
                    })?,
                    "step",
                )?;
                Ok(Workflow { version })
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )
    })
}

struct Workflow {
    version: u32,
}

impl DurableWorkflowProvider for Workflow {
    async fn step(&self, invocation: TaskInvocation, _context: Context) -> Result<TaskTransition> {
        let task = match invocation {
            TaskInvocation::Migrate { task, .. } => {
                return if task.input["reject_migration"] == true {
                    Ok(TaskTransition::RejectMigration {
                        reason: "unsupported checkpoint".into(),
                    })
                } else {
                    Ok(TaskTransition::Migrate {
                        checkpoint: task.checkpoint,
                        phase: task.phase,
                    })
                };
            }
            TaskInvocation::Run { task, .. } => task,
        };
        match task.phase.as_str() {
            "prepare" => Ok(TaskTransition::Checkpoint {
                checkpoint: serde_json::json!({"operation_id":format!("{}:publish",task.id)}),
                phase: "effect".into(),
            }),
            "effect" => {
                if let Some(path) = task.input["effect_log"].as_str() {
                    let mut file = OpenOptions::new()
                        .append(true)
                        .create(true)
                        .open(path)
                        .map_err(|error| {
                            RuntimeError::invalid("fixture_effect", error.to_string())
                        })?;
                    writeln!(
                        file,
                        "{}",
                        task.checkpoint["operation_id"]
                            .as_str()
                            .unwrap_or("missing")
                    )
                    .and_then(|()| file.sync_all())
                    .map_err(|error| RuntimeError::invalid("fixture_effect", error.to_string()))?;
                    if std::env::var_os("SWARMIE_TASK_FIXTURE_EXIT_AFTER_EFFECT").is_some() {
                        std::process::exit(42);
                    }
                    if std::env::var_os("SWARMIE_TASK_FIXTURE_HANG_AFTER_EFFECT").is_some() {
                        futures::future::pending::<()>().await;
                    }
                }
                Ok(TaskTransition::Checkpoint {
                    checkpoint: task.checkpoint,
                    phase: "finish".into(),
                })
            }
            "finish" => Ok(TaskTransition::Complete {
                value: serde_json::json!({"published":true,"version":self.version}),
            }),
            "spawn" => {
                let children = ["first", "second"]
                    .map(|suffix| {
                        let mut child = crate::task(&format!("{}:{suffix}", task.id));
                        if task.input["children_sleep"] == true {
                            child.phase = "sleep".into();
                        }
                        if task.input["fail_fast"] == true {
                            child.phase = if suffix == "first" { "fail" } else { "sleep" }.into();
                        }
                        child
                    })
                    .to_vec();
                let task_ids = vec![];
                Ok(TaskTransition::Wait {
                    checkpoint: serde_json::json!({"children_created":true}),
                    phase: "finish".into(),
                    children,
                    task_ids,
                    policy: if task.input["fail_fast"] == true {
                        WaitPolicy::FailFast
                    } else {
                        WaitPolicy::AllSettled
                    },
                })
            }
            "sleep" => Ok(TaskTransition::Sleep {
                checkpoint: task.checkpoint,
                phase: "finish".into(),
                wake_at_ms: task.input["wake_at_ms"].as_u64().unwrap_or(u64::MAX / 4),
            }),
            "fail" => Ok(TaskTransition::Fail {
                error: "expected task failure".into(),
            }),
            "fault" => Err(RuntimeError::invalid(
                "fixture_handler_fault",
                "expected execution failure",
            )),
            _ => Err(RuntimeError::invalid(
                "fixture_phase",
                "unknown workflow phase",
            )),
        }
    }
}
