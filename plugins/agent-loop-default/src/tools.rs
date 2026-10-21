use swarmie_agent_contracts::StartTurn;
use swarmie_agent_contracts::{
    ReplayPolicy, ToolAuthorization, ToolCall, ToolDefinition, ToolHandler, ToolPolicy,
    ToolPolicyInput, ToolResult,
};
use swarmie_task_contracts::{DurableTasks, DurableTasksInput, TaskLookup, TaskStatus};
use tembra::{core, prelude::*};

use crate::{
    journal,
    state::{decode, encode, invalid, stable},
    workflow::Engine,
};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ToolIntent {
    pub call: ToolCall,
    pub replay: ReplayPolicy,
    pub denial: Option<String>,
}

pub(crate) struct Definition {
    pub metadata: ToolDefinition,
    pub operation: core::RegisteredOperation,
}

pub(crate) fn definition(
    registry: &core::RegistryView,
    name: &str,
    context: &Context,
) -> Result<Definition> {
    let table = registry.table(Some(context.core()))?;
    let entry = table.get(name).ok_or_else(|| {
        RuntimeError::invalid("agent_tool_missing", "requested tool is unavailable")
    })?;
    let metadata: ToolDefinition = decode(entry.data().clone())?;
    if metadata.name != name {
        return Err(invalid("tool registration identity differs"));
    }
    let operation = entry
        .operation()
        .ok_or_else(|| invalid("tool has no managed operation"))?
        .clone();
    let canonical = ToolHandler.descriptor()?;
    let expected = canonical
        .members("")
        .and_then(|members| members.get("execute"));
    let actual = operation
        .contract()
        .members("")
        .and_then(|members| members.get(operation.method()));
    if actual != expected {
        return Err(invalid("tool method differs from canonical schema"));
    }
    operation.check_portable(context.core())?;
    Ok(Definition {
        metadata,
        operation,
    })
}

pub(crate) fn validate_arguments(definition: &ToolDefinition, call: &ToolCall) -> Result<()> {
    let schema = tembra::contracts::normalize_schema(&definition.arguments_schema)
        .map_err(|error| RuntimeError::invalid("agent_tool_schema", error.to_string()))?;
    tembra::contracts::validate(&schema, &call.arguments)
        .map_err(|error| RuntimeError::invalid("agent_tool_arguments", error.to_string()))
}

