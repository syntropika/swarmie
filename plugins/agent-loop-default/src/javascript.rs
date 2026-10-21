//! Explicit typed codecs for contributions implemented by trusted JavaScript plugins.

use std::{marker::PhantomData, rc::Rc};

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use swarmie_agent_contracts::*;
use swarmie_model_contracts::GenerateResult;
use tembra::prelude::*;
use tembra_executor_js::{Executor, NativeValue, ScopedCodec};
use tembra_hooks::{BailDecision, RustHookDefinition};

struct JsonCodec<T>(PhantomData<fn() -> T>);

impl<T: Serialize + DeserializeOwned + 'static> ScopedCodec<T> for JsonCodec<T> {
    fn encode(&self, value: T) -> Result<NativeValue> {
        tembra::contracts::to_portable_value(&value)
            .map(NativeValue::portable)
            .map_err(|error| RuntimeError::invalid("agent_hook_value", error.to_string()))
    }

    fn decode(&self, value: NativeValue) -> Result<T> {
        serde_json::from_value(json_value(value)?)
            .map_err(|error| RuntimeError::invalid("agent_hook_value", error.to_string()))
    }
}

fn json_value(value: NativeValue) -> Result<Value> {
    if let Ok(value) = value.clone().into_portable() {
        return Ok(value);
    }
    let record = value.into_record()?;
    let mut object = serde_json::Map::new();
    for (key, value) in record.entries() {
        if !value.is_absent() {
            object.insert(key.to_owned(), json_value(value.clone())?);
        }
    }
    Ok(Value::Object(object))
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
                "agent_hook_value",
                "observer must return no value",
            ))
        }
    }
}

fn codec<T: Serialize + DeserializeOwned + 'static>() -> Rc<dyn ScopedCodec<T>> {
    Rc::new(JsonCodec(PhantomData))
}

fn observer<A: Serialize + DeserializeOwned + 'static>(
    executor: &Executor,
    definition: RustHookDefinition<A, ()>,
) -> Result<()> {
    executor.register_hook(definition.descriptor(), codec::<A>(), Rc::new(UnitCodec))
}

fn transform<A: Serialize + DeserializeOwned + 'static>(
    executor: &Executor,
    definition: RustHookDefinition<A, A>,
) -> Result<()> {
    executor.register_hook(definition.descriptor(), codec::<A>(), codec::<A>())
}

/// Registers the real Tembra executor's exact typed contribution channels. Call once for
/// each Executor before loading JavaScript agent contributions. This performs no invocation
/// and owns no Host, plugin, Reference or hook dispatch loop.
///
/// # Errors
/// Refuses incompatible, duplicate or capacity-exhausted executor signatures. Earlier codec
/// registrations remain installed if a later registration fails.
pub fn register_agent_hooks(executor: &Executor) -> Result<()> {
    observer(executor, on_turn_start()?)?;
    transform(executor, prepare_context()?)?;
    transform(executor, before_model_call()?)?;
    observer(executor, after_model_call()?)?;
    observer(executor, on_model_event()?)?;
    transform(executor, before_tool_call()?)?;
    executor.register_hook(
        authorize_tool_call()?.descriptor(),
        codec::<ToolCall>(),
        codec::<BailDecision<ToolDenial>>(),
    )?;
    observer(executor, after_tool_call()?)?;
    observer(executor, on_step_end()?)?;
    observer(executor, on_turn_end()?)?;
    executor.register_hook(
        wrap_model_call()?.descriptor(),
        codec::<ModelCall>(),
        codec::<GenerateResult>(),
    )
}
