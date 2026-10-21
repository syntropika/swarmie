//! Bounded admission to Tembra Hooks. This adapter owns no composition or handler execution.

use std::{cell::RefCell, collections::VecDeque, rc::Rc, time::Duration};

use futures::FutureExt;
use swarmie_task_contracts::{MAX_PORTABLE_INTEGER, TaskLifecycleDiagnostics, TaskLifecycleEvent};
use tembra::{Result, RuntimeError, core};
use tembra_hooks::{HookManager, HookManagerOptions};

use crate::{
    delivery::{Notification, Pipelines, notifications},
    scheduler::Wake,
};

const MAX_PENDING: usize = 256;
const MAX_CONTRIBUTIONS: usize = 64;
const DEADLINE: Duration = Duration::from_secs(1);

pub(crate) struct Observations {
    pipelines: Pipelines,
    manager: HookManager,
    driver: Rc<dyn core::Driver>,
    queue: RefCell<VecDeque<Notification>>,
    diagnostics: RefCell<TaskLifecycleDiagnostics>,
    signal: RefCell<Option<core::FacetSignal>>,
    wake: Wake,
}

fn increment(counter: &mut u64) {
    *counter = counter.saturating_add(1).min(MAX_PORTABLE_INTEGER);
}

impl Observations {
    pub fn new(scope: core::FacetScope, driver: Rc<dyn core::Driver>) -> Result<Rc<Self>> {
        let pipelines = Pipelines::new(&scope)?;
        let manager = HookManager::new(
            scope,
            driver.clone(),
            HookManagerOptions {
                max_contributions: MAX_CONTRIBUTIONS,
                max_active_runs: 1,
            },
        )?;
        Ok(Rc::new(Self {
            pipelines,
            manager,
            driver,
            queue: RefCell::default(),
            diagnostics: RefCell::default(),
            signal: RefCell::default(),
            wake: Wake::default(),
        }))
    }

    /// Infallible after-commit admission, independent of the submitting caller's Context.
    /// A current Registry probe avoids allocating queue entries when no observer exists.
    pub fn emit(&self, event: TaskLifecycleEvent) {
        for notification in notifications(event) {
            self.enqueue(notification);
        }
    }

    fn enqueue(&self, event: Notification) {
        if self
            .signal
            .borrow()
            .as_ref()
            .is_some_and(core::FacetSignal::is_cancelled)
        {
            increment(&mut self.diagnostics.borrow_mut().abandoned);
            return;
        }
        match self.pipelines.has_handlers(&event) {
            Ok(false) => {
                increment(&mut self.diagnostics.borrow_mut().skipped);
                return;
            }
            Ok(true) => {}
            Err(error) => {
                self.record_error(&error);
                return;
            }
        }
        let mut queue = self.queue.borrow_mut();
        if queue.len() >= MAX_PENDING {
            increment(&mut self.diagnostics.borrow_mut().dropped);
            return;
        }
        queue.push_back(event);
        let mut diagnostics = self.diagnostics.borrow_mut();
        increment(&mut diagnostics.enqueued);
        diagnostics.queued = queue.len() as u32;
        drop(diagnostics);
        drop(queue);
        self.wake.notify();
    }

    pub fn diagnostics(&self) -> TaskLifecycleDiagnostics {
        self.diagnostics.borrow().clone()
    }

    pub fn record_error(&self, error: &RuntimeError) {
        let mut diagnostics = self.diagnostics.borrow_mut();
        increment(&mut diagnostics.failed);
        // Reasons and messages may originate in user handlers. Retain a bounded UTF-8 prefix.
        let mut detail = error.to_string();
        let mut end = detail.len().min(512);
        while !detail.is_char_boundary(end) {
            end -= 1;
        }
        detail.truncate(end);
        diagnostics.last_error = Some(detail);
    }

    pub async fn run(self: Rc<Self>, scope: core::TaskScope) -> Result<()> {
        let signal = scope.signal();
        *self.signal.borrow_mut() = Some(signal.clone());
        let mut batch = 0;
        loop {
            if signal.is_cancelled() {
                break;
            }
            let event = self.queue.borrow_mut().pop_front();
            if let Some(event) = event {
                {
                    let mut diagnostics = self.diagnostics.borrow_mut();
                    diagnostics.queued = self.queue.borrow().len() as u32;
                    diagnostics.active = true;
                }
                let selected = self.clone();
                let result = scope
                    .root(move |context| {
                        async move {
                            let run = selected.pipelines.run(&selected.manager, event, &context);
                            let timer = selected.driver.sleep(DEADLINE);
                            futures::pin_mut!(run, timer);
                            match futures::future::select(run, timer).await {
                                futures::future::Either::Left((result, _)) => result,
                                futures::future::Either::Right(((), run)) => {
                                    increment(&mut selected.diagnostics.borrow_mut().timed_out);
                                    context.cancellation().cancel(RuntimeError::aborted(
                                        "task_lifecycle_timeout",
                                        "lifecycle observation exceeded its deadline",
                                    ));
                                    // A deadline signals cancellation, never physical completion.
                                    // Tembra retains the run budget and joins the entered handler.
                                    run.await
                                }
                            }
                        }
                        .boxed_local()
                    })
                    .await;
                self.diagnostics.borrow_mut().active = false;
                match result {
                    Ok(()) => increment(&mut self.diagnostics.borrow_mut().completed),
                    Err(error) => self.record_error(&error),
                }
                // Bound synchronous work without imposing a timer on every notification.
                batch += 1;
                if batch == 16 {
                    self.driver.sleep(Duration::from_millis(1)).await;
                    batch = 0;
                }
            } else {
                let stopped = signal.cancelled();
                let ready = self.wake.wait();
                futures::pin_mut!(stopped, ready);
                futures::future::select(stopped, ready).await;
            }
        }
        let pending = self.queue.borrow_mut().drain(..).count() as u64;
        let mut diagnostics = self.diagnostics.borrow_mut();
        diagnostics.abandoned = diagnostics
            .abandoned
            .saturating_add(pending)
            .min(MAX_PORTABLE_INTEGER);
        diagnostics.queued = 0;
        Ok(())
    }
}
