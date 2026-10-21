//! Durable phases for preparing requests, invoking models and confirming model outcomes.

use swarmie_agent_contracts::*;
use swarmie_model_contracts::{
    ContentPart, FinishReason, GenerateResult, GenerationOptions, ModelRequest, Role,
};
use swarmie_task_contracts::TaskTransition;
use tembra::prelude::*;
use tembra_hooks::{HookTerminal, HookType};

use crate::{
    context::{append_message, context_messages},
    journal,
    state::{Checkpoint, decode, encode, invalid, stable},
    tools,
    workflow::{Engine, next},
};

impl Engine {
    pub(crate) async fn prepare_model(
        &self,
        request: &StartTurn,
        mut cp: Checkpoint,
        context: &Context,
    ) -> Result<TaskTransition> {
        let turn = cp.turn(request);
        if cp.step >= request.limits.max_steps {
            return self.end_checkpoint(cp, "step limit reached");
        }
        if cp.output_tokens >= request.limits.max_output_tokens {
            return self.end_checkpoint(cp, "output token limit reached");
        }
        let suffix = format!("model:{}:request", cp.step);
        let id = stable(&request.turn_id, &suffix);
        let events = journal::history(&self.sessions, &request.session_id, context).await?;
        if journal::find(&events, &id).is_none() {
            let messages = context_messages(&events, request, &cp)?;
            let prepared = self
                .hooks
                .run_waterfall(
                    &prepare_context()?,
                    PreparedContext {
                        turn: turn.clone(),
                        messages,
                    },
                    context.core(),
                )
                .await?;
            if prepared.turn != turn {
                return Err(invalid("context hook changed turn identity"));
            }
            let available = self.tools.table(Some(context.core()))?;
            let mut definitions = Vec::new();
            for (name, _) in available.entries() {
                let definition = tools::definition(&self.tools, name, context)?.metadata;
                definitions.push(swarmie_model_contracts::ToolDefinition {
                    name: definition.name,
                    description: definition.description,
                    parameters: definition.arguments_schema,
                });
            }
            let maximum = u32::try_from(
                request
                    .limits
                    .max_output_tokens
                    .saturating_sub(cp.output_tokens),
            )
            .unwrap_or(u32::MAX);
            let call = ModelCall {
                turn: turn.clone(),
                request: ModelRequest {
                    request_id: stable(&request.turn_id, &format!("model:{}", cp.step)),
                    model: request.model.clone(),
                    messages: prepared.messages,
                    tools: definitions,
                    options: GenerationOptions {
                        max_output_tokens: Some(maximum),
                        ..Default::default()
                    },
                },
            };
            let mut transformed = self
                .hooks
                .run_waterfall(&before_model_call()?, call.clone(), context.core())
                .await?;
            if transformed.turn != turn
                || transformed.request.request_id != call.request.request_id
                || transformed.request.model != request.model
            {
                return Err(invalid("model hook changed causal identity"));
            }
            if transformed
                .request
                .messages
                .iter()
                .flat_map(|message| &message.content)
                .any(|part| matches!(part, ContentPart::Reasoning { .. }))
            {
                return Err(invalid(
                    "hidden reasoning cannot be persisted in session requests",
                ));
            }
            transformed.request.options.max_output_tokens = Some(
                transformed
                    .request
                    .options
                    .max_output_tokens
                    .unwrap_or(maximum)
                    .min(maximum),
            );
            journal::append(
                &self.sessions,
                &turn,
                &suffix,
                "swarmie.agent.model-request",
                encode(&transformed)?,
                context,
            )
            .await?;
        }
        cp.request_event = Some(id);
        cp.response_event = None;
        cp.tool_index = 0;
        next(cp, "model")
    }

    pub(crate) async fn model(
        &self,
        request: &StartTurn,
        mut cp: Checkpoint,
        context: &Context,
    ) -> Result<TaskTransition> {
        let turn = cp.turn(request);
        let events = journal::history(&self.sessions, &request.session_id, context).await?;
        let call: ModelCall = decode(
            journal::own_event(
                &events,
                cp.request_event
                    .as_deref()
                    .ok_or_else(|| invalid("request reference absent"))?,
                &turn,
                "swarmie.agent.model-request",
            )?
            .payload
            .clone(),
        )?;
        let suffix = format!("model:{}:confirmed", cp.step);
        let id = stable(&request.turn_id, &suffix);
        if journal::find(&events, &id).is_none() {
            let operation = {
                let table = self.terminals.table(Some(context.core()))?;
                table
                    .get("default")
                    .and_then(|entry| entry.operation())
                    .ok_or_else(|| invalid("model terminal is unavailable"))?
                    .clone()
            };
            let terminal = HookTerminal::portable(
                operation,
                hook_type::<ModelCall>()?,
                hook_type::<GenerateResult>()?,
                |value| encode(&value).map(Some),
                |value| decode(value.ok_or_else(|| invalid("model terminal returned no result"))?),
            )?;
            let result = self
                .hooks
                .run_middleware(&wrap_model_call()?, call.clone(), context.core(), terminal)
                .await?;
            check_model_result(&result, &call)?;
            // Middleware may supply a complete replacement without entering the terminal.
            journal::append(
                &self.sessions,
                &turn,
                &suffix,
                "swarmie.agent.model-confirmed",
                serde_json::json!({"operation_id":call.request.request_id,"result":result}),
                context,
            )
            .await?;
        }
        cp.response_event = Some(id);
        next(cp, "model-result")
    }