impl Engine {
    pub(crate) async fn execute_tool(
        &self,
        intent: ToolIntent,
        index: u32,
        context: &Context,
    ) -> Result<ToolResult> {
        let call = &intent.call;
        let suffix = format!("tool:{}:{}", call.turn.step, index);
        let events = journal::history(&self.sessions, &call.turn, context).await?;
        let confirmed_id = stable(&call.turn, &format!("{suffix}:confirmed"));
        if let Some(event) = journal::find(&events, &confirmed_id) {
            let event = journal::own_event(
                &events,
                &event.event_id,
                &call.turn,
                "swarmie.agent.tool-confirmed",
            )?;
            return confirmed_tool(event.payload.clone(), call);
        }
        let resolution_id = stable(&call.turn, &format!("resolution:{}", call.operation_id));
        if let Some(event) = journal::find(&events, &resolution_id) {
            let resolution: swarmie_agent_contracts::KnownResult = decode(event.payload.clone())?;
            let confirmed = journal::own_event(
                &events,
                &resolution.result_event_id,
                &call.turn,
                "swarmie.agent.tool-confirmed",
            )?;
            let result = confirmed_tool(confirmed.payload.clone(), call)?;
            journal::append(
                &self.sessions,
                &call.turn,
                &format!("{suffix}:confirmed"),
                "swarmie.agent.tool-confirmed",
                serde_json::json!({"operation_id":call.operation_id,"result":result}),
                context,
            )
            .await?;
            return Ok(result);
        }
        if let Some(reason) = intent.denial {
            let result = ToolResult {
                operation_id: call.operation_id.clone(),
                tool_call_id: call.tool_call_id.clone(),
                output: serde_json::json!({"denied":reason}),
                is_error: true,
            };
            journal::append(
                &self.sessions,
                &call.turn,
                &format!("{suffix}:confirmed"),
                "swarmie.agent.tool-confirmed",
                serde_json::json!({"operation_id":call.operation_id,"result":result}),
                context,
            )
            .await?;
            return Ok(result);
        }
        self.admit_tool(call, index, context).await?;
        let definition = definition(&self.tools, &call.tool, context)?;
        validate_arguments(&definition.metadata, call)?;
        let started_id = stable(&call.turn, &format!("{suffix}:started"));
        if journal::find(&events, &started_id).is_some()
            && (intent.replay != ReplayPolicy::Safe
                || definition.metadata.replay != ReplayPolicy::Safe)
        {
            return Err(RuntimeError::unavailable(
                "agent_tool_uncertain",
                "external tool may have executed; reconcile a confirmed result before continuing",
            ));
        }
        // Mandatory policy is resolved only at this boundary. A missing provider cannot
        // prevent a no-tool turn, and can never become an implicit authorization grant.
        let policy = context.get(ToolPolicy, ToolPolicyInput {}).await?;
        let ToolAuthorization { allowed, reason } =
            policy.with_context(context).authorize(call.clone()).await?;
        if !allowed {
            let result = ToolResult {
                operation_id: call.operation_id.clone(),
                tool_call_id: call.tool_call_id.clone(),
                output: serde_json::json!({"denied":reason}),
                is_error: true,
            };
            journal::append(
                &self.sessions,
                &call.turn,
                &format!("{suffix}:confirmed"),
                "swarmie.agent.tool-confirmed",
                serde_json::json!({"operation_id":call.operation_id,"result":result}),
                context,
            )
            .await?;
            return Ok(result);
        }
        journal::append(
            &self.sessions,
            &call.turn,
            &format!("{suffix}:started"),
            "swarmie.agent.tool-started",
            serde_json::json!({"operation_id":call.operation_id,"replay":intent.replay}),
            context,
        )
        .await?;
        self.admit_tool(call, index, context).await?;
        context.check(context.host_id())?;
        // This managed operation is the effect boundary. Its provider independently checks
        // authority; optional lifecycle handlers never receive an execution grant.
        let operation = definition
            .operation
            .call_json_tracked(context.core(), Some(encode(call)?));
        let receipt = operation.receipt();
        let result = operation.await;
        receipt.settled().await;
        let value = result?.ok_or_else(|| invalid("tool returned no result"))?;
        let result: ToolResult = tembra::contracts::from_portable_value(&value)
            .map_err(|error| invalid(error.to_string()))?;
        check_result(&result, call)?;
        journal::append(
            &self.sessions,
            &call.turn,
            &format!("{suffix}:confirmed"),
            "swarmie.agent.tool-confirmed",
            serde_json::json!({"operation_id":call.operation_id,"result":result}),
            context,
        )
        .await?;
        Ok(result)
    }
    async fn admit_tool(&self, call: &ToolCall, index: u32, context: &Context) -> Result<()> {
        let tasks = context.get(DurableTasks, DurableTasksInput {}).await?;
        let task = tasks
            .with_context(context)
            .get(TaskLookup {
                id: call.turn.task_id.clone(),
            })
            .await?
            .ok_or_else(|| invalid("tool turn task is absent"))?;
        let start: StartTurn = decode(task.input.clone())?;
        let cp: crate::state::Checkpoint = decode(task.checkpoint.clone())?;
        cp.validate_identity(&start)?;
        if task.kind != crate::TURN_KIND
            || task.definition_version != 2
            || task.status != TaskStatus::Running
            || task.phase != "tool"
            || task.cancellation_requested
            || call.turn != cp.turn(&start)
            || call.turn.task_id != crate::state::task_id(&start)
            || index != cp.tool_index
            || cp.tool_calls > start.limits.max_tool_calls
            || cp.step >= start.limits.max_steps
            || call.operation_id != stable(&start, &format!("tool:{}:{index}", cp.step))
            || crate::state::now_ms()?
                >= cp
                    .started_at_ms
                    .saturating_add(start.limits.max_duration_ms)
        {
            return Err(invalid(
                "tool operation is no longer admitted by the durable turn",
            ));
        }
        let events = journal::history(&self.sessions, &start, context).await?;
        let intent_event = journal::own_event(
            &events,
            cp.tool_event
                .as_deref()
                .ok_or_else(|| invalid("tool intent reference absent"))?,
            &call.turn,
            "swarmie.agent.tool-intent",
        )?;
        let intent: ToolIntent = decode(intent_event.payload.clone())?;
        if &intent.call != call {
            return Err(invalid("tool call differs from admitted durable intent"));
        }
        Ok(())
    }
}

fn confirmed_tool(payload: serde_json::Value, call: &ToolCall) -> Result<ToolResult> {
    if payload["operation_id"] != call.operation_id {
        return Err(invalid("tool receipt operation differs"));
    }
    let result: ToolResult = decode(payload["result"].clone())?;
    check_result(&result, call)?;
    Ok(result)
}

fn check_result(result: &ToolResult, call: &ToolCall) -> Result<()> {
    if result.operation_id != call.operation_id || result.tool_call_id != call.tool_call_id {
        return Err(invalid("tool result correlation differs"));
    }
    Ok(())
}
