//! Static typed bindings for the capability's twelve Tembra pipelines.

use swarmie_task_contracts::*;
use tembra::{Result, core};
use tembra_hooks::{HookManager, HookRun, RustHookDefinition};

struct Binding<T: 'static> {
    definition: RustHookDefinition<T, ()>,
    registry: core::RegistryView,
}

impl<T: 'static> Binding<T> {
    fn new(scope: &core::FacetScope, definition: RustHookDefinition<T, ()>) -> Result<Self> {
        Ok(Self {
            registry: scope.registry(definition.descriptor().registry())?,
            definition,
        })
    }
}

// The list declares typed wiring only. Detection stays in notifications(), and all
// composition, invocation, priorities and physical settlement stay in Tembra.
macro_rules! pipelines {
    ($($variant:ident($field:ident): $payload:ty => $definition:ident),+ $(,)?) => {
        #[derive(Clone)]
        pub(crate) enum Notification { $($variant($payload)),+ }

        pub(crate) struct Pipelines { $($field: Binding<$payload>),+ }

        impl Pipelines {
            pub fn new(scope: &core::FacetScope) -> Result<Self> {
                Ok(Self { $($field: Binding::new(scope, $definition()?)?),+ })
            }

            pub fn has_handlers(&self, notification: &Notification) -> Result<bool> {
                match notification { $(Notification::$variant(_) =>
                    self.$field.registry.table(None).map(|table| !table.is_empty())),+ }
            }

            pub fn run(&self, manager: &HookManager, notification: Notification, context: &core::Context) -> HookRun<()> {
                match notification { $(Notification::$variant(value) =>
                    manager.run_series(&self.$field.definition, value, context)),+ }
            }
        }
    };
}

pipelines! {
    Created(created): TaskCreated => on_task_created,
    PhaseStarted(phase_started): TaskPhaseStarted => on_task_phase_started,
    PhaseSettled(phase_settled): TaskPhaseSettled => on_task_phase_settled,
    Checkpoint(checkpoint): TaskCheckpoint => on_task_checkpoint,
    Waiting(waiting): TaskWaiting => on_task_waiting,
    Sleeping(sleeping): TaskSleeping => on_task_sleeping,
    Recovered(recovered): TaskRecovered => on_task_recovered,
    Blocked(blocked): TaskBlocked => on_task_blocked,
    Parked(parked): TaskParked => on_task_parked,
    Retry(retry): TaskRetry => on_task_retry,
    CancellationRequested(cancellation_requested): TaskCancellationRequested => on_task_cancellation_requested,
    Terminal(terminal): TaskTerminal => on_task_terminal,
}

/// Ordered admission, not durable delivery or proof that every observer succeeded.
/// The task's invocation settlement is a separate fact, never inferred from a commit.
pub(crate) fn notifications(event: TaskLifecycleEvent) -> Vec<Notification> {
    let TaskLifecycleEvent::Committed {
        task,
        sequence,
        previous_status,
        current_status,
        origin,
        checkpoint_committed,
        cancellation_requested,
        outcome,
        blocking,
    } = event
    else {
        let TaskLifecycleEvent::PhaseSettled {
            task,
            invocation,
            outcome,
        } = event
        else {
            unreachable!("the lifecycle has only committed and physically settled facts");
        };
        return vec![Notification::PhaseSettled(TaskPhaseSettled {
            task,
            invocation,
            outcome,
        })];
    };
    let transition = TaskCommittedObservation {
        task,
        sequence,
        previous_status,
        current_status,
        origin,
        checkpoint_committed,
        cancellation_requested,
        outcome,
        blocking,
    };
    let mut notifications = Vec::new();
    if previous_status.is_none() {
        notifications.push(Notification::Created(TaskCreated {
            transition: transition.clone(),
        }));
    }
    if origin == TaskLifecycleOrigin::Reservation && current_status == TaskStatus::Running {
        notifications.push(Notification::PhaseStarted(TaskPhaseStarted {
            transition: transition.clone(),
        }));
    }
    if checkpoint_committed {
        notifications.push(Notification::Checkpoint(TaskCheckpoint {
            transition: transition.clone(),
        }));
    }
    if previous_status != Some(current_status) && current_status == TaskStatus::Waiting {
        notifications.push(Notification::Waiting(TaskWaiting {
            transition: transition.clone(),
        }));
    }
    if previous_status != Some(current_status) && current_status == TaskStatus::Sleeping {
        notifications.push(Notification::Sleeping(TaskSleeping {
            transition: transition.clone(),
        }));
    }
    if origin == TaskLifecycleOrigin::Recovery {
        notifications.push(Notification::Recovered(TaskRecovered {
            transition: transition.clone(),
        }));
    }
    if previous_status != Some(current_status) && current_status == TaskStatus::Blocked {
        notifications.push(Notification::Blocked(TaskBlocked {
            transition: transition.clone(),
        }));
    }
    if previous_status != Some(current_status) && current_status == TaskStatus::Parked {
        notifications.push(Notification::Parked(TaskParked {
            transition: transition.clone(),
        }));
    }
    if origin == TaskLifecycleOrigin::Retry {
        notifications.push(Notification::Retry(TaskRetry {
            transition: transition.clone(),
        }));
    }
    if cancellation_requested {
        notifications.push(Notification::CancellationRequested(
            TaskCancellationRequested {
                transition: transition.clone(),
            },
        ));
    }
    if previous_status != Some(current_status) && current_status == TaskStatus::Terminal {
        notifications.push(Notification::Terminal(TaskTerminal { transition }));
    }
    notifications
}
