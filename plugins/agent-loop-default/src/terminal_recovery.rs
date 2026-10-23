//! Reconciles a terminal Session decision whose task transition or append reply was lost.

use std::time::Duration;

use swarmie_agent_contracts::StartTurn;
use swarmie_model_contracts::{ContentPart, FinishReason, GenerateResult};
use swarmie_task_contracts::{TaskLookup, TaskOutcome, TaskRecord, TaskTransition};
use tembra::{Context, Result, RuntimeError};

use crate::{
    journal::{self, Journal},
    state::{Checkpoint, decode, invalid, stable},
    workflow::{Engine, completion_value},
};

/// A failure to resolve existing terminal evidence must park the original effect checkpoint;
/// inventing a different ending could conflict with a committed decision under its stable id.
pub(crate) const UNRESOLVED: &str = "agent_terminal_evidence";

impl Engine {
    /// Runs only after duration exhaustion or a phase timeout. The ordinary successful path
    /// performs no extra Session I/O. A fresh child of the live parent gives reconciliation
    /// its own bounded finalization grace after the expired work is physically joined.
    pub(crate) async fn recover_terminal_decision(
        &self,
        task: &TaskRecord,
        request: &StartTurn,
        checkpoint: &Checkpoint,
        context: &Context,
    ) -> Result<Option<TaskTransition>> {
        context.check(context.host_id())?;
        let (cleanup, guard) = context.child_call(context.principal().clone())?;
        let cleanup = Context::from_core(cleanup);
        let work = async {
            let admitted = self
                .admission
                .confirm(&checkpoint.turn(request), &cleanup)
                .await?;
            let current = self
                .tasks
                .with_context(&cleanup)
                .get(TaskLookup {
                    id: task.id.clone(),
                })
                .await?;
            if current.as_ref() != Some(task) {
                return Err(RuntimeError::aborted(
                    "agent_terminal_admission",
                    "turn changed before terminal evidence recovery",
                ));
            }
            let mut journal = Journal::new(admitted, crate::HistoryCommit::Independent);
            self.recorded_terminal_decision(task, request, checkpoint, &mut journal, &cleanup)
                .await
        };
        let timer = self.driver.sleep(Duration::from_secs(1));
        futures::pin_mut!(work, timer);
        let result = match futures::future::select(work, timer).await {
            futures::future::Either::Left((result, _)) => result,
            futures::future::Either::Right(((), work)) => {
                cleanup.cancellation().cancel(RuntimeError::aborted(
                    UNRESOLVED,
                    "terminal evidence recovery grace elapsed",
                ));
                // Uncertain evidence never authorizes another effect or a conflicting ending.
                let _ = work.await;
                Err(RuntimeError::unavailable(
                    UNRESOLVED,
                    "terminal evidence could not be resolved within finalization grace",
                ))
            }
        };
        drop(guard);
        context.check(context.host_id())?;
        result.map_err(|error| {
            if error.code == tembra::ErrorCode::Aborted {
                error
            } else {
                RuntimeError::unavailable(
                    UNRESOLVED,
                    format!("terminal evidence remains unresolved: {}", error.reason),
                )
            }
        })
    }

    async fn recorded_terminal_decision(
        &self,
        task: &TaskRecord,
        request: &StartTurn,
        checkpoint: &Checkpoint,
        journal: &mut Journal,
        context: &Context,
    ) -> Result<Option<TaskTransition>> {
        let turn = checkpoint.turn(request);
        let id = stable(request, "turn-decision");
        let events = journal.history(&self.sessions, context).await?;
        if journal::find(events, &id).is_none() {
            return Ok(None);
        }
        let event = journal::own_event(events, &id, &turn, "swarmie.agent.turn-ended")?;
        let outcome: TaskOutcome = decode(event.payload.clone())?;
        match outcome {
            TaskOutcome::Completed { value } => {
                // A successful folded model root is the only invocation that can have
                // committed completion without first replacing its effect-entry checkpoint.
                if task.phase != "model"
                    || checkpoint.response_event.is_some()
                    || checkpoint.tool_index != 0
                    || checkpoint.step >= request.limits.max_steps
                    || checkpoint.tool_calls > request.limits.max_tool_calls
                    || checkpoint.request_event.as_deref()
                        != Some(
                            stable(request, &format!("model:{}:request", checkpoint.step)).as_str(),
                        )
                {
                    return Err(invalid(
                        "terminal completion differs from the admitted effect checkpoint",
                    ));
                }
                let mut completed = checkpoint.clone();
                completed.response_event = Some(stable(
                    request,
                    &format!("model:{}:confirmed", checkpoint.step),
                ));
                let (call, result) = self
                    .model_observation(request, &completed, journal, context)
                    .await?;
                let GenerateResult::Response { response } = result else {
                    return Err(invalid(
                        "terminal completion has no successful model result",
                    ));
                };
                if !matches!(
                    response.finish_reason,
                    FinishReason::Stop | FinishReason::StopSequence
                ) || response
                    .message
                    .content
                    .iter()
                    .any(|part| matches!(part, ContentPart::ToolCall { .. }))
                {
                    return Err(invalid(
                        "terminal completion has an unfinished model response",
                    ));
                }
                let remaining = request
                    .limits
                    .max_output_tokens
                    .saturating_sub(completed.output_tokens);
                let charged = response.usage.output_tokens.unwrap_or_else(|| {
                    u64::from(call.request.options.max_output_tokens.unwrap_or(u32::MAX))
                });
                if charged > remaining {
                    return Err(invalid(
                        "terminal completion exceeds the admitted output budget",
                    ));
                }
                completed.output_tokens += charged;
                if value != completion_value(request, &completed) {
                    return Err(invalid(
                        "terminal completion counters or response identity differ from committed evidence",
                    ));
                }
                Ok(Some(TaskTransition::Complete { value }))
            }
            TaskOutcome::Failed { error }
                if task.phase == "finish"
                    && checkpoint.ending.as_deref() == Some(error.as_str()) =>
            {
                Ok(Some(TaskTransition::Fail { error }))
            }
            _ => Err(invalid(
                "terminal Session decision differs from the admitted task ending",
            )),
        }
    }
}
