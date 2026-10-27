use super::checkpoint::{Checkpoint, Completion, ExecutionFailure, ExecutionResult, Request};
use super::writer::Writer;
use crate::{Agent, model::invalid};
use futures::FutureExt;
use std::{cell::RefCell, rc::Rc};
use tembra::{Context, Declaration, Host, Reference, Resource};

/// Connect the engine to stored history through a Tembra reference.
#[derive(Clone)]
pub struct History(ReferenceSource);

#[derive(Clone)]
enum ReferenceSource {
    Borrowed(Rc<Reference>),
    Bound(Rc<Resource<Reference>>),
}

impl History {
    /// Use a caller-owned reference. Prefer [`Self::bind`] inside an Orb for managed cleanup.
    pub fn new(reference: Rc<Reference>) -> Self {
        Self(ReferenceSource::Borrowed(reference))
    }

    /// Bind provider cleanup to this object. Calls authorize the invoking principal.
    pub fn bind(
        declaration: &Declaration,
        host: Host,
        service: impl Into<String>,
        key: Vec<String>,
    ) -> tembra::Result<Self> {
        let service = service.into();
        let name = serde_json::to_string(&("agent.history", &service, &key)).map_err(invalid)?;
        let reference = declaration.resource(
            &name,
            1,
            move || host.get(&service, key, "host"),
            |reference, _| async move { reference.release() }.boxed_local(),
        )?;
        Ok(Self(ReferenceSource::Bound(Rc::new(reference))))
    }

    pub(super) fn reference(&self) -> tembra::Result<Rc<Reference>> {
        match &self.0 {
            ReferenceSource::Borrowed(reference) => Ok(reference.clone()),
            ReferenceSource::Bound(reference) => reference.value(),
        }
    }

    /// Direct callers own the physical future. Use a Tembra method for managed
    /// execution when dropping a waiter must not abandon the enclosing engine.
    pub async fn execute(
        &self,
        context: &Context,
        agent: &Agent,
        request: Request,
    ) -> ExecutionResult {
        self.resume(context, agent, Checkpoint::start(request))
            .await
    }

    /// Restore the last confirmed history and reconcile pending provider operations.
    /// Invalid checkpoints return no progress and do not reach the agent.
    pub async fn resume(
        &self,
        context: &Context,
        agent: &Agent,
        checkpoint: Checkpoint,
    ) -> ExecutionResult {
        let (mut state, progress) =
            self.restore(context, checkpoint)
                .await
                .map_err(|error| ExecutionFailure {
                    error,
                    checkpoint: None,
                })?;
        let writer = Writer {
            history: self,
            progress: RefCell::new(progress),
            #[cfg(feature = "durable-tasks")]
            execution: None,
        };
        let result = agent.drive(context, &mut state, &writer).await;
        let checkpoint = writer.progress.into_inner().checkpoint;
        match result {
            Ok(output) => Ok(Completion { output, checkpoint }),
            Err(error) => Err(ExecutionFailure {
                error: error.original(),
                checkpoint: Some(Box::new(checkpoint)),
            }),
        }
    }
}
