//! Capability-owned language adaptation; Tembra owns hook composition and execution.

use std::{marker::PhantomData, rc::Rc};

use serde::{Serialize, de::DeserializeOwned};
use swarmie_task_contracts::{
    on_task_blocked, on_task_cancellation_requested, on_task_checkpoint, on_task_created,
    on_task_parked, on_task_phase_settled, on_task_phase_started, on_task_recovered, on_task_retry,
    on_task_sleeping, on_task_terminal, on_task_waiting,
};
use tembra::{Result, RuntimeError};
use tembra_executor_js::{Executor, NativeValue, ScopedCodec};
use tembra_hooks::RustHookDefinition;

struct EventCodec<T>(PhantomData<T>);

impl<T: Serialize + DeserializeOwned + 'static> ScopedCodec<T> for EventCodec<T> {
    fn encode(&self, event: T) -> Result<NativeValue> {
        tembra::contracts::to_portable_value(&event)
            .map(NativeValue::portable)
            .map_err(|error| RuntimeError::invalid("task_lifecycle_value", error.to_string()))
    }

    fn decode(&self, value: NativeValue) -> Result<T> {
        serde_json::from_value(value.into_portable()?)
            .map_err(|error| RuntimeError::invalid("task_lifecycle_value", error.to_string()))
    }
}

struct UnitCodec;

impl ScopedCodec<()> for UnitCodec {
    fn encode(&self, (): ()) -> Result<NativeValue> {
        Ok(NativeValue::absent())
    }

    fn decode(&self, value: NativeValue) -> Result<()> {
        if value.is_absent() {
            Ok(())
        } else {
            Err(RuntimeError::invalid(
                "task_lifecycle_value",
                "series result must be absent",
            ))
        }
    }
}

fn register<T: Serialize + DeserializeOwned + 'static>(
    executor: &Executor,
    definition: RustHookDefinition<T, ()>,
) -> Result<()> {
    executor.register_hook(
        definition.descriptor(),
        Rc::new(EventCodec::<T>(PhantomData)),
        Rc::new(UnitCodec),
    )
}

/// Registers this capability's typed Hooks on an embedding-owned JavaScript executor.
///
/// Call before activating dynamic contributors. Definitions remain available independently
/// of the scheduler's application selection; Tembra controls contribution availability.
///
/// # Errors
/// Returns invalid definitions or incompatible existing executor registrations.
pub fn register_language_hooks(executor: &Executor) -> Result<()> {
    register(executor, on_task_created()?)?;
    register(executor, on_task_phase_started()?)?;
    register(executor, on_task_phase_settled()?)?;
    register(executor, on_task_checkpoint()?)?;
    register(executor, on_task_waiting()?)?;
    register(executor, on_task_sleeping()?)?;
    register(executor, on_task_recovered()?)?;
    register(executor, on_task_blocked()?)?;
    register(executor, on_task_parked()?)?;
    register(executor, on_task_retry()?)?;
    register(executor, on_task_cancellation_requested()?)?;
    register(executor, on_task_terminal()?)?;
    Ok(())
}
