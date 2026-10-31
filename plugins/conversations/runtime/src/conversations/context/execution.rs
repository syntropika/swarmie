use super::{Progress, Work, configuration};
use crate::conversations::{Session, failure, types::ExecutionContext};
use futures::FutureExt;
use std::rc::Rc;
use swarmie_agent_loop::{
    Agent,
    conversation::{Checkpoint as TurnCheckpoint, Record},
};
use swarmie_context::{Activation, ArtifactRef, Candidate, Checkpoint, Plan, Strategy};
use swarmie_conversations::{EntryDraft, Head, WriteConfirmation};
use swarmie_durable_tasks::{Execution, TaskOutcome};
use swarmie_providers::{Model, OperationId, Recovery};
use tembra::{Context, Host, Orb, Service};

pub(in crate::conversations) fn instructions(binding: Option<&ExecutionContext>) -> String {
    binding.map_or_else(String::new, |binding| format!(
        "Use the captured Git checkout at {} on node {}. Git tools must target its canonical root/Git-directory identity {}. Other provider tools retain their own resource scope.",
        binding.path, binding.node_id, binding.identity
    ))
}

enum Owner {
    Manual(Box<Progress>),
    Turn(Box<TurnCheckpoint>),
}

impl Owner {
    fn checkpoint(&self) -> Checkpoint {
        match self {
            Self::Manual(value) => value.checkpoint.clone(),
            Self::Turn(value) => value.context_progress().clone(),
        }
    }
    fn head(&self) -> Head {
        match self {
            Self::Manual(value) => value.head,
            Self::Turn(value) => value.head(),
        }
    }
    fn set(&mut self, checkpoint: Checkpoint) -> tembra::Result<()> {
        match self {
            Self::Manual(value) => value.checkpoint = checkpoint,
            Self::Turn(value) => **value = (**value).clone().with_context_progress(checkpoint)?,
        }
        Ok(())
    }
    fn encoded(&self) -> tembra::Result<serde_json::Value> {
        match self {
            Self::Manual(value) => serde_json::to_value(value),
            Self::Turn(value) => serde_json::to_value(value),
        }
        .map_err(|_| failure("context progress cannot be encoded"))
    }
    async fn persist(&self, context: &Context, execution: &Execution) -> tembra::Result<()> {
        execution.commit(context, self.encoded()?).await
    }
}

pub(in crate::conversations) fn handler(host: Host) -> Orb {
    Orb::new(swarmie_context::durable::SERVICE, 1).service(
        Service::new(swarmie_context::durable::SERVICE, move |_| Ok(host.clone())).method(
            swarmie_durable_tasks::TaskMethod::Execute.as_str(),
            |host, context, execution: Execution| async move {
                execution.check(&context)?;
                let input: Work = serde_json::from_value(execution.input())
                    .map_err(|_| failure("context input is invalid"))?;
                let session = host.get(
                    crate::conversations::SERVICE,
                    vec![input.session],
                    &context.principal()?,
                )?;
                let result: tembra::Result<TaskOutcome> = session
                    .call(&context, "execute_compaction", execution)
                    .await;
                let _ = session.release();
                result
            },
        ),
    )
}

impl Session {
    pub(in crate::conversations) fn context_policy(
        &self,
        execution: Execution,
        initial: Head,
    ) -> swarmie_context::BudgetPolicy {
        let state = self.state.clone();
        swarmie_context::BudgetPolicy::default().observe(move |context, prepared, decision| {
            let at = execution
                .checkpoint()
                .and_then(|value| serde_json::from_value::<TurnCheckpoint>(value).ok())
                .map_or(initial, |value| value.head());
            let inspection = swarmie_context::Inspection {
                at,
                prepared: prepared.clone(),
                decision: decision.clone(),
                stale: false,
            };
            inspection.validate()?;
            state.update(context, |value| {
                let mut next = value.clone();
                next.context.inspection = Some(inspection.clone());
                next.context.unavailable = None;
                next.context.can_compact = false;
                Ok(crate::conversations::projection::bound(next))
            })?;
            Ok(())
        })
    }

