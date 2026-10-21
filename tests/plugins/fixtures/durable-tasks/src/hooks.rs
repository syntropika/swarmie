//! Native lifecycle observers registered through actual Tembra hook contributions.

use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

use futures::FutureExt;
use swarmie_task_contracts::*;
use tembra::{core, prelude::*};
use tembra_hooks::{HookArgs, hook_handler};

pub const HOOKS: [&str; 12] = [
    "swarmie.task-created",
    "swarmie.task-phase-started",
    "swarmie.task-phase-settled",
    "swarmie.task-checkpoint",
    "swarmie.task-waiting",
    "swarmie.task-sleeping",
    "swarmie.task-recovered",
    "swarmie.task-blocked",
    "swarmie.task-parked",
    "swarmie.task-retry",
    "swarmie.task-cancellation-requested",
    "swarmie.task-terminal",
];

/// Every specialized hook call, including several notifications for one commit.
#[derive(Clone, Debug)]
pub struct ObservedNotification {
    pub observer: String,
    pub hook: String,
    pub event: TaskLifecycleEvent,
}

/// Shared assertions observe managed entries without owning their execution.
#[derive(Default)]
pub struct Observer {
    /// Normalized facts, deduplicated per observer and fact across specialized hooks.
    pub events: RefCell<Vec<(String, TaskLifecycleEvent)>>,
    pub notifications: RefCell<Vec<ObservedNotification>>,
    pub fail: Cell<bool>,
    pub fail_on_hook: RefCell<Option<String>>,
    pub hold: Cell<bool>,
    pub started: Cell<bool>,
    pub cancelled: Cell<bool>,
    pub reentrant_cancel: RefCell<Option<(DurableTasksRef, String)>>,
    lease: RefCell<Option<core::InvocationLease>>,
}

impl Observer {
    /// Release physical ownership after logical cancellation has been observed.
    pub fn release(&self) {
        drop(self.lease.borrow_mut().take());
    }
}

fn committed(transition: TaskCommittedObservation) -> TaskLifecycleEvent {
    TaskLifecycleEvent::Committed {
        task: transition.task,
        sequence: transition.sequence,
        previous_status: transition.previous_status,
        current_status: transition.current_status,
        origin: transition.origin,
        checkpoint_committed: transition.checkpoint_committed,
        cancellation_requested: transition.cancellation_requested,
        outcome: transition.outcome,
        blocking: transition.blocking,
    }
}

async fn observe(
    observer: Rc<Observer>,
    owner: core::InvocationOwner,
    name: String,
    hook: &str,
    event: TaskLifecycleEvent,
    context: core::Context,
) -> Result<()> {
    observer
        .notifications
        .borrow_mut()
        .push(ObservedNotification {
            observer: name.clone(),
            hook: hook.into(),
            event: event.clone(),
        });
    {
        let mut facts = observer.events.borrow_mut();
        if !facts
            .iter()
            .any(|(seen_name, seen_event)| seen_name == &name && seen_event == &event)
        {
            facts.push((name, event));
        }
    }
    observer.started.set(true);
    let reentrant = observer.reentrant_cancel.borrow_mut().take();
    if let Some((tasks, id)) = reentrant {
        tasks.cancel(TaskLookup { id }).await?;
        tasks.resume().await?;
    }
    if observer.hold.get() {
        *observer.lease.borrow_mut() = Some(owner.retain(&context)?);
        context.cancellation().cancelled().await;
        observer.cancelled.set(true);
    }
    if observer.fail.get() || observer.fail_on_hook.borrow().as_deref() == Some(hook) {
        return Err(RuntimeError::invalid(
            "fixture_observer_failure",
            "expected lifecycle observation failure",
        ));
    }
    Ok(())
}

/// Registers twelve independent ordinary Tembra contributions.
pub fn facet(name: &str, priority: i64, observer: Rc<Observer>) -> Result<NativeFacet> {
    facet_with_hooks(name, priority, observer, &HOOKS)
}

