//! Durable phases for preparing tool intentions and publishing confirmed tool results.

use swarmie_agent_contracts::*;
use swarmie_model_contracts::{ContentPart, GenerateResult, Message, Role};
use swarmie_task_contracts::TaskTransition;
use tembra::prelude::*;
use tembra_hooks::BailOutcome;

use crate::{
    context::append_message,
    journal,
    state::{Checkpoint, decode, encode, invalid, stable},
    tools::{self, ToolIntent},
    workflow::{Engine, next},
};

impl Engine {
    pub(crate) async fn prepare_tool(
        &self,
        request: &StartTurn,
        mut cp: Checkpoint,
        context: &Context,
    ) -> Result<TaskTransition> {
        let turn = cp.turn(request);
        if cp.tool_calls >= request.limits.max_tool_calls {
            return self.end_checkpoint(cp, "tool call limit reached");
        }
        let (_, result) = self.model_observation(request, &cp, context).await?;
        let GenerateResult::Response { response } = result else {
            return Err(invalid("tools require a confirmed model response"));
        };
        let calls = response
            .message
            .content
            .iter()
            .filter_map(|part| {
                if let ContentPart::ToolCall { call } = part {
                    Some(call)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let call = calls
            .get(cp.tool_index as usize)
            .ok_or_else(|| invalid("tool index outside response"))?;
        if calls.iter().filter(|other| other.id == call.id).count() != 1 {
            return self.end_checkpoint(cp, "model repeated tool call identity");
        }
        let suffix = format!("tool:{}:{}:intent", cp.step, cp.tool_index);
        let id = stable(&request.turn_id, &suffix);
        let events = journal::history(&self.sessions, &request.session_id, context).await?;
        if journal::find(&events, &id).is_none() {
            let invocation = ToolCall {
                turn: turn.clone(),
                operation_id: stable(
                    &request.turn_id,
                    &format!("tool:{}:{}", cp.step, cp.tool_index),
                ),
                tool_call_id: call.id.clone(),
                tool: call.name.clone(),
                arguments: call.arguments.clone(),
            };
            let transformed = self
                .hooks
                .run_waterfall(&before_tool_call()?, invocation.clone(), context.core())
                .await?;
            if transformed.turn != turn
                || transformed.operation_id != invocation.operation_id
                || transformed.tool_call_id != invocation.tool_call_id
                || transformed.tool != invocation.tool
            {
                return Err(invalid("tool hook changed causal identity"));
            }
            let definition = tools::definition(&self.tools, &transformed.tool, context)?;
            tools::validate_arguments(&definition.metadata, &transformed)?;
            let denial = match self
                .hooks
                .run_bail(
                    &authorize_tool_call()?,
                    transformed.clone(),
                    context.core(),
                    None,
                )
                .await?
            {
                BailOutcome::Unresolved => None,
                BailOutcome::Resolved(denial) => Some(denial.reason),
            };
            let intent = ToolIntent {
                call: transformed,
                replay: definition.metadata.replay,
                denial,
            };
            journal::append(
                &self.sessions,
                &turn,
                &suffix,
                "swarmie.agent.tool-intent",
                encode(&intent)?,
                context,
            )
            .await?;
        }
        cp.tool_event = Some(id);
        cp.tool_calls += 1;
        next(cp, "tool")
    }

    pub(crate) async fn tool(
        &self,
        request: &StartTurn,
        cp: Checkpoint,
        context: &Context,
    ) -> Result<TaskTransition> {
        let intent = self.tool_intent(request, &cp, context).await?;
        self.execute_tool(intent, cp.tool_index, context).await?;
        next(cp, "tool-result")
    }

    pub(crate) async fn tool_result(
        &self,
        request: &StartTurn,
        mut cp: Checkpoint,
        context: &Context,
    ) -> Result<TaskTransition> {
        let turn = cp.turn(request);
        let intent = self.tool_intent(request, &cp, context).await?;
        let result = self
            .execute_tool(intent.clone(), cp.tool_index, context)
            .await?;
        self.hooks
            .run_series(
                &after_tool_call()?,
                ToolObservation {
                    call: intent.call,
                    result: result.clone(),
                },
                context.core(),
            )
            .await?;
        append_message(
            &self.sessions,
            &turn,
            &format!("tool:{}:{}:message", cp.step, cp.tool_index),
            Message {
                role: Role::Tool,
                content: vec![ContentPart::ToolResult {
                    call_id: result.tool_call_id.clone(),
                    value: result.output,
                    is_error: result.is_error,
                }],
            },
            Some(result.tool_call_id),
            context,
        )
        .await?;
        cp.tool_index += 1;
        let (_, result) = self.model_observation(request, &cp, context).await?;
        let GenerateResult::Response { response } = result else {
            return Err(invalid("missing model response"));
        };
        let count = response
            .message
            .content
            .iter()
            .filter(|part| matches!(part, ContentPart::ToolCall { .. }))
            .count();
        if (cp.tool_index as usize) < count {
            next(cp, "prepare-tool")
        } else {
            next(cp, "step-end")
        }
    }

    pub(crate) async fn tool_intent(
        &self,
        request: &StartTurn,
        cp: &Checkpoint,
        context: &Context,
    ) -> Result<ToolIntent> {
        let events = journal::history(&self.sessions, &request.session_id, context).await?;
        let intent: ToolIntent = decode(
            journal::own_event(
                &events,
                cp.tool_event
                    .as_deref()
                    .ok_or_else(|| invalid("missing tool reference"))?,
                &cp.turn(request),
                "swarmie.agent.tool-intent",
            )?
            .payload
            .clone(),
        )?;
        let (_, result) = self.model_observation(request, cp, context).await?;
        let GenerateResult::Response { response } = result else {
            return Err(invalid("tool requires confirmed model request"));
        };
        let expected = response
            .message
            .content
            .iter()
            .filter_map(|part| {
                if let ContentPart::ToolCall { call } = part {
                    Some(call)
                } else {
                    None
                }
            })
            .nth(cp.tool_index as usize)
            .ok_or_else(|| invalid("tool index differs from confirmed model request"))?;
        if intent.call.turn != cp.turn(request)
            || intent.call.operation_id
                != stable(
                    &request.turn_id,
                    &format!("tool:{}:{}", cp.step, cp.tool_index),
                )
            || intent.call.tool_call_id != expected.id
            || intent.call.tool != expected.name
        {
            return Err(invalid(
                "persisted tool intent does not belong to this operation",
            ));
        }
        Ok(intent)
    }
}
