//! Language adapter only; Tembra owns hook composition, admission and physical settlement.

use std::rc::Rc;

use swarmie_task_contracts::{TaskLifecycleEvent, task_lifecycle};
use tembra::{Result, RuntimeError};
use tembra_executor_js::{Executor, NativeValue, ScopedCodec};

struct EventCodec;

impl ScopedCodec<TaskLifecycleEvent> for EventCodec {
    fn encode(&self, event: TaskLifecycleEvent) -> Result<NativeValue> {
        tembra::contracts::to_portable_value(&event)
            .map(NativeValue::portable)
            .map_err(|error| RuntimeError::invalid("task_lifecycle_value", error.to_string()))
    }

    fn decode(&self, value: NativeValue) -> Result<TaskLifecycleEvent> {
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

pub(crate) fn register(executor: &Executor) -> Result<()> {
    executor.register_hook(
        task_lifecycle()?.descriptor(),
        Rc::new(EventCodec),
        Rc::new(UnitCodec),
    )
}
