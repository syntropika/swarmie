//! Real Facet-owned consumer calls shared by SQLite integration fixtures.

use std::{cell::RefCell, rc::Rc};

use futures::FutureExt;
use swarmie_task_contracts::{
    TaskOutboxAcknowledgement, TaskOutboxCompaction, TaskOutboxConsumer, TaskOutboxConsumerState,
    TaskOutboxPage, TaskOutboxQuery, TaskOutboxRetention, TaskStorageRef,
};
use tembra::{core, prelude::*};

/// Keeps each operation inside its live facet-owned root invocation. No Context escapes
/// its settlement, and consumer ownership is never supplied through request metadata.
#[derive(Clone)]
pub struct Consumer {
    scope: core::FacetScope,
    storage: TaskStorageRef,
}

/// Installs a real native caller. Reuse its stable facet name when reopening a durable consumer.
pub async fn install(host: &Host, name: &str, storage: TaskStorageRef) -> Result<Consumer> {
    let captured = Rc::new(RefCell::new(None));
    let opening = captured.clone();
    host.install(
        NativeFacet::new(name, vec![], move |scope, _| {
            *opening.borrow_mut() = Some(scope.clone());
            Ok(())
        })?,
        core::LoadOptions::default(),
    )?
    .await?;
    let scope = captured.borrow_mut().take().ok_or_else(|| {
        RuntimeError::unavailable(
            "fixture_consumer_scope",
            "consumer facet did not initialize",
        )
    })?;
    Ok(Consumer { scope, storage })
}

#[allow(
    dead_code,
    reason = "integration binaries use different subsets of this shared caller fixture"
)]
impl Consumer {
    pub async fn register_outbox_consumer(
        &self,
        request: TaskOutboxConsumer,
    ) -> Result<TaskOutboxConsumerState> {
        let storage = self.storage.clone();
        self.scope
            .root(move |context| {
                async move {
                    storage
                        .with_context(&Context::from_core(context))
                        .register_outbox_consumer(request)
                        .await
                }
                .boxed_local()
            })
            .await
    }

    pub async fn read_outbox(&self, request: TaskOutboxQuery) -> Result<TaskOutboxPage> {
        let storage = self.storage.clone();
        self.scope
            .root(move |context| {
                async move {
                    storage
                        .with_context(&Context::from_core(context))
                        .read_outbox(request)
                        .await
                }
                .boxed_local()
            })
            .await
    }

    pub async fn acknowledge_outbox(&self, request: TaskOutboxAcknowledgement) -> Result<()> {
        let storage = self.storage.clone();
        self.scope
            .root(move |context| {
                async move {
                    storage
                        .with_context(&Context::from_core(context))
                        .acknowledge_outbox(request)
                        .await
                }
                .boxed_local()
            })
            .await
    }

    pub async fn remove_outbox_consumer(&self, request: TaskOutboxConsumer) -> Result<()> {
        let storage = self.storage.clone();
        self.scope
            .root(move |context| {
                async move {
                    storage
                        .with_context(&Context::from_core(context))
                        .remove_outbox_consumer(request)
                        .await
                }
                .boxed_local()
            })
            .await
    }

    pub async fn compact_outbox(
        &self,
        request: TaskOutboxCompaction,
    ) -> Result<TaskOutboxRetention> {
        let storage = self.storage.clone();
        self.scope
            .root(move |context| {
                async move {
                    storage
                        .with_context(&Context::from_core(context))
                        .compact_outbox(request)
                        .await
                }
                .boxed_local()
            })
            .await
    }
}

impl std::ops::Deref for Consumer {
    type Target = TaskStorageRef;

    fn deref(&self) -> &Self::Target {
        &self.storage
    }
}

#[allow(
    dead_code,
    reason = "integration binaries use different subsets of this shared caller fixture"
)]
impl Consumer {
    pub fn raw(&self) -> TaskStorageRef {
        self.storage.clone()
    }

    pub async fn conformance(&self) -> Result<()> {
        let storage = self.storage.clone();
        self.scope
            .root(move |context| {
                Box::pin(async move {
                    swarmie_task_contracts::conformance::run(
                        &storage.with_context(&Context::from_core(context)),
                    )
                    .await
                })
            })
            .await
    }
}
