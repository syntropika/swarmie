use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use futures::{
    FutureExt,
    future::{Either, select},
    lock::Mutex,
};
use swarmie_session_contracts::SessionStoreRef;
use swarmie_task_contracts::*;
use swarmie_task_session_contracts::*;
use tembra::{Context, Resource, Result, RuntimeError, core};

use crate::{
    Configuration,
    admission::{destination_identity, invalid, source_identity},
    mapping,
};

pub(crate) struct Publisher {
    pub configuration: Configuration,
    pub storage: TaskStorageRef,
    pub sessions: SessionStoreRef,
    pub bindings: TaskSessionBindingsRef,
    pub startup: Resource<TaskOutboxConsumerState>,
    pub driver: Rc<dyn core::Driver>,
    pub gate: Mutex<()>,
    pub failures: Cell<u64>,
    pub last_error: RefCell<Option<String>>,
    pub missing: RefCell<Option<BindingKey>>,
}

impl Publisher {
    pub async fn check(&self, context: &Context) -> Result<()> {
        self.startup.value()?;
        source_identity(&self.storage, context, &self.configuration.task_store_id).await?;
        destination_identity(
            &self.sessions,
            context,
            &self.configuration.session_store_id,
        )
        .await
    }

    pub fn failed(&self, error: &RuntimeError) {
        self.failures.set(
            self.failures
                .get()
                .saturating_add(1)
                .min(MAX_PORTABLE_INTEGER),
        );
        *self.last_error.borrow_mut() = Some(
            format!("{:?}/{}", error.code, error.reason)
                .chars()
                .take(512)
                .collect(),
        );
    }

    pub async fn publish(
        &self,
        request: PublishRequest,
        context: &Context,
    ) -> Result<PublishReceipt> {
        let _guard = self.gate.lock().await;
        let result = self.pass(request, context).await;
        if let Err(error) = &result {
            self.failed(error);
        }
        result
    }

    async fn pass(&self, request: PublishRequest, context: &Context) -> Result<PublishReceipt> {
        if !(1..=256).contains(&request.limit) {
            return Err(invalid(
                "binding_publish_limit",
                "publish limit must be 1..=256",
            ));
        }
        self.check(context).await?;
        let page = self
            .storage
            .with_context(context)
            .read_outbox(TaskOutboxQuery {
                consumer_id: self.configuration.consumer_id.clone(),
                after: None,
                limit: request.limit,
            })
            .await?;
        if page.store_id != self.configuration.task_store_id {
            return Err(invalid(
                "binding_store_mismatch",
                "outbox page belongs to a replacement source",
            ));
        }
        let mut published = 0;
        let mut excluded = 0;
        *self.missing.borrow_mut() = None;
        for fact in page.events {
            let key = BindingKey {
                task_store_id: fact.id.store_id.clone(),
                task_id: fact.task_id.clone(),
            };
            let disposition = self
                .bindings
                .with_context(context)
                .lookup(key.clone())
                .await?;
            match disposition {
                None => {
                    *self.missing.borrow_mut() = Some(key);
                    return Err(invalid(
                        "binding_missing",
                        "publication paused at an unresolved source mapping",
                    ));
                }
                Some(BindingDisposition::Excluded { ref exclusion }) => {
                    if exclusion.key != key {
                        return Err(invalid(
                            "binding_fact_mismatch",
                            "exclusion belongs to another source identity",
                        ));
                    }
                }
                Some(BindingDisposition::Routed { .. }) => {
                    let binding = self
                        .bindings
                        .with_context(context)
                        .require_confirmed_binding(key)
                        .await?;
                    let append = mapping::append(&binding, &fact)?;
                    let receipt = self
                        .sessions
                        .with_context(context)
                        .append(append.clone())
                        .await?;
                    mapping::receipt(&binding, &append, &receipt)?;
                    // No later await may replace the exact request before its matching receipt.
                }
            }
            self.storage
                .with_context(context)
                .acknowledge_outbox(TaskOutboxAcknowledgement {
                    consumer_id: self.configuration.consumer_id.clone(),
                    through: fact.id,
                })
                .await?;
            if matches!(disposition, Some(BindingDisposition::Excluded { .. })) {
                excluded += 1;
            } else {
                published += 1;
            }
        }
        let consumer = self
            .storage
            .with_context(context)
            .register_outbox_consumer(TaskOutboxConsumer {
                id: self.configuration.consumer_id.clone(),
            })
            .await?;
        *self.last_error.borrow_mut() = None;
        Ok(PublishReceipt {
            published,
            excluded,
            consumer,
        })
    }

    pub async fn diagnostics(&self, context: &Context) -> Result<PublisherStatus> {
        let _guard = self.gate.lock().await;
        self.check(context).await?;
        // This bounded diagnostic read can advance delivery, never acknowledgement or retention.
        let page = self
            .storage
            .with_context(context)
            .read_outbox(TaskOutboxQuery {
                consumer_id: self.configuration.consumer_id.clone(),
                after: None,
                limit: 256,
            })
            .await?;
        let tail = self
            .storage
            .with_context(context)
            .read_outbox(TaskOutboxQuery {
                consumer_id: self.configuration.consumer_id.clone(),
                after: page.next.clone(),
                limit: 1,
            })
            .await?;
        let source = self.storage.with_context(context).outbox_status().await?;
        let consumer = source
            .consumers
            .iter()
            .find(|entry| entry.consumer_id == self.configuration.consumer_id)
            .cloned();
        let capacity_exhausted = source.retained_events >= source.limits.max_events
            || source.retained_bytes >= source.limits.max_bytes;
        Ok(PublisherStatus {
            source,
            consumer,
            lag: page.events.len() as u64 + tail.events.len() as u64,
            lag_complete: tail.events.is_empty(),
            failures: self.failures.get(),
            last_error: self.last_error.borrow().clone(),
            missing_binding: self.missing.borrow().clone(),
            capacity_exhausted,
        })
    }

    pub async fn compact(
        &self,
        request: TaskOutboxCompaction,
        context: &Context,
    ) -> Result<TaskOutboxRetention> {
        let _guard = self.gate.lock().await;
        self.check(context).await?;
        self.storage
            .with_context(context)
            .compact_outbox(request)
            .await
    }

    pub async fn run(self: Rc<Self>, scope: core::TaskScope) -> Result<()> {
        let signal = scope.signal();
        loop {
            if signal.is_cancelled() {
                return Ok(());
            }
            let publisher = self.clone();
            let result = scope
                .root(move |context| {
                    async move {
                        publisher
                            .publish(PublishRequest { limit: 256 }, &Context::from_core(context))
                            .await
                            .map(|_| ())
                    }
                    .boxed_local()
                })
                .await;
            if let Err(error) = result {
                if signal.is_cancelled() {
                    return Ok(());
                }
                // pass records ordinary failures; root admission errors need the same diagnostic.
                if self.last_error.borrow().is_none() {
                    self.failed(&error);
                }
            }
            if let Either::Left(_) = select(
                signal.cancelled(),
                self.driver
                    .sleep(Duration::from_millis(self.configuration.poll_interval_ms)),
            )
            .await
            {
                return Ok(());
            }
        }
    }
}
