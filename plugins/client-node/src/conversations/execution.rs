//! Attempt-owned client execution over the existing conversation task adapter.
use super::{
    Running, RunningContext, Session, checkout, failure, projection, tasks,
    types::{self, Phase},
};
use std::rc::Rc;
use swarmie_agent_loop::Agent;
use swarmie_providers::Progress;
use tembra::Context;

impl Session {
    pub(super) async fn execute_task(
        &self,
        context: Context,
        execution: swarmie_durable_tasks::Execution,
    ) -> tembra::Result<swarmie_durable_tasks::TaskOutcome> {
        execution.check(&context)?;
        let input: tasks::Input = serde_json::from_value(execution.input())
            .map_err(|_| failure("invalid persisted client turn"))?;
        let metadata = self
            .index
            .get(&self.id)?
            .ok_or_else(|| failure("the turn is not indexed"))?;
        if input.session != self.id
            || context.principal()? != self.principal
            || metadata.task != Some(execution.id())
            || metadata.execution_context != input.binding
            || input.request.at.conversation != self.history_id
        {
            return Err(failure(
                "the task belongs to another conversation or checkout",
            ));
        }
        if metadata.stop_requested {
            let checkpoint = execution
                .checkpoint()
                .map(serde_json::from_value)
                .transpose()
                .map_err(|_| failure("invalid stored turn checkpoint"))?;
            self.index.change(&self.id, |record| {
                record.phase = if checkpoint.as_ref().is_some_and(
                    |value: &swarmie_agent_loop::conversation::Checkpoint| {
                        value.has_unfinished_tools()
                    },
                ) {
                    Phase::Unknown
                } else {
                    Phase::Stopped
                };
                record.checkpoint = checkpoint;
            })?;
            return Ok(swarmie_durable_tasks::TaskOutcome::Completed(
                tasks::Outcome::StopRequested.value(),
            ));
        }
        self.index
            .change(&self.id, |record| record.phase = Phase::Working)?;
        *self.running.borrow_mut() = Some(RunningContext {
            context: context.clone(),
            binding: input.binding.clone(),
            execution: Some(execution.clone()),
        });
        let _running = Running(&self.running, context.clone());
        let _ = self.refresh(&context).await;
        let scoped_tools = Rc::new(checkout::ScopedTools {
            inner: self.tools.clone(),
            binding: input.binding.clone(),
            authority: self.agent_git_authority.clone(),
        });
        let state = self.state.clone();
        let progress = Progress::new(move |context, preview| {
            let _ = state.update(context, |current| {
                let mut next = current.clone();
                if next
                    .messages
                    .last()
                    .is_some_and(|message| message.id == "preview")
                {
                    next.messages.pop();
                }
                next.messages.push(types::Message {
                    id: "preview".into(),
                    role: "assistant".into(),
                    text: preview.content,
                    truncated: false,
                });
                Ok(projection::bound(next))
            });
        });
        let result = async {
            if let Some(binding) = &input.binding {
                checkout::validate(
                    self.git.as_ref(),
                    &context,
                    &self.node,
                    &self.principal,
                    binding,
                )
                .await?;
            }
            let model = self
                .providers
                .model(&self.principal, &input.account, &input.model, &input.effort)
                .await?;
            let agent = Agent::new(Rc::new(model), scoped_tools.clone()).with_progress(progress);
            swarmie_agent_loop::conversation::durable::execute_request(
                &self.agent_history,
                &context,
                &agent,
                &execution,
                input.request,
            )
            .await
        }
        .await;
        let (phase, checkpoint) = match result {
            Ok(_) => (Phase::Completed, None),
            Err(_) => {
                let checkpoint: Option<swarmie_agent_loop::conversation::Checkpoint> = execution
                    .checkpoint()
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(|_| failure("invalid confirmed task checkpoint"))?;
                let phase = if checkpoint
                    .as_ref()
                    .is_some_and(|value| value.has_unfinished_tools())
                {
                    Phase::Unknown
                } else if context.check().is_err() {
                    Phase::Stopped
                } else {
                    Phase::Failed
                };
                (phase, checkpoint)
            }
        };
        // Physical settlement always records the outcome, including observer cancellation.
        self.index.change(&self.id, |record| {
            record.phase = phase;
            record.checkpoint = checkpoint;
        })?;
        drop(_running);
        checkout::sync_hold(&self.index, self.git.as_ref(), &self.principal, &self.id).await;
        // Observation cannot replace a confirmed execution outcome.
        let _ = self.refresh(&context).await;
        Ok(swarmie_durable_tasks::TaskOutcome::Completed(
            tasks::Outcome::Settled { phase }.value(),
        ))
    }

    pub(super) async fn request_stop(&self) -> tembra::Result<()> {
        if let Some(metadata) = self.index.get(&self.id)?
            && metadata.task.is_some()
        {
            self.index
                .change(&self.id, |record| record.stop_requested = true)?;
        }
        let running = self.running.borrow().clone();
        if let Some(running) = running {
            if let Some(execution) = &running.execution {
                loop {
                    match execution
                        .complete(&running.context, tasks::Outcome::StopRequested.value())
                        .await
                    {
                        Err(error) if error.reason == "swarmie.tasks.busy" => {
                            tokio::task::yield_now().await
                        }
                        outcome => {
                            let _ = running.context.cancel();
                            return outcome;
                        }
                    }
                }
            }
            running.context.cancel()?;
        }
        Ok(())
    }
}