    async fn persist_context(
        &self,
        context: &Context,
        execution: &Execution,
        owner: &mut Owner,
    ) -> tembra::Result<()> {
        owner.persist(context, execution).await?;
        // Re-read after the asynchronous commit so Stop cannot lose its fence.
        let mut ledger = self.context_ledger()?;
        let mut checkpoint = owner.checkpoint();
        if let Some(stopped) = ledger
            .checkpoint
            .compaction()
            .filter(|work| work.stop_requested)
            && checkpoint
                .compaction()
                .is_some_and(|work| work.operation == stopped.operation)
        {
            checkpoint.stop(stopped.operation)?;
        }
        owner.set(checkpoint.clone())?;
        ledger.checkpoint = checkpoint.clone();
        self.save_context_ledger(ledger.clone())?;
        let mut snapshot = self.state.value()?.context.clone();
        snapshot.checkpoint = checkpoint;
        snapshot.fence = Some(ledger.fence);
        snapshot.can_compact = false;
        if let Some(inspection) = &mut snapshot.inspection {
            inspection.stale = true;
        }
        // Durable task confirmation owns progress; an inspector cannot replace it.
        let _ = self.publish_context(context, snapshot);
        Ok(())
    }

    async fn run_compaction(
        &self,
        context: &Context,
        execution: &Execution,
        model: &swarmie_providers::ai::AiModel,
        work: &Work,
        owner: &mut Owner,
    ) -> tembra::Result<()> {
        let operation = work.request.operation;
        if configuration(model, &work.instructions, &work.tools)? != work.request.plan.configuration
        {
            return Err(failure(
                "the captured compaction model or contributions changed before dispatch",
            ));
        }
        let mut checkpoint = owner.checkpoint();
        if checkpoint
            .compaction()
            .is_none_or(|value| value.operation != operation)
        {
            checkpoint.request(swarmie_context::Request {
                operation,
                plan: work.request.plan.clone(),
                required: work.required,
            })?;
            owner.set(checkpoint.clone())?;
            self.persist_context(context, execution, owner).await?;
        }
        let phase = checkpoint
            .compaction()
            .ok_or_else(|| failure("context operation is missing"))?
            .phase
            .clone();
        if matches!(
            phase,
            swarmie_context::Phase::Dispatched | swarmie_context::Phase::Unknown
        ) {
            checkpoint.unknown(operation)?;
            owner.set(checkpoint)?;
            self.persist_context(context, execution, owner).await?;
            return Err(failure(
                "compaction dispatch requires recognition; it will not be replayed",
            ));
        }
        if phase == swarmie_context::Phase::Planned {
            let source_head: Head = work.request.plan.through.into();
            let source = self
                .projected_context(
                    context,
                    source_head,
                    work.instructions.clone(),
                    &work.initial,
                    work.request.plan.configuration,
                )
                .await?;
            let prepared = match swarmie_context::textual::prepare(
                model, context, operation, &source, 0,
            )
            .await
            {
                Ok(value) => value,
                Err(error) => {
                    checkpoint.refuse(operation, "The complete summary source cannot be prepared within the selected model budget".into())?;
                    owner.set(checkpoint)?;
                    self.persist_context(context, execution, owner).await?;
                    return Err(error);
                }
            };
            let ledger = self.context_ledger()?;
            if ledger.fence != work.request.fence
                || ledger
                    .checkpoint
                    .compaction()
                    .is_some_and(|value| value.operation == operation && value.stop_requested)
            {
                checkpoint.stop(operation)?;
                owner.set(checkpoint)?;
                self.persist_context(context, execution, owner).await?;
                return Err(failure("compaction was stopped before dispatch"));
            }
            checkpoint.dispatch(operation)?;
            owner.set(checkpoint.clone())?;
            self.persist_context(context, execution, owner).await?;
            let response = match prepared.execute().await {
                Ok(value) => value,
                Err(error) => {
                    if error.reason == "swarmie.models.ai.not_dispatched" {
                        checkpoint.recognize(operation, Recovery::SafeToRetry)?;
                        checkpoint.refuse(
                            operation,
                            "Model admission refused before physical dispatch".into(),
                        )?;
                    } else {
                        checkpoint.unknown(operation)?;
                    }
                    if self
                        .context_ledger()?
                        .checkpoint
                        .compaction()
                        .is_some_and(|value| value.operation == operation && value.stop_requested)
                    {
                        checkpoint.stop(operation)?;
                    }
                    owner.set(checkpoint)?;
                    self.persist_context(context, execution, owner).await?;
                    return Err(error);
                }
            };
            let usable = swarmie_context::textual::result(response.clone()).is_ok();
            let original_head = owner.head();
            let at = self
                .history
                .get(context, self.history_id)
                .await?
                .ok_or_else(|| failure("conversation history is missing"))?
                .head;
            let advanced_turn = matches!(owner, Owner::Turn(_)) && at != original_head;
            let entry = EntryDraft::encode(Record::ContextResult {
                operation,
                plan: work.request.plan.clone(),
                response,
            })?;
            let head = Head {
                conversation: at.conversation,
                entry: Some(entry.id),
            };
            let artifact = ArtifactRef::new(head, at.entry)?;
            if self
                .context_ledger()?
                .checkpoint
                .compaction()
                .is_some_and(|value| value.operation == operation && value.stop_requested)
            {
                checkpoint.stop(operation)?;
            }
            checkpoint.recognize(
                operation,
                Recovery::Completed(Candidate {
                    operation,
                    plan: work.request.plan.clone(),
                    artifact,
                }),
            )?;
            if (!usable || advanced_turn)
                && checkpoint.compaction().is_some_and(|value| {
                    matches!(value.phase, swarmie_context::Phase::Candidate { .. })
                })
            {
                checkpoint.reject(
                    operation,
                    Candidate {
                        operation,
                        plan: work.request.plan.clone(),
                        artifact,
                    },
                    if advanced_turn {
                        "History advanced outside the paused turn"
                    } else {
                        "The provider returned no usable textual checkpoint"
                    }
                    .into(),
                )?;
            }
            owner.set(checkpoint.clone())?;
            if let Owner::Manual(value) = owner {
                value.head = head;
            }
            let saved = owner.encoded()?;
            let history = self.history.clone();
            execution
                .commit_with(context, move |writing| {
                    async move {
                        let receipt = history.append(&writing, at, vec![entry]).await?;
                        if receipt.confirmation != WriteConfirmation::Staged || receipt.head != head
                        {
                            return Err(failure(
                                "context result was not staged on the task storage owner",
                            ));
                        }
                        Ok(saved)
                    }
                    .boxed_local()
                })
                .await?;
        }
        let mut checkpoint = owner.checkpoint();
        let Some(compaction) = checkpoint.compaction().cloned() else {
            return Err(failure("context operation is missing"));
        };
        if let swarmie_context::Phase::Candidate { artifact } = compaction.phase {
            let projection = swarmie_context::Projection {
                operation,
                plan: compaction.plan.clone(),
                artifact,
            };
            let current_head = self
                .history
                .get(context, self.history_id)
                .await?
                .ok_or_else(|| failure("conversation history is missing"))?
                .head;
            let source = self
                .agent_history
                .context_with_projection(
                    context,
                    current_head,
                    work.instructions.clone(),
                    projection.clone(),
                )
                .await?;
            let request = swarmie_providers::ModelRequest {
                context: super::source_view(source, work.sources.clone())?,
                tools: work.tools.clone(),
            };
            let prepared = model
                .prepare(context, OperationId::new(), request, None)
                .await?;
            let fits = !swarmie_context::Budget::calculate(prepared.description(), 0)?
                .requires_compaction();
            let ledger = self.context_ledger()?;
            if ledger.fence != work.request.fence
                || ledger
                    .checkpoint
                    .compaction()
                    .is_some_and(|value| value.operation == operation && value.stop_requested)
            {
                checkpoint.stop(operation)?;
                owner.set(checkpoint)?;
                self.persist_context(context, execution, owner).await?;
                return Err(failure("compaction was stopped before activation"));
            }
            if !fits {
                checkpoint.reject(
                    operation,
                    Candidate {
                        operation,
                        plan: compaction.plan,
                        artifact,
                    },
                    "The recognized summary does not fit the next prepared request".into(),
                )?;
                owner.set(checkpoint)?;
                self.persist_context(context, execution, owner).await?;
                return Err(failure(
                    "recognized compaction result does not fit; it will not be regenerated automatically",
                ));
            }
            checkpoint.activate(
                operation,
                Activation {
                    head: current_head,
                    configuration: configuration(model, &work.instructions, &work.tools)?,
                    prefix_is_visible: true,
                    execution_is_safe: true,
                    request_fits: true,
                },
            )?;
            if let Owner::Turn(value) = owner {
                **value = self
                    .agent_history
                    .adopt_projection(context, (**value).clone(), projection)
                    .await?;
            }
            owner.set(checkpoint)?;
            self.persist_context(context, execution, owner).await?;
        }
        let mut ledger = self.context_ledger()?;
        ledger.checkpoint = owner.checkpoint();
        let activated = ledger
            .checkpoint
            .compaction()
            .is_some_and(|value| value.phase == swarmie_context::Phase::Activated);
        self.save_context_ledger(ledger)?;
        if !activated {
            return Err(failure(
                "the context result is retained but was not activated",
            ));
        }
        Ok(())
    }

