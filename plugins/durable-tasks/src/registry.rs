use swarmie_task_contracts::{BlockingReason, TaskDefinition, TaskHandler, TaskRecord};
use tembra::{ContractToken, Result, RuntimeError, core};

pub(crate) struct Definition {
    pub metadata: TaskDefinition,
    pub operation: core::RegisteredOperation,
}

/// Selects the actual admitted Context's table, never a current table from another epoch.
pub(crate) fn definition(
    registry: &core::RegistryView,
    task: &TaskRecord,
    context: &core::Context,
) -> std::result::Result<Definition, BlockingReason> {
    let invalid = |detail: String| BlockingReason::IncompatibleDefinition {
        target_version: task.definition_version,
        detail,
    };
    let table = registry
        .table(Some(context))
        .map_err(|error| invalid(error.to_string()))?;
    let entry = table
        .get(&task.kind)
        .ok_or(BlockingReason::MissingDefinition)?;
    let metadata: TaskDefinition =
        serde_json::from_value(entry.data().clone()).map_err(|error| invalid(error.to_string()))?;
    let invalid = |detail: String| BlockingReason::IncompatibleDefinition {
        target_version: metadata.version,
        detail,
    };
    if metadata.kind != task.kind || metadata.version == 0 {
        return Err(invalid("definition kind or version is invalid".into()));
    }
    if metadata.version < task.definition_version {
        return Err(invalid(
            "registered definition is older than the persisted checkpoint version".into(),
        ));
    }
    let operation = entry
        .operation()
        .ok_or_else(|| invalid("definition has no registered operation".into()))?
        .clone();
    validate_operation(&operation, context).map_err(|error| invalid(error.to_string()))?;
    Ok(Definition {
        metadata,
        operation,
    })
}

fn validate_operation(
    operation: &core::RegisteredOperation,
    context: &core::Context,
) -> Result<()> {
    let canonical = TaskHandler.descriptor()?;
    let expected = canonical
        .members("")
        .and_then(|members| members.get("step"));
    let actual = operation
        .contract()
        .members("")
        .and_then(|members| members.get(operation.method()));
    if actual != expected {
        return Err(RuntimeError::invalid(
            "durable_tasks_handler_schema",
            "registered step differs from the canonical task handler method schema",
        ));
    }
    operation.check_portable(context)
}
