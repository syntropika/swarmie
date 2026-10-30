//! Client task admission and observation through the existing durable task owner.
use super::{
    SERVICE, failure,
    types::{ExecutionContext, Phase},
};
use futures::{
    FutureExt,
    channel::oneshot,
    task::{LocalFutureObj, LocalSpawn, SpawnError},
};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, num::NonZeroU32, rc::Rc};
use swarmie_agent_loop::conversation::Request;
use swarmie_conversations::{Conversations, EntryDraft, WriteConfirmation};
use swarmie_durable_tasks::{
    self as durable, CommitOutcome, ExecutorId, TaskChange, TaskDefinition, TaskId, TaskSpec,
    TaskStatus, TaskStorage,
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
    pub executor: ExecutorId,
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

    /// Wait for physical release on this executor. Losing an observer never cancels work.
    pub async fn wait(&self, context: &Context, id: TaskId) -> tembra::Result<()> {
        loop {
            context.check()?;
            let (send, receive) = oneshot::channel();
            let notify = Rc::new(RefCell::new(Some(send)));
            let changed = notify.clone();
            let _subscription = self.storage.changes().subscribe(move || {
                if let Some(send) = changed.borrow_mut().take() {
                    let _ = send.send(());
                }
            });
            let _owner = self.storage.owner().changes().subscribe(move || {
                if let Some(send) = notify.borrow_mut().take() {
                    let _ = send.send(());
                }
            });
            let record = self
                .storage
                .get(id)
                .await
                .map_err(|error| failure(&error.to_string()))?
                .ok_or_else(|| failure("the admitted turn is missing"))?;
            if matches!(
                record.status,
                TaskStatus::Completed(_) | TaskStatus::Failed(_)
            ) && record
                .assignment
                .as_ref()
                .is_none_or(|assignment| assignment.executor != self.executor)
            {
                return Ok(());
            }
            let signal = context.signal();
            futures::select_biased! {
                () = signal.cancelled().fuse() => return context.check(),
                _ = receive.fuse() => {},
            }
        }
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
            let input: Input = serde_json::from_value(execution.input())
                .map_err(|_| failure("invalid persisted turn input"))?;
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
                .call(&context, durable::Method::Resume.as_str(), ())
                .await
        }
        .boxed_local()
    })
    .await
}
