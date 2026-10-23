//! Synthetic handlers for invalid transitions and physical invocation ownership acceptance.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use swarmie_task_contracts::{
    TaskDefinition, TaskInvocation, TaskSpec, TaskTransition, WaitPolicy, task_definitions,
};
use tembra::{core, prelude::*};

/// External observation of a real retained invocation entry. Releasing the fixture lease is
/// deliberately separate from cooperative logical cancellation of the handler Context.
#[derive(Default)]
pub struct Control {
    pub started: Cell<bool>,
    lease: RefCell<Option<core::InvocationLease>>,
    gate_started: core::CancellationToken,
    released: Cell<bool>,
}

impl Control {
    /// Lets the already admitted physical entry settle, without reviving its cancelled Context.
    pub fn release(&self) {
        self.released.set(true);
        let lease = self.lease.borrow_mut().take();
        drop(lease);
    }
}

#[tembra::contract(local, name = "swarmie.fixture.adversarial-task", version = 1)]
trait AdversarialTask {
    #[portable]
    async fn step(&self, invocation: TaskInvocation) -> Result<TaskTransition>;
}

/// Creates a genuinely managed application Facet with the canonical portable step schema.
pub fn facet(control: Rc<Control>, version: u32) -> Result<NativeFacet> {
    NativeFacet::new(
        "swarmie.fixture.adversarial-task",
        vec![],
        move |_, setup| {
            let control = control.clone();
            setup.serve_with_options(
                AdversarialTask,
                move |_, _, declaration| {
                    declaration.core_mut().register_method(
                        &task_definitions()?,
                        "fixture.adversarial",
                        serde_json::to_value(TaskDefinition {
                            kind: "fixture.adversarial".into(),
                            version,
                        })
                        .unwrap(),
                        "step",
                    )?;
                    Ok(Handler {
                        control: control.clone(),
                        owner: declaration.core_mut().invocation_owner(),
                    })
                },
                core::ServeOptions {
                    lifetime: core::Lifetime::Host,
                    ..Default::default()
                },
            )
        },
    )
}

struct Handler {
    control: Rc<Control>,
    owner: core::InvocationOwner,
}

impl AdversarialTaskProvider for Handler {
    async fn step(&self, invocation: TaskInvocation, context: Context) -> Result<TaskTransition> {
        match invocation {
            TaskInvocation::Migrate { .. } => Err(RuntimeError::invalid(
                "fixture_migration",
                "checkpoint cannot be migrated",
            )),
            TaskInvocation::Run { task, .. } => match task.phase.as_str() {
                "complete" => Ok(TaskTransition::Complete {
                    value: serde_json::Value::Null,
                }),
                "spawn-failure" => Ok(TaskTransition::Wait {
                    checkpoint: serde_json::json!({"spawned":true}),
                    phase: "done".into(),
                    children: ["fail", "gate"]
                        .into_iter()
                        .map(|phase| TaskSpec {
                            id: format!("{}:{phase}", task.id),
                            kind: task.kind.clone(),
                            definition_version: 1,
                            input: serde_json::Value::Null,
                            checkpoint: serde_json::Value::Null,
                            phase: phase.into(),
                        })
                        .collect(),
                    task_ids: vec![],
                    policy: WaitPolicy::FailFast,
                }),
                "fail" => {
                    self.control.gate_started.cancelled().await;
                    Ok(TaskTransition::Fail {
                        error: "owned child failure".into(),
                    })
                }
                "gate" => {
                    assert_eq!(task.status, swarmie_task_contracts::TaskStatus::Running);
                    assert!(task.attempts > 0);
                    if self.control.released.get() {
                        return Ok(TaskTransition::Complete {
                            value: serde_json::Value::Null,
                        });
                    }
                    *self.control.lease.borrow_mut() = Some(self.owner.retain(context.core())?);
                    self.control.started.set(true);
                    self.control.gate_started.cancel(RuntimeError::aborted(
                        "fixture_started",
                        "gated invocation entered",
                    ));
                    context.cancellation().cancelled().await;
                    Ok(TaskTransition::Complete {
                        value: serde_json::Value::Null,
                    })
                }
                "wrong-mode" => Ok(TaskTransition::Migrate {
                    checkpoint: serde_json::json!({"invalid_progress":true}),
                    phase: "done".into(),
                }),
                "invalid-phase" => Ok(TaskTransition::Checkpoint {
                    checkpoint: serde_json::json!({"invalid_progress":true}),
                    phase: "!".into(),
                }),
                "no-progress" => Ok(TaskTransition::Checkpoint {
                    checkpoint: task.checkpoint.clone(),
                    phase: task.phase.clone(),
                }),
                "continue-invalid-phase" => Ok(TaskTransition::Continue {
                    checkpoint: serde_json::json!({"invalid_progress":true}),
                    phase: "!".into(),
                }),
                "continue-no-progress" => Ok(TaskTransition::Continue {
                    checkpoint: task.checkpoint.clone(),
                    phase: task.phase.clone(),
                }),
                _ => Err(RuntimeError::invalid(
                    "fixture_phase",
                    "unknown adversarial phase",
                )),
            },
        }
    }
}
