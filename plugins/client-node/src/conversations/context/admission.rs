use super::{Ledger, Progress, Work, configuration};
use crate::conversations::{Session, failure};
use swarmie_context::{Admission, CompactionRequest, Inspection, Receipt, Snapshot, Strategy};
use swarmie_durable_tasks::{CommitOutcome, TaskChange, TaskId, TaskSpec};
use swarmie_providers::{Model, OperationId, PreparationPolicy};
use tembra::Context;

fn task_id(operation: OperationId) -> tembra::Result<TaskId> {
    let id = uuid::Uuid::new_v5(&operation.as_uuid(), b"swarmie.context.task");
    serde_json::from_value(serde_json::Value::String(id.to_string()))
        .map_err(|_| failure("context task identity is invalid"))
}

impl Session {
    pub(in crate::conversations) async fn observed_context(
        &self,
        context: &Context,
    ) -> tembra::Result<Ledger> {
        self.tasks
            .storage
            .owner()
            .check()
            .map_err(|error| failure(&error.to_string()))?;
        let mut ledger = self.context_ledger()?;
        if let Some(id) = ledger.task
            && let Some(task) = self.tasks.observe(context, id).await?
        {
            if task.task.principal != self.principal {
                return Err(failure("context task belongs to another principal"));
            }
            if let Some(value) = task.task.checkpoint {
                let checkpoint = if ledger.automatic {
                    serde_json::from_value::<swarmie_agent_loop::conversation::Checkpoint>(value)
                        .map_err(|_| failure("context turn checkpoint is invalid"))?
                        .context_progress()
                        .clone()
                } else {
                    serde_json::from_value::<Progress>(value)
                        .map_err(|_| failure("context checkpoint is invalid"))?
                        .checkpoint
                };
                if checkpoint.compaction().is_some() {
                    let stopped = ledger
                        .checkpoint
                        .compaction()
                        .filter(|work| work.stop_requested)
                        .map(|work| work.operation);
                    ledger.checkpoint = checkpoint;
                    if let Some(operation) = stopped
                        && ledger
                            .checkpoint
                            .compaction()
                            .is_some_and(|work| work.operation == operation)
                    {
                        ledger.checkpoint.stop(operation)?;
                    }
                }
            }
        }
        Ok(ledger)
    }

    pub(super) fn publish_context(
        &self,
        context: &Context,
        snapshot: Snapshot,
    ) -> tembra::Result<Snapshot> {
        snapshot.validate()?;
        self.state.update(context, |value| {
            let mut next = value.clone();
            next.context = snapshot.clone();
            Ok(super::super::projection::bound(next))
        })?;
        Ok(snapshot)
    }

    pub(in crate::conversations) async fn context_inspect(
        &self,
        context: &Context,
    ) -> tembra::Result<Snapshot> {
        context.check()?;
        if context.principal()? != self.principal {
            return Err(failure("context principal is invalid"));
        }
        let Some(metadata) = self.index.get(&self.id)? else {
            return self.publish_context(
                context,
                Snapshot {
                    unavailable: Some("Start a conversation before inspecting its context".into()),
                    ..Snapshot::default()
                },
            );
        };
        let ledger = self.observed_context(context).await?;
        let mut snapshot = Snapshot {
            checkpoint: ledger.checkpoint.clone(),
            fence: Some(ledger.fence),
            inspection: self.state.value()?.context.inspection.clone(),
            can_compact: false,
            unavailable: None,
        };
        let pending = ledger
            .checkpoint
            .compaction()
            .is_some_and(|work| work.is_pending());
        if pending || self.running.borrow().is_some() || self.compaction_running.borrow().is_some()
        {
            return self.publish_context(context, snapshot);
        }
        let prepared = async {
            let history = self
                .history
                .get(context, self.history_id)
                .await?
                .ok_or_else(|| failure("conversation history is unavailable"))?;
            let sources = self.captured_sources(context, &metadata).await?;
            let instructions = super::execution::instructions(metadata.execution_context.as_ref());
            let tools =
                self.context_tools(metadata.execution_context.as_ref(), sources.as_ref())?;
            let model = self
                .providers
                .model_with_protection(
                    &self.principal,
                    &metadata.account,
                    &metadata.model,
                    &metadata.effort,
                    swarmie_providers::ai::ModelProtection::Unprotected,
                )
                .await?;
            let configuration = configuration(&model, &instructions, &tools)?;
            let mut request = self
                .context_request(
                    context,
                    history.head,
                    instructions,
                    tools,
                    &ledger.checkpoint,
                    configuration,
                )
                .await?;
            request.context = super::source_view(request.context, sources.clone())?;
            let prepared = model
                .prepare(context, OperationId::new(), request.clone(), None)
                .await?;
            let policy = swarmie_context::BudgetPolicy::default();
            let fixed =
                if let Some(request) = policy.fixed_request(&request, prepared.description())? {
                    Some(
                        model
                            .prepare(context, OperationId::new(), request, None)
                            .await?,
                    )
                } else {
                    None
                };
            let decision = policy.decision(
                prepared.description(),
                fixed.as_ref().map(|value| value.description()),
            )?;
            let inspection = Inspection {
                at: history.head,
                prepared: prepared.description().clone(),
                decision,
                stale: false,
            };
            inspection.validate()?;
            let can_compact =
                self.context_prefix(context, history.head)
                    .await
                    .is_ok_and(|through| {
                        !ledger
                            .checkpoint
                            .active_for(configuration)
                            .is_some_and(|active| active.plan.through == through)
                    });
            Ok::<_, tembra::Error>((inspection, can_compact))
        }
        .await;
        match prepared {
            Ok((inspection, can_compact)) => {
                snapshot.inspection = Some(inspection);
                snapshot.can_compact = can_compact;
            }
            Err(_) => {
                snapshot.unavailable = Some(
                    "Qualified model preparation is unavailable; no request will be dispatched"
                        .into(),
                )
            }
        }
        self.publish_context(context, snapshot)
    }