/// Selects Registry contributions on installation, without filtering callback input.
pub fn facet_with_hooks(
    name: &str,
    priority: i64,
    observer: Rc<Observer>,
    hooks: &[&str],
) -> Result<NativeFacet> {
    if hooks.iter().any(|hook| !HOOKS.contains(hook)) {
        return Err(RuntimeError::invalid(
            "fixture_hook",
            "unknown lifecycle hook",
        ));
    }
    let name = name.to_owned();
    let hooks: Vec<String> = hooks.iter().map(|hook| (*hook).into()).collect();
    let mut members = BTreeMap::new();
    macro_rules! member {
        ($definition:ident, $method:literal) => {
            let definition = $definition()?;
            if hooks
                .iter()
                .any(|hook| hook == &definition.descriptor().descriptor().name)
            {
                members.insert(
                    $method.into(),
                    definition.descriptor().contribution_member(),
                );
            }
        };
    }
    member!(on_task_created, "created");
    member!(on_task_phase_started, "phase_started");
    member!(on_task_phase_settled, "phase_settled");
    member!(on_task_checkpoint, "checkpoint");
    member!(on_task_waiting, "waiting");
    member!(on_task_sleeping, "sleeping");
    member!(on_task_recovered, "recovered");
    member!(on_task_blocked, "blocked");
    member!(on_task_parked, "parked");
    member!(on_task_retry, "retry");
    member!(on_task_cancellation_requested, "cancellation_requested");
    member!(on_task_terminal, "terminal");
    let contract = Rc::new(
        core::contracts::Contract::local(
            format!("fixture.lifecycle-observer.{name}"),
            1,
            serde_json::json!({}),
            members,
        )
        .map_err(|error| RuntimeError::invalid("fixture_hook_contract", error.to_string()))?,
    );
    NativeFacet::new(
        format!("fixture.lifecycle-observer.{name}"),
        vec![],
        move |_, setup| {
            let observer = observer.clone();
            let name = name.clone();
            let hooks = hooks.clone();
            setup.core_mut().serve(
                contract.clone(),
                Rc::new(move |_, _, declaration| {
                    let owner = declaration.invocation_owner();
                    let mut implementation = core::Implementation::new();
                    macro_rules! contribution {
                        ($definition:ident, $payload:ty, $method:literal, $normalize:expr) => {
                            let definition = $definition()?;
                            let hook = definition.descriptor().descriptor().name.clone();
                            if hooks.contains(&hook) {
                                definition.register(
                                    declaration,
                                    name.clone(),
                                    priority,
                                    $method,
                                )?;
                                let observer = observer.clone();
                                let owner = owner.clone();
                                let name = name.clone();
                                let handler = hook_handler(Rc::new(
                                    move |args: HookArgs<$payload, ()>, context: core::Context| {
                                        let observer = observer.clone();
                                        let owner = owner.clone();
                                        let name = name.clone();
                                        let hook = hook.clone();
                                        let event = ($normalize)(args.input().clone());
                                        async move {
                                            observe(observer, owner, name, &hook, event, context)
                                                .await
                                        }
                                        .boxed_local()
                                    },
                                ));
                                implementation = implementation.method(
                                    $method,
                                    move |args: HookArgs<$payload, ()>, context| {
                                        handler(args, context)
                                    },
                                );
                            }
                        };
                    }
                    contribution!(
                        on_task_created,
                        TaskCreated,
                        "created",
                        |event: TaskCreated| committed(event.transition)
                    );
                    contribution!(
                        on_task_phase_started,
                        TaskPhaseStarted,
                        "phase_started",
                        |event: TaskPhaseStarted| committed(event.transition)
                    );
                    contribution!(
                        on_task_phase_settled,
                        TaskPhaseSettled,
                        "phase_settled",
                        |event: TaskPhaseSettled| TaskLifecycleEvent::PhaseSettled {
                            task: event.task,
                            invocation: event.invocation,
                            outcome: event.outcome
                        }
                    );
                    contribution!(
                        on_task_checkpoint,
                        TaskCheckpoint,
                        "checkpoint",
                        |event: TaskCheckpoint| committed(event.transition)
                    );
                    contribution!(
                        on_task_waiting,
                        TaskWaiting,
                        "waiting",
                        |event: TaskWaiting| committed(event.transition)
                    );
                    contribution!(
                        on_task_sleeping,
                        TaskSleeping,
                        "sleeping",
                        |event: TaskSleeping| committed(event.transition)
                    );
                    contribution!(
                        on_task_recovered,
                        TaskRecovered,
                        "recovered",
                        |event: TaskRecovered| committed(event.transition)
                    );
                    contribution!(
                        on_task_blocked,
                        TaskBlocked,
                        "blocked",
                        |event: TaskBlocked| committed(event.transition)
                    );
                    contribution!(on_task_parked, TaskParked, "parked", |event: TaskParked| {
                        committed(event.transition)
                    });
                    contribution!(on_task_retry, TaskRetry, "retry", |event: TaskRetry| {
                        committed(event.transition)
                    });
                    contribution!(
                        on_task_cancellation_requested,
                        TaskCancellationRequested,
                        "cancellation_requested",
                        |event: TaskCancellationRequested| committed(event.transition)
                    );
                    contribution!(
                        on_task_terminal,
                        TaskTerminal,
                        "terminal",
                        |event: TaskTerminal| committed(event.transition)
                    );
                    Ok(implementation)
                }),
                core::ServeOptions {
                    lifetime: core::Lifetime::Host,
                    ..Default::default()
                },
            )
        },
    )
}
