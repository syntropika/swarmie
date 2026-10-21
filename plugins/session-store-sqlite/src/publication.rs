//! One coalesced retry hint over Tembra's managed activity and the existing Host Driver.
use crate::database::Store;
use futures::{
    FutureExt,
    future::{Either, poll_fn, select},
    task::AtomicWaker,
};
use std::{
    cell::{Cell as Flag, RefCell},
    rc::Rc,
    time::Duration,
};
use swarmie_session_contracts::{MAX_SEQUENCE, PublicationStatus, StoreHead};
use tembra::{core, prelude::*};

pub(crate) struct Publication {
    resource: Resource<Store>,
    committed: Cell<StoreHead>,
    driver: Rc<dyn core::Driver>,
    pending: Flag<bool>,
    wake: AtomicWaker,
    failures: Flag<u64>,
    last_error: RefCell<Option<String>>,
}

impl Publication {
    pub(crate) fn new(
        resource: Resource<Store>,
        committed: Cell<StoreHead>,
        driver: Rc<dyn core::Driver>,
    ) -> Rc<Self> {
        Rc::new(Self {
            resource,
            committed,
            driver,
            pending: Flag::new(false),
            wake: AtomicWaker::new(),
            failures: Flag::new(0),
            last_error: RefCell::new(None),
        })
    }

    pub(crate) fn attach(self: &Rc<Self>, declaration: &mut tembra::Declaration<'_>) -> Result<()> {
        let publication = self.clone();
        declaration
            .core_mut()
            .task(move |scope| publication.clone().run(scope).boxed_local())?;
        Ok(())
    }

    /// Publication failure cannot replace an authoritative SQLite result. One boolean retains
    /// pending work; the managed worker obtains fresh roots and current durable metadata.
    pub(crate) fn reconcile(&self, context: &Context) {
        let result = (|| {
            let head = self.resource.value()?.head()?;
            if self.committed.value()? != head {
                self.committed.replace(context, head)?;
            }
            self.pending.set(false);
            Ok(())
        })();
        if let Err(error) = result {
            self.failed(&error);
        }
    }

    fn failed(&self, error: &RuntimeError) {
        self.failures
            .set(self.failures.get().saturating_add(1).min(MAX_SEQUENCE));
        // Preserve bounded categories, not arbitrary downstream diagnostic text or payloads.
        *self.last_error.borrow_mut() = Some(
            format!("{:?}/{}", error.code, error.reason)
                .chars()
                .take(512)
                .collect(),
        );
        self.pending.set(true);
        self.wake.wake();
    }

    pub(crate) fn status(&self) -> Result<PublicationStatus> {
        let durable = self.resource.value()?.head()?;
        let published = self.committed.value()?;
        Ok(PublicationStatus {
            pending: durable != published,
            durable,
            published,
            failures: self.failures.get(),
            last_error: self.last_error.borrow().clone(),
        })
    }

    async fn run(self: Rc<Self>, scope: core::TaskScope) -> Result<()> {
        let signal = scope.signal();
        loop {
            let ready = poll_fn(|context| {
                self.wake.register(context.waker());
                if self.pending.get() {
                    std::task::Poll::Ready(())
                } else {
                    std::task::Poll::Pending
                }
            });
            if let Either::Left(_) = select(signal.cancelled(), ready.boxed_local()).await {
                return Ok(());
            }
            // Bound retry pressure while a derived State keeps refusing candidates. Cancellation
            // interrupts this delay; no timer or resource access survives activity retirement.
            if let Either::Left(_) = select(
                signal.cancelled(),
                self.driver.sleep(Duration::from_millis(50)),
            )
            .await
            {
                return Ok(());
            }
            if !self.pending.get() {
                continue;
            }
            let publication = self.clone();
            let result = scope
                .root(move |context| {
                    async move {
                        publication.reconcile(&Context::from_core(context));
                        Ok(())
                    }
                    .boxed_local()
                })
                .await;
            if let Err(error) = result {
                self.failed(&error);
            }
        }
    }
}
