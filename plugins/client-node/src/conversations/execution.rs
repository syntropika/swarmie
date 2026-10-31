//! Attempt-owned client execution over the existing conversation task adapter.
use super::{
    Running, RunningContext, Session, checkout, projection, tasks,
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
        let input: tasks::Input = serde_json::from_value(execution.input()).map_err(|_| {
            tembra::Error::invalid("conversation.input", "invalid persisted client turn")
        })?;
        let metadata = self.index.get(&self.id)?.ok_or_else(|| {
            tembra::Error::invalid("conversation.index", "the turn is not indexed")
        })?;
        if input.session != self.id
            || context.principal()? != self.principal
            || metadata.principal != self.principal
            || metadata.node != self.node
            || metadata.task != Some(execution.id())
            || metadata.execution_context != input.binding
            || input.request.at.conversation != self.history_id
        {
            return Err(tembra::Error::new(
                tembra::Code::Forbidden,
                "conversation.binding",
                "the task belongs to another conversation or checkout",
            ));
        }
        if let Some(sources) = &input.request.sources {
            sources.validate()?;
            if self.project_configuration.is_none()
                || sources.scope().node != self.node
                || sources.scope().principal != self.principal
                || input
                    .binding
                    .as_ref()
                    .is_none_or(|binding| binding.path != sources.scope().directory)
            {
                return Err(super::failure(
                    "captured sources do not match the admitted checkout",
                ));
            }
        }
        if metadata.stop_requested {
            let checkpoint = execution
                .checkpoint()
                .map(serde_json::from_value)
                .transpose()
                .map_err(|_| {
                    tembra::Error::invalid(
                        "conversation.checkpoint",
                        "invalid stored turn checkpoint",
                    )
                })?;
            let _ = self.index.change(&self.id, |record| {
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
            });
            return Ok(swarmie_durable_tasks::TaskOutcome::Completed(
                tasks::Outcome::StopRequested.value(),
            ));
        }
        if metadata.phase != Phase::Working {
            self.index
                .change(&self.id, |record| record.phase = Phase::Working)?;
        }
        *self.running.borrow_mut() = Some(RunningContext {
            context: context.clone(),
            binding: input.binding.clone(),
            execution: Some(execution.clone()),
            sources: input.request.sources.clone(),
        });
        let _running = Running(&self.running, context.clone());
        let _ = self.refresh(&context).await;
        let scoped_tools = Rc::new(checkout::ScopedTools {
            inner: self.tools.clone(),
            binding: input.binding.clone(),
            authority: self.agent_git_authority.clone(),
            project: input
                .request
                .sources
                .as_ref()
                .map(|_| {
                    super::sources::SkillTools::new(
                        self.tasks.host.clone(),
                        self.id.clone(),
                        self.principal.clone(),
                    )
                })
                .transpose()?,
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
            // The task owns this physical scope, including after observer loss or
            // recovery. Admission failures still use the normal settlement path.
            let scope = self.enter_execution(&context)?;
            let context = scope.as_ref().map_or(&context, |scope| scope.context());
            let protection = scope.as_ref().map_or(
                swarmie_model_provider_ai::ModelProtection::Unprotected,
                |scope| scope.protection(),
            );
            if let Some(binding) = &input.binding {
                checkout::validate(
                    self.git.as_ref(),
                    context,
                    &self.node,
                    &self.principal,
                    binding,
                )
                .await?;
            }
            let model = self
                .providers
                .model_with_protection(
                    &self.principal,
                    &input.account,
                    &input.model,
                    &input.effort,
                    protection,
                )
                .await?;
            let stop = swarmie_ai::StopHandle::new();
            let model = Rc::new(model.with_stop(stop.clone()));
            let agent = Agent::new(model.clone(), scoped_tools.clone())
                .with_progress(progress)
                .with_preparation(Rc::new(
                    self.context_policy(execution.clone(), input.request.at),
                ));
            self.drive_context(context, model, &agent, &execution, input, stop)
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
                    .map_err(|_| {
                        tembra::Error::invalid(
                            "conversation.checkpoint",
                            "invalid confirmed task checkpoint",
                        )
                    })?;
                let stop_requested = self
                    .index
                    .get(&self.id)
                    .ok()
                    .flatten()
                    .is_some_and(|record| record.stop_requested);
                let phase = if !stop_requested && execution.check(&context).is_err() {
                    Phase::Interrupted
                } else if checkpoint
                    .as_ref()
                    .is_some_and(|value| value.has_unfinished_tools())
                {
                    Phase::Unknown
                } else if stop_requested {
                    Phase::Stopped
                } else {
                    Phase::Failed
                };
                (phase, checkpoint)
            }
        };
        // The durable task outcome is authoritative if this presentation write fails.
        let _ = self.index.change(&self.id, |record| {
            record.phase = phase;
            record.checkpoint = checkpoint;
        });
        drop(_running);
        checkout::sync_hold(&self.index, self.git.as_ref(), &self.principal, &self.id).await;
        // Observation cannot replace a confirmed execution outcome.
        let _ = self.refresh(&context).await;
        let _ = self.context_inspect(&context).await;
        Ok(swarmie_durable_tasks::TaskOutcome::Completed(
            tasks::Outcome::Settled { phase }.value(),
        ))
    }

    pub(super) async fn request_stop(&self, context: &Context) -> tembra::Result<()> {
        let compaction = self
            .compaction_running
            .borrow()
            .as_ref()
            .map(|(operation, _)| *operation);
        if let Some(operation) = compaction {
            self.index
                .change(&self.id, |record| record.stop_requested = true)?;
            self.context_stop(context, operation).await?;
            return Ok(());
        }
        if let Some(metadata) = self.index.get(&self.id)?
            && metadata.task.is_some()
        {
            self.index
                .change(&self.id, |record| record.stop_requested = true)?;
        }
        let running = self.running.borrow().clone();
        if let Some(running) = running {
            if let Some(execution) = &running.execution {
                match execution
                    .complete(&running.context, tasks::Outcome::StopRequested.value())
                    .await
                {
                    Ok(()) => {
                        running.context.cancel()?;
                    }
                    Err(_) if execution.check(&running.context).is_err() => {
                        // The persisted stop intent survives lease revocation. The caller
                        // joins the attempt and wakes only this held task to consume it.
                    }
                    Err(error) => return Err(error),
                }
                return Ok(());
            }
            running.context.cancel()?;
        }
        Ok(())
    }
}