    async fn context_receipt(
        &self,
        context: &Context,
        requested: OperationId,
        operation: OperationId,
        admission: Admission,
    ) -> tembra::Result<Receipt> {
        let snapshot = self.context_inspect(context).await?;
        let receipt = Receipt {
            requested,
            operation,
            admission,
            snapshot,
        };
        receipt.validate()?;
        Ok(receipt)
    }

    pub(in crate::conversations) async fn context_compact(
        &self,
        context: &Context,
        request: CompactionRequest,
    ) -> tembra::Result<Receipt> {
        context.check()?;
        if context.principal()? != self.principal {
            return Err(failure("context principal is invalid"));
        }
        let id = task_id(request.operation)?;
        if let Some(existing) = self.tasks.observe(context, id).await? {
            let work: Work = serde_json::from_value(existing.task.spec.input)
                .map_err(|_| failure("context task input is invalid"))?;
            if work.session != self.id
                || work.request.operation != request.operation
                || existing.task.spec.definition != swarmie_context::durable::definition()
                || work.request.fence != request.fence
                || existing.task.principal != self.principal
            {
                return Err(failure(
                    "context operation identity was reused with another request",
                ));
            }
            return self
                .context_receipt(
                    context,
                    request.operation,
                    work.request.operation,
                    Admission::Recognized,
                )
                .await;
        }
        let ledger = self.observed_context(context).await?;
        if ledger
            .checkpoint
            .compaction()
            .is_some_and(|work| work.operation == request.operation && !work.is_pending())
        {
            if ledger.operation_fence != Some(request.fence) {
                return Err(failure(
                    "context operation identity was reused with another fence",
                ));
            }
            return self
                .context_receipt(
                    context,
                    request.operation,
                    request.operation,
                    Admission::Recognized,
                )
                .await;
        }
        if let Some(current) = ledger
            .checkpoint
            .compaction()
            .filter(|work| work.is_pending())
        {
            if ledger.fence != request.fence {
                return Err(failure("context admission fence changed"));
            }
            // The metadata association precedes schedulable task creation.
            // An absent task is authoritative only after the serial admission
            // owner has settled; a concurrent creation must still be joined.
            if !ledger.automatic
                && let Ok(_serial) = self.serial.try_lock()
                && let Some(task) = ledger.task
                && self.tasks.observe(context, task).await?.is_none()
            {
                if current.phase != swarmie_context::Phase::Planned {
                    self.tasks.storage.owner().require_recovery();
                    return Err(failure(
                        "context task evidence is missing after dispatch intent",
                    ));
                }
                let operation = current.operation;
                let mut ledger = ledger.clone();
                ledger.checkpoint.refuse(
                    operation,
                    "Compaction admission ended without creating a durable task".into(),
                )?;
                self.save_context_ledger(ledger)?;
                return self.context_receipt(context, request.operation, operation, Admission::Refused { reason: "Compaction was not durably admitted; no model request was dispatched".into() }).await;
            }
            return self
                .context_receipt(
                    context,
                    request.operation,
                    current.operation,
                    Admission::Joined,
                )
                .await;
        }
        let _serial = self.serial.lock().await;
        self.wait_turn(context).await?;
        let mut ledger = self.observed_context(context).await?;
        if ledger.fence != request.fence {
            return Err(failure("context admission fence changed before admission"));
        }
        if let Some(current) = ledger
            .checkpoint
            .compaction()
            .filter(|work| work.is_pending())
        {
            return self
                .context_receipt(
                    context,
                    request.operation,
                    current.operation,
                    Admission::Joined,
                )
                .await;
        }
        let metadata = self
            .index
            .get(&self.id)?
            .ok_or_else(|| failure("conversation metadata is unavailable"))?;
        let head = self
            .history
            .get(context, self.history_id)
            .await?
            .ok_or_else(|| failure("conversation history is unavailable"))?
            .head;
        let sources = self.captured_sources(context, &metadata).await?;
        let instructions = super::execution::instructions(metadata.execution_context.as_ref());
        let tools = self.context_tools(metadata.execution_context.as_ref(), sources.as_ref())?;
        let model = self
            .providers
            .model_with_protection(
                &self.principal,
                &metadata.account,
                &metadata.model,
                &metadata.effort,
                swarmie_providers::ai::ModelProtection::Unprotected,
            )
            .await?;
        let configuration = configuration(&model, &instructions, &tools)?;
        let through = self.context_prefix(context, head).await?;
        if ledger
            .checkpoint
            .active_for(configuration)
            .is_some_and(|active| active.plan.through == through)
        {
            return Err(failure("the selected prefix is already compacted"));
        }
        let plan = swarmie_context::Plan {
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
        let required = ledger
            .checkpoint
            .compaction()
            .is_some_and(|work| work.required && work.phase != swarmie_context::Phase::Activated);
        ledger.checkpoint.request(swarmie_context::Request {
            operation: request.operation,
            plan: plan.clone(),
            required,
        })?;
        ledger.task = Some(id);
        ledger.automatic = false;
        ledger.operation_fence = Some(request.fence);
        let work = Work {
            session: self.id.clone(),
            account: metadata.account,
            model: metadata.model,
            effort: metadata.effort,
            instructions,
            sources: sources.clone(),
            tools,
            head,
            initial,
            required,
            request: swarmie_context::durable::Input {
                operation: request.operation,
                fence: request.fence,
                plan,
            },
        };
        // Publish the association before creating schedulable work. The task's
        // deterministic identity recognizes a lost creation response without replay.
        context.check()?;
        self.save_context_ledger(ledger)?;
        let change = TaskChange::Create {
            spec: TaskSpec {
                id,
                definition: swarmie_context::durable::definition(),
                input: serde_json::to_value(work)
                    .map_err(|_| failure("context task cannot be encoded"))?,
            },
            principal: self.principal.clone(),
        };
        match self
            .tasks
            .storage
            .commit(vec![change])
            .await
            .map_err(|error| failure(&error.to_string()))?
        {
            CommitOutcome::Applied { .. } => {
                self.context_receipt(
                    context,
                    request.operation,
                    request.operation,
                    Admission::Admitted,
                )
                .await
            }
            CommitOutcome::Unknown { .. } => {
                self.tasks.storage.owner().require_recovery();
                Err(failure(
                    "context admission is unknown; recognize the same operation after recovery",
                ))
            }
            CommitOutcome::Rejected(reason) => {
                let mut ledger = self.context_ledger()?;
                if ledger.task == Some(id)
                    && ledger.checkpoint.compaction().is_some_and(|work| {
                        work.operation == request.operation
                            && work.phase == swarmie_context::Phase::Planned
                    })
                {
                    ledger.checkpoint.refuse(
                        request.operation,
                        "The durable owner refused compaction admission".into(),
                    )?;
                    self.save_context_ledger(ledger)?;
                }
                Err(failure(&reason.to_string()))
            }
        }
    }

    pub(in crate::conversations) async fn context_stop(
        &self,
        context: &Context,
        operation: OperationId,
    ) -> tembra::Result<Receipt> {
        context.check()?;
        if context.principal()? != self.principal {
            return Err(failure("context principal is invalid"));
        }
        let mut ledger = self.observed_context(context).await?;
        if !ledger
            .checkpoint
            .compaction()
            .is_some_and(|work| work.operation == operation)
        {
            return Err(failure("context Stop names another operation"));
        }
        ledger.checkpoint.stop(operation)?;
        ledger.fence = uuid::Uuid::new_v4().try_into()?;
        self.save_context_ledger(ledger)?;
        if let Some((current, stop)) = self.compaction_running.borrow().as_ref()
            && *current == operation
        {
            stop.cancel();
        }
        self.context_receipt(context, operation, operation, Admission::Recognized)
            .await
    }

    pub(in crate::conversations) async fn context_wait(
        &self,
        context: &Context,
    ) -> tembra::Result<()> {
        if self.index.get(&self.id)?.is_none() {
            return Ok(());
        }
        let ledger = self.observed_context(context).await?;
        if !ledger.automatic
            && let Some(task) = ledger.task
        {
            self.tasks.wait(context, task).await?;
        }
        let ledger = self.observed_context(context).await?;
        if ledger.checkpoint.compaction().is_some_and(|work| {
            work.is_pending() || work.required && work.phase != swarmie_context::Phase::Activated
        }) {
            return Err(failure(
                "context compaction must be recognized or completed before another turn",
            ));
        }
        Ok(())
    }
}