    pub(in crate::conversations) async fn execute_compaction(
        &self,
        context: &Context,
        execution: Execution,
    ) -> tembra::Result<TaskOutcome> {
        execution.check(context)?;
        let work: Work = serde_json::from_value(execution.input())
            .map_err(|_| failure("context task input is invalid"))?;
        if work.session != self.id || context.principal()? != self.principal {
            return Err(failure("context task belongs to another conversation"));
        }
        let admitted = self.context_ledger()?;
        if admitted.task != Some(execution.id())
            || admitted.automatic
            || !admitted.checkpoint.compaction().is_some_and(|value| {
                value.operation == work.request.operation && value.plan == work.request.plan
            })
        {
            return Err(failure(
                "context task has no matching conversation admission",
            ));
        }
        let mut owner = Owner::Manual(Box::new(match execution.checkpoint() {
            Some(value) => serde_json::from_value(value)
                .map_err(|_| failure("context task progress is invalid"))?,
            None => Progress {
                head: work.head,
                checkpoint: work.initial.clone(),
            },
        }));
        let stop = swarmie_ai::StopHandle::new();
        *self.compaction_running.borrow_mut() = Some((work.request.operation, stop.clone()));
        let result = async {
            let scope = self.enter_execution(context)?;
            let protection = scope.as_ref().map_or(
                swarmie_providers::ai::ModelProtection::Unprotected,
                |value| value.protection(),
            );
            let model = self
                .providers
                .model_with_protection(
                    &self.principal,
                    &work.account,
                    &work.model,
                    &work.effort,
                    protection,
                )
                .await?
                .with_stop(stop);
            let model_context = scope.as_ref().map_or(context, |value| value.context());
            self.run_compaction(model_context, &execution, &model, &work, &mut owner)
                .await
        }
        .await;
        self.compaction_running.borrow_mut().take();
        let mut ledger = self.context_ledger()?;
        ledger.checkpoint = owner.checkpoint();
        self.save_context_ledger(ledger)?;
        let _ = self.context_inspect(context).await;
        result?;
        Ok(TaskOutcome::Completed(owner.encoded()?))
    }

