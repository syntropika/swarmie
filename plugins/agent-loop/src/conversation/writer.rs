use super::{
    History,
    checkpoint::{Checkpoint, Phase, Record},
};
use crate::{Message, State, engine::Confirmation, model::invalid};
use futures::{FutureExt, future::LocalBoxFuture};
use std::cell::RefCell;
use swarmie_conversations::{EntryDraft, EntryId, Head, Method, WriteConfirmation, WriteReceipt};
#[cfg(feature = "durable-tasks")]
use tembra::Code;
use tembra::Context;

pub(super) struct Progress {
    pub checkpoint: Checkpoint,
    pub(super) messages: usize,
    pub(super) assistant: Option<EntryId>,
}

pub(super) struct Writer<'a> {
    pub history: &'a History,
    pub progress: RefCell<Progress>,
    #[cfg(feature = "durable-tasks")]
    pub execution: Option<&'a swarmie_durable_tasks::Execution>,
}

impl History {
    async fn append(
        &self,
        context: &Context,
        at: Head,
        entries: Vec<EntryDraft>,
        expected: WriteConfirmation,
    ) -> tembra::Result<Head> {
        let receipt: WriteReceipt = self
            .reference()?
            .call(context, Method::Append.as_str(), (at, entries))
            .await?;
        if receipt.confirmation != expected {
            return Err(invalid("history write used an incompatible storage scope"));
        }
        Ok(receipt.head)
    }
}

impl Writer<'_> {
    /// Build history and checkpoint changes without publishing progress.
    fn prepare(&self, next: &State) -> tembra::Result<(Progress, Vec<EntryDraft>)> {
        let current = self.progress.borrow();
        let mut checkpoint = current.checkpoint.clone();
        let mut assistant = current.assistant;
        let mut entries = Vec::new();
        for message in next
            .request
            .context
            .messages
            .get(current.messages..)
            .ok_or_else(|| invalid("confirmed history shrank"))?
        {
            let entry = EntryDraft::encode(Record::Message(message.clone()))?;
            if matches!(message, Message::Assistant(_)) {
                assistant = Some(entry.id)
            }
            entries.push(entry);
        }
        checkpoint.turn = next.turn;
        checkpoint.usage = next.usage;
        checkpoint.available_tools = next.available_tools.clone();
        checkpoint.phase = match &next.phase {
            crate::Phase::PrepareTurn => Phase::PrepareTurn,
            crate::Phase::ModelPending {
                operation,
                request,
                preparation,
            } => {
                let reused = match current.checkpoint.phase {
                    Phase::ModelPending {
                        operation: pending,
                        request,
                    } if pending == *operation => Some(request),
                    _ => None,
                };
                let request_id = if let Some(id) = reused {
                    id
                } else {
                    let entry = EntryDraft::encode(Record::PreparedRequest {
                        operation: *operation,
                        request: request.clone(),
                        description: preparation.clone(),
                    })?;
                    let id = entry.id;
                    entries.push(entry);
                    id
                };
                Phase::ModelPending {
                    operation: *operation,
                    request: request_id,
                }
            }
            crate::Phase::PrepareTools { next, .. } => Phase::PrepareTools {
                assistant: assistant
                    .ok_or_else(|| invalid("missing assistant history reference"))?,
                next: *next,
            },
            crate::Phase::ToolPending {
                operation,
                call,
                provider,
                calls,
                next,
            } => {
                let reused = match current.checkpoint.phase {
                    Phase::ToolPending {
                        operation: pending,
                        prepared,
                        ..
                    } if pending == *operation => Some(prepared),
                    _ => None,
                };
                let prepared = if let Some(id) = reused {
                    id
                } else if calls.get(*next) == Some(call) && provider.is_none() {
                    None
                } else {
                    let entry = EntryDraft::encode(Record::PreparedTool {
                        operation: *operation,
                        call: call.clone(),
                        provider: provider.clone(),
                    })?;
                    let id = entry.id;
                    entries.push(entry);
                    Some(id)
                };
                Phase::ToolPending {
                    operation: *operation,
                    prepared,
                    assistant: assistant
                        .ok_or_else(|| invalid("missing assistant history reference"))?,
                    next: *next,
                }
            }
            crate::Phase::Finished { reason } => Phase::Finished { reason: *reason },
        };
        if let Some(entry) = entries.last() {
            checkpoint.head.entry = Some(entry.id)
        }
        Ok((
            Progress {
                checkpoint,
                messages: next.request.context.messages.len(),
                assistant,
            },
            entries,
        ))
    }

    /// Publish progress only after the enclosing storage commit succeeds.
    async fn persist(
        &self,
        context: &Context,
        proposed: &Progress,
        entries: Vec<EntryDraft>,
    ) -> tembra::Result<()> {
        let old_head = self.progress.borrow().checkpoint.head;
        let expected_head = proposed.checkpoint.head;
        #[cfg(feature = "durable-tasks")]
        if let Some(execution) = self.execution {
            let saved = serde_json::to_value(&proposed.checkpoint).map_err(invalid)?;
            let committed = if entries.is_empty() {
                execution.commit(context, saved).await
            } else {
                let history = self.history.clone();
                execution
                    .commit_with(context, move |writing| {
                        async move {
                            let head = history
                                .append(&writing, old_head, entries, WriteConfirmation::Staged)
                                .await?;
                            if head != expected_head {
                                return Err(invalid("history returned a different proposed head"));
                            }
                            Ok(saved)
                        }
                        .boxed_local()
                    })
                    .await
            };
            committed.map_err(persistence_error)?;
            return Ok(());
        }
        if !entries.is_empty() {
            let head = self
                .history
                .append(context, old_head, entries, WriteConfirmation::Committed)
                .await?;
            if head != expected_head {
                return Err(invalid("history returned a different confirmed head"));
            }
        }
        Ok(())
    }
}

impl Confirmation for Writer<'_> {
    fn confirm<'a>(
        &'a self,
        context: &'a Context,
        next: &'a State,
    ) -> LocalBoxFuture<'a, tembra::Result<()>> {
        async move {
            let (proposed, entries) = self.prepare(next)?;
            self.persist(context, &proposed, entries).await?;
            self.progress.replace(proposed);
            Ok(())
        }
        .boxed_local()
    }
}

#[cfg(feature = "durable-tasks")]
fn persistence_error(error: tembra::Error) -> tembra::Error {
    if matches!(error.code, Code::Conflict | Code::Unavailable) {
        tembra::Error::new(
            Code::Unavailable,
            crate::FailureReason::RecoveryRequired.as_str(),
            error.to_string(),
        )
    } else {
        error
    }
}
