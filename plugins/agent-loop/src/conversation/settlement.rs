use super::{Checkpoint, History, writer::Writer};
use crate::{
    FinishReason, Message, Recovery, ToolExecution, ToolResult, Tools,
    engine::Confirmation,
    hooks::{ToolOutput, transform_tool_result},
    model::invalid,
};
use std::cell::RefCell;
use swarmie_conversations::Method;
use tembra::{Code, Context, Error, HookHandle};

/// The result of tool recovery without dispatch of another tool or model request.
#[derive(Debug)]
pub enum ToolSettlement {
    Settled(Checkpoint),
    Unknown(Checkpoint),
}

impl History {
    /// Close unfinished tool calls without executing another operation.
    /// Recover the pending provider result before an unknown outcome can be acknowledged.
    /// Set `acknowledge_unknown` only after the user accepts the unknown external outcome.
    /// Unknown outcomes stay unchanged otherwise. Remaining undispatched calls are blocked.
    /// The returned checkpoint is not evidence that the model turn completed.
    /// Wait for the enclosing execution to settle before this call.
    /// This method commits ordinary history. It does not update durable task storage.
    /// Supply the same declared result hook as the interrupted Agent.
    /// Use `None` only for an Agent without bound hooks.
    pub async fn settle_tools(
        &self,
        context: &Context,
        tools: &dyn Tools,
        checkpoint: Checkpoint,
        acknowledge_unknown: bool,
        result_hook: Option<&HookHandle<ToolOutput, ToolOutput>>,
    ) -> tembra::Result<ToolSettlement> {
        let (mut state, progress) = self.restore(context, checkpoint).await?;
        let current: Option<swarmie_conversations::Conversation> = self
            .reference()?
            .call(
                context,
                Method::Get.as_str(),
                (progress.checkpoint.head.conversation,),
            )
            .await?;
        if current
            .ok_or_else(|| invalid("conversation is missing"))?
            .head
            != progress.checkpoint.head
        {
            return Err(Error::new(
                Code::Conflict,
                "swarmie.agent_loop.history_changed",
                "history changed before tool settlement",
            ));
        }
        let (calls, next, pending) = match &state.phase {
            crate::Phase::PrepareTools { calls, next } => (calls.clone(), *next, None),
            crate::Phase::ToolPending {
                operation,
                call,
                provider,
                calls,
                next,
            } => {
                context.check()?;
                let recovery = tools
                    .recover(
                        context,
                        ToolExecution {
                            operation: *operation,
                            call,
                            provider: provider.as_ref(),
                        },
                    )
                    .await;
                context.check()?;
                let recovery = match recovery {
                    Ok(recovery) => recovery,
                    Err(_) if acknowledge_unknown => Recovery::Unknown,
                    Err(error) => return Err(error),
                };
                let result = match recovery {
                    Recovery::Completed(result) => result,
                    Recovery::SafeToRetry => {
                        ToolResult::Blocked("not retried after interruption".into())
                    }
                    Recovery::Unknown if acknowledge_unknown => {
                        ToolResult::Failed("outcome unknown; not replayed".into())
                    }
                    Recovery::Unknown => return Ok(ToolSettlement::Unknown(progress.checkpoint)),
                };
                (calls.clone(), *next, Some((call.clone(), result)))
            }
            _ => return Ok(ToolSettlement::Settled(progress.checkpoint)),
        };
        context.check()?;
        for (offset, call) in calls.iter().skip(next).enumerate() {
            let (prepared_call, result) = if offset == 0 {
                pending
                    .clone()
                    .unwrap_or_else(|| (call.clone(), ToolResult::Blocked("not executed".into())))
            } else {
                (call.clone(), ToolResult::Blocked("not executed".into()))
            };
            let result =
                transform_tool_result(result_hook, context, state.turn, &prepared_call, result)
                    .await?;
            state.request.context.messages.push(Message::Tool {
                call: call.id.clone(),
                result,
            });
        }
        state.phase = if state.turn >= state.request.max_turns.get() {
            crate::Phase::Finished {
                reason: FinishReason::TurnLimit,
            }
        } else {
            crate::Phase::PrepareTurn
        };
        state.validate()?;
        let writer = Writer {
            history: self,
            progress: RefCell::new(progress),
            #[cfg(feature = "durable-tasks")]
            execution: None,
        };
        writer.confirm(context, &state).await?;
        Ok(ToolSettlement::Settled(
            writer.progress.into_inner().checkpoint,
        ))
    }
}
