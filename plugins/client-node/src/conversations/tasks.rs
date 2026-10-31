//! Client task admission and observation through the existing durable task owner.
use super::{
    SERVICE, failure,
    types::{ExecutionContext, Phase},
};
use futures::{
    FutureExt,
    task::{LocalFutureObj, LocalSpawn, SpawnError},
};
use serde::{Deserialize, Serialize};
use std::{num::NonZeroU32, rc::Rc};
use swarmie_agent_loop::conversation::Request;
use swarmie_conversations::{Conversations, EntryDraft, WriteConfirmation};
use swarmie_durable_tasks::{
    self as durable, CommitOutcome, TaskChange, TaskDefinition, TaskId, TaskObservation,
    TaskSnapshot, TaskSpec, TaskStorage,
};
use tembra::{Context, Host, Orb, Service};

pub(super) const TURN: &str = "swarmie.conversation.turn";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Input {
    pub session: String,
    pub account: String,
    pub model: String,
    pub effort: String,
    pub binding: Option<ExecutionContext>,
    pub request: Request,
    pub context_fence: swarmie_context::WorkFence,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Outcome {
    Settled { phase: Phase },
    StopRequested,
}

impl Outcome {
    pub fn value(self) -> serde_json::Value {
        serde_json::to_value(self).expect("finite turn outcomes serialize")
    }
}

pub(super) struct Tasks {
    pub storage: Rc<dyn TaskStorage>,
    pub host: Host,
}

impl Tasks {
    pub async fn admit(
        &self,
        context: &Context,
        history: &Conversations,
        id: TaskId,
        entry: EntryDraft,
        mut input: Input,
    ) -> tembra::Result<()> {
        let pending = self.storage.owner().prepare(context)?;
        let receipt = history
            .append(pending.context(), input.request.at, vec![entry])
            .await?;
        if receipt.confirmation != WriteConfirmation::Staged {
            return Err(failure(
                "task admission requires shared history preparation",
            ));
        }
        input.request.at = receipt.head;
        let change = TaskChange::Create {
            spec: TaskSpec {
                id,
                definition: definition(),
                input: serde_json::to_value(input)
                    .map_err(|_| failure("the turn input could not be encoded"))?,
            },
            principal: context.principal()?,
        };
        match self
            .storage
            .commit_with(vec![change], pending.seal()?)
            .await
        {
            Ok(CommitOutcome::Applied { .. }) => Ok(()),
            Ok(CommitOutcome::Unknown { .. }) => {
                self.storage.owner().require_recovery();
                Err(failure(
                    "turn admission acknowledgement is unknown; do not replay",
                ))
            }
            Ok(CommitOutcome::Rejected(reason)) => Err(failure(&reason.to_string())),
            Err(error) => Err(failure(&error.to_string())),
        }
    }

    async fn call<R: 'static>(
        &self,
        context: &Context,
        method: durable::Method,
        id: TaskId,
    ) -> tembra::Result<R> {
        let scheduler = self.host.get(
            durable::SERVICE,
            vec![durable::INSTANCE_KEY.into()],
            durable::HOST_PRINCIPAL,
        )?;
        scheduler.call(context, method.as_str(), (id,)).await
    }

    pub async fn observe(
        &self,
        context: &Context,
        id: TaskId,
    ) -> tembra::Result<Option<TaskSnapshot>> {
        self.call(context, durable::Method::Observe, id).await
    }

    /// Wait for local physical settlement or explicit recovery. Observer loss does not stop work.
    pub async fn wait(&self, context: &Context, id: TaskId) -> tembra::Result<TaskObservation> {
        self.call(context, durable::Method::Wait, id).await
    }

    pub async fn wake(&self, context: &Context, id: TaskId) -> tembra::Result<()> {
        self.call(context, durable::Method::WakeTask, id).await
    }
}

pub(super) fn definition() -> TaskDefinition {
    TaskDefinition::new(TURN, NonZeroU32::new(1).unwrap()).expect("static turn definition")
}

pub(super) fn handler(host: Host) -> Orb {
    Orb::new(TURN, 1).service(Service::new(TURN, move |_| Ok(host.clone())).method(
        durable::TaskMethod::Execute.as_str(),
        |host, context, execution: durable::Execution| async move {
            execution.check(&context)?;
            let input: Input = serde_json::from_value(execution.input()).map_err(|_| {
                tembra::Error::invalid("conversation.input", "invalid persisted turn input")
            })?;
            let session = host.get(SERVICE, vec![input.session], &context.principal()?)?;
            let result = session
                .call::<_, durable::TaskOutcome>(&context, "execute_task", execution)
                .await;
            // Releasing observation cannot replace a known execution outcome.
            let _ = session.release();
            result
        },
    ))
}

pub(super) struct Executor;

impl LocalSpawn for Executor {
    fn spawn_local_obj(&self, future: LocalFutureObj<'static, ()>) -> Result<(), SpawnError> {
        tokio::task::spawn_local(future);
        Ok(())
    }
}

pub(crate) async fn start(host: &Host) -> tembra::Result<()> {
    let scheduler = host.get(
        durable::SERVICE,
        vec![durable::INSTANCE_KEY.into()],
        durable::HOST_PRINCIPAL,
    )?;
    host.root(durable::HOST_PRINCIPAL, move |context| {
        async move {
            scheduler
                .call(&context, durable::Method::Enable.as_str(), ())
                .await
        }
        .boxed_local()
    })
    .await
}