    pub(crate) async fn model_result(
        &self,
        request: &StartTurn,
        mut cp: Checkpoint,
        context: &Context,
    ) -> Result<TaskTransition> {
        let turn = cp.turn(request);
        let (call, result) = self.model_observation(request, &cp, context).await?;
        self.hooks
            .run_series(
                &after_model_call()?,
                ModelObservation {
                    call: call.clone(),
                    result: result.clone(),
                },
                context.core(),
            )
            .await?;
        match result {
            GenerateResult::Error { .. } => self.end_checkpoint(cp, "model request failed"),
            GenerateResult::Response { response } => {
                let remaining = request
                    .limits
                    .max_output_tokens
                    .saturating_sub(cp.output_tokens);
                let charged = response.usage.output_tokens.unwrap_or_else(|| {
                    // Unknown usage consumes the admitted cap; raw usage remains in Session.
                    u64::from(call.request.options.max_output_tokens.unwrap_or(u32::MAX))
                });
                cp.output_tokens = cp.output_tokens.saturating_add(charged.min(remaining));
                if charged > remaining {
                    return self.end_checkpoint(
                        cp,
                        "provider output usage exceeded the remaining turn budget",
                    );
                }
                append_message(
                    &self.sessions,
                    &turn,
                    &format!("model:{}:message", cp.step),
                    response.message.clone(),
                    None,
                    context,
                )
                .await?;
                let calls = response
                    .message
                    .content
                    .iter()
                    .filter(|part| matches!(part, ContentPart::ToolCall { .. }))
                    .count();
                if calls > 0 && response.finish_reason != FinishReason::ToolCalls {
                    return self
                        .end_checkpoint(cp, "model returned inconsistent tool finish reason");
                }
                if calls == 0 && response.finish_reason == FinishReason::ToolCalls {
                    return self.end_checkpoint(cp, "model requested tools without tool calls");
                }
                if calls == 0 {
                    next(cp, "step-end")
                } else if cp.output_tokens >= request.limits.max_output_tokens {
                    self.end_checkpoint(cp, "output token limit reached")
                } else {
                    next(cp, "prepare-tool")
                }
            }
        }
    }

    pub(crate) async fn model_observation(
        &self,
        request: &StartTurn,
        cp: &Checkpoint,
        context: &Context,
    ) -> Result<(ModelCall, GenerateResult)> {
        let events = journal::history(&self.sessions, &request.session_id, context).await?;
        let turn = cp.turn(request);
        let call: ModelCall = decode(
            journal::own_event(
                &events,
                cp.request_event
                    .as_deref()
                    .ok_or_else(|| invalid("missing request reference"))?,
                &turn,
                "swarmie.agent.model-request",
            )?
            .payload
            .clone(),
        )?;
        let actual_id = stable(&request.turn_id, &format!("model:{}:actual", cp.step));
        let call = if journal::find(&events, &actual_id).is_some() {
            decode(
                journal::own_event(&events, &actual_id, &turn, "swarmie.agent.model-actual")?
                    .payload
                    .clone(),
            )?
        } else {
            call
        };
        let event = journal::own_event(
            &events,
            cp.response_event
                .as_deref()
                .ok_or_else(|| invalid("missing response reference"))?,
            &turn,
            "swarmie.agent.model-confirmed",
        )?;
        if event.payload["operation_id"] != call.request.request_id {
            return Err(invalid("model receipt operation differs"));
        }
        let result: GenerateResult = decode(event.payload["result"].clone())?;
        check_model_result(&result, &call)?;
        Ok((call, result))
    }
}

fn hook_type<T: serde::Serialize + JsonSchema + 'static>() -> Result<HookType<T>> {
    HookType::serde(
        tembra::contracts::schema_for::<T>().map_err(|error| invalid(error.to_string()))?,
    )
}

fn check_model_result(result: &GenerateResult, call: &ModelCall) -> Result<()> {
    if let GenerateResult::Response { response } = result {
        if response.request_id != call.request.request_id
            || response.model != call.request.model
            || response.message.role != Role::Assistant
        {
            return Err(invalid("model result correlation or role differs"));
        }
        if response
            .message
            .content
            .iter()
            .any(|part| matches!(part, ContentPart::Reasoning { .. }))
        {
            return Err(invalid("hidden reasoning cannot be persisted"));
        }
    }
    Ok(())
}