    pub(in crate::conversations) async fn drive_context(
        &self,
        context: &Context,
        model: Rc<swarmie_providers::ai::AiModel>,
        agent: &Agent,
        execution: &Execution,
        input: crate::conversations::tasks::Input,
        stop: swarmie_ai::StopHandle,
    ) -> tembra::Result<swarmie_agent_loop::Output> {
        let admission_fence = input.context_fence;
        let input = input.request;
        loop {
            if let Some(value) = execution.checkpoint() {
                let checkpoint: TurnCheckpoint = serde_json::from_value(value)
                    .map_err(|_| failure("turn checkpoint is invalid"))?;
                if checkpoint
                    .context_progress()
                    .compaction()
                    .is_some_and(|work| {
                        matches!(
                            work.phase,
                            swarmie_context::Phase::Dispatched | swarmie_context::Phase::Unknown
                        )
                    })
                {
                    return Err(failure(
                        "an interrupted context dispatch must be recognized before continuing",
                    ));
                }
            }
            if let Some(value) = execution.checkpoint() {
                let checkpoint: TurnCheckpoint = serde_json::from_value(value)
                    .map_err(|_| failure("turn checkpoint is invalid"))?;
                if let Some(current) = checkpoint
                    .context_progress()
                    .compaction()
                    .filter(|value| {
                        matches!(
                            value.phase,
                            swarmie_context::Phase::Planned
                                | swarmie_context::Phase::Candidate { .. }
                        )
                    })
                    .cloned()
                {
                    let fence = checkpoint.context_fence().ok_or_else(|| {
                        failure("the original compaction admission fence is missing")
                    })?;
                    let metadata = self
                        .index
                        .get(&self.id)?
                        .ok_or_else(|| failure("conversation metadata is missing"))?;
                    let work = Work {
                        session: self.id.clone(),
                        account: metadata.account,
                        model: metadata.model,
                        effort: metadata.effort,
                        instructions: input.instructions.clone(),
                        sources: input.sources.clone(),
                        tools: input.tools.clone(),
                        head: checkpoint.head(),
                        initial: checkpoint.context_progress().clone(),
                        required: current.required,
                        request: swarmie_context::durable::Input {
                            operation: current.operation,
                            fence,
                            plan: current.plan,
                        },
                    };
                    let mut owner = Owner::Turn(Box::new(checkpoint));
                    *self.compaction_running.borrow_mut() = Some((current.operation, stop.clone()));
                    let result = self
                        .run_compaction(context, execution, &model, &work, &mut owner)
                        .await;
                    self.compaction_running.borrow_mut().take();
                    result?;
                    continue;
                }
            }
            let result = swarmie_agent_loop::conversation::durable::execute_request(
                &self.agent_history,
                context,
                agent,
                execution,
                input.clone(),
            )
            .await;
            if !result
                .as_ref()
                .err()
                .is_some_and(swarmie_context::compaction_required)
            {
                return result;
            }
            let mut ledger = self.observed_context(context).await?;
            if ledger.fence != admission_fence {
                return Err(failure("the original turn admission was stopped"));
            }
            let checkpoint = match execution.checkpoint() {
                Some(value) => serde_json::from_value::<TurnCheckpoint>(value)
                    .map_err(|_| failure("paused turn checkpoint is invalid"))?,
                None => TurnCheckpoint::from_request(input.clone()),
            };
            let through = self.context_prefix(context, checkpoint.head()).await?;
            let configuration = configuration(&model, &input.instructions, &input.tools)?;
            if ledger.checkpoint.compaction().is_some_and(|work| {
                work.plan.through == through && work.plan.configuration == configuration
            }) {
                return Err(failure(
                    "this fixed prefix was already compacted or rejected; no automatic regeneration is allowed",
                ));
            }
            let operation = OperationId::new();
            let plan = Plan {
                through,
                previous: ledger
                    .checkpoint
                    .active_for(configuration)
                    .map(|value| value.artifact.entry()),
                configuration,
                strategy: Strategy::Textual {
                    model: configuration,
                },
            };
            let initial = ledger.checkpoint.clone();
            ledger.checkpoint.request(swarmie_context::Request {
                operation,
                plan: plan.clone(),
                required: true,
            })?;
            ledger.task = Some(execution.id());
            ledger.automatic = true;
            ledger.operation_fence = Some(admission_fence);
            let metadata = self
                .index
                .get(&self.id)?
                .ok_or_else(|| failure("conversation metadata is unavailable"))?;
            let work = Work {
                session: self.id.clone(),
                account: metadata.account,
                model: metadata.model,
                effort: metadata.effort,
                instructions: input.instructions.clone(),
                sources: input.sources.clone(),
                tools: input.tools.clone(),
                head: checkpoint.head(),
                initial,
                required: true,
                request: swarmie_context::durable::Input {
                    operation,
                    fence: admission_fence,
                    plan,
                },
            };
            self.save_context_ledger(ledger.clone())?;
            *self.compaction_running.borrow_mut() = Some((operation, stop.clone()));
            let mut owner = Owner::Turn(Box::new(
                checkpoint.with_context_admission(ledger.checkpoint, admission_fence)?,
            ));
            let result = self
                .run_compaction(context, execution, &model, &work, &mut owner)
                .await;
            self.compaction_running.borrow_mut().take();
            result?;
        }
    }
}
