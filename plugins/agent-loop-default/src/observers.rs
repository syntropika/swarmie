//! Bounded notification admission; Tembra owns all Hook composition and execution.

use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
    task::{Poll, Waker},
    time::Duration,
};

use futures::FutureExt;
use swarmie_agent_contracts::*;
use swarmie_task_contracts::MAX_PORTABLE_INTEGER;
use tembra::{core, prelude::*};
use tembra_hooks::{HookManager, HookManagerOptions, HookRun, RustHookDefinition};

const MAX_PENDING: usize = 64;
const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_NOTIFICATION_BYTES: usize = 1024 * 1024;
const DEADLINE: Duration = Duration::from_secs(1);

struct Binding<T: 'static> {
    definition: RustHookDefinition<T, ()>,
    registry: core::RegistryView,
}

impl<T: 'static> Binding<T> {
    fn new(scope: &core::FacetScope, definition: RustHookDefinition<T, ()>) -> Result<Self> {
        Ok(Self {
            registry: scope.registry(definition.descriptor().registry())?,
            definition,
        })
    }
}

// Only static typed wiring is declared here; no handler ordering or Next engine is recreated.
macro_rules! observations {
    ($($variant:ident($field:ident): $payload:ty => $definition:ident),+ $(,)?) => {
        #[derive(serde::Serialize)]
        pub(crate) enum Observation { $($variant($payload)),+ }

        struct Pipelines { $($field: Binding<$payload>),+ }

        impl Pipelines {
            fn new(scope: &core::FacetScope) -> Result<Self> {
                Ok(Self { $($field: Binding::new(scope, $definition()?)?),+ })
            }

            fn has_handlers(&self, observation: &Observation) -> Result<bool> {
                match observation { $(Observation::$variant(_) =>
                    self.$field.registry.table(None).map(|table| !table.is_empty())),+ }
            }

            fn run(&self, manager: &HookManager, observation: Observation, context: &core::Context) -> HookRun<()> {
                match observation { $(Observation::$variant(value) =>
                    manager.run_series(&self.$field.definition, value, context)),+ }
            }
        }
    };
}

observations! {
    TurnStart(turn_start): TurnContext => on_turn_start,
    Model(model): ModelObservation => after_model_call,
    Stream(stream): ModelStreamObservation => on_model_event,
    Tool(tool): ToolObservation => after_tool_call,
    Step(step): StepEnd => on_step_end,
    Turn(turn): TurnEnd => on_turn_end,
}

/// One lifecycle-owned, serial Tembra activity, independent of turn invocation cancellation.
/// Deadlines request cancellation and still join physical work; noncooperative handlers can
/// delay later observations and Host closure, but never hold a turn phase or generation open.
pub(crate) struct ObserverDispatcher {
    pipelines: Pipelines,
    manager: HookManager,
    driver: Rc<dyn core::Driver>,
    queue: RefCell<VecDeque<(Observation, usize)>>,
    diagnostics: RefCell<AgentObservationDiagnostics>,
    signal: RefCell<Option<core::FacetSignal>>,
    pending: Cell<bool>,
    waiter: RefCell<Option<Waker>>,
}

fn increment(value: &mut u64) {
    *value = value.saturating_add(1).min(MAX_PORTABLE_INTEGER);
}

impl ObserverDispatcher {
    pub fn new(scope: core::FacetScope, driver: Rc<dyn core::Driver>) -> Result<Rc<Self>> {
        let pipelines = Pipelines::new(&scope)?;
        let manager = HookManager::new(
            scope,
            driver.clone(),
            HookManagerOptions {
                max_contributions: 64,
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
            pending: Cell::new(false),
            waiter: RefCell::default(),
        }))
    }

    /// Infallible, bounded admission. Notification loss is explicit in diagnostics and never
    /// changes an already confirmed operation. No durable delivery guarantee is implied.
    pub fn enqueue(&self, observation: Observation) {
        if self
            .signal
            .borrow()
            .as_ref()
            .is_some_and(core::FacetSignal::is_cancelled)
        {
            increment(&mut self.diagnostics.borrow_mut().abandoned);
            return;
        }
        match self.pipelines.has_handlers(&observation) {
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
        // Count without allocating a second payload-sized buffer. Serialization stops at the
        // per-notification bound, including for arbitrarily expanded handler results.
        let mut size = Size { bytes: 0 };
        if serde_json::to_writer(&mut size, &observation).is_err() {
            increment(&mut self.diagnostics.borrow_mut().dropped);
            return;
        }
        let mut queue = self.queue.borrow_mut();
        let mut diagnostics = self.diagnostics.borrow_mut();
        if queue.len() >= MAX_PENDING || diagnostics.queued_bytes as usize + size.bytes > MAX_BYTES
        {
            increment(&mut diagnostics.dropped);
            return;
        }
        queue.push_back((observation, size.bytes));
        increment(&mut diagnostics.enqueued);
        diagnostics.queued = queue.len() as u32;
        diagnostics.queued_bytes += size.bytes as u32;
        drop(diagnostics);
        drop(queue);
        self.pending.set(true);
        if let Some(waiter) = self.waiter.borrow_mut().take() {
            waiter.wake();
        }
    }

    fn record_error(&self, error: &RuntimeError) {
        let mut diagnostics = self.diagnostics.borrow_mut();
        increment(&mut diagnostics.failed);
        let reason = &error.reason;
        let mut end = reason.len().min(128);
        while !reason.is_char_boundary(end) {
            end -= 1;
        }
        diagnostics.last_error_reason = Some(reason[..end].into());
    }

    pub fn diagnostics(&self) -> AgentObservationDiagnostics {
        self.diagnostics.borrow().clone()
    }

    pub async fn run(self: Rc<Self>, scope: core::TaskScope) -> Result<()> {
        let signal = scope.signal();
        *self.signal.borrow_mut() = Some(signal.clone());
        let mut batch = 0;
        loop {
            if signal.is_cancelled() {
                break;
            }
            let next = self.queue.borrow_mut().pop_front();
            if let Some((observation, bytes)) = next {
                {
                    let mut diagnostics = self.diagnostics.borrow_mut();
                    diagnostics.queued = self.queue.borrow().len() as u32;
                    diagnostics.queued_bytes -= bytes as u32;
                    diagnostics.active = true;
                }
                let selected = self.clone();
                let result = scope
                    .root(move |context| {
                        async move {
                            let run =
                                selected
                                    .pipelines
                                    .run(&selected.manager, observation, &context);
                            let timer = selected.driver.sleep(DEADLINE);
                            futures::pin_mut!(run, timer);
                            match futures::future::select(run, timer).await {
                                futures::future::Either::Left((result, _)) => result,
                                futures::future::Either::Right(((), run)) => {
                                    increment(&mut selected.diagnostics.borrow_mut().timed_out);
                                    context.cancellation().cancel(RuntimeError::aborted(
                                        "agent_observation_timeout",
                                        "observation deadline reached",
                                    ));
                                    // A deadline is cancellation intent; Tembra still owns settlement.
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
                batch += 1;
                if batch == 16 {
                    self.driver.sleep(Duration::from_millis(1)).await;
                    batch = 0;
                }
            } else {
                let ready = futures::future::poll_fn(|context| {
                    if self.pending.replace(false) {
                        Poll::Ready(())
                    } else {
                        *self.waiter.borrow_mut() = Some(context.waker().clone());
                        Poll::Pending
                    }
                });
                let stopped = signal.cancelled();
                futures::pin_mut!(ready, stopped);
                futures::future::select(ready, stopped).await;
            }
        }
        let abandoned = self.queue.borrow_mut().drain(..).count() as u64;
        let mut diagnostics = self.diagnostics.borrow_mut();
        diagnostics.abandoned = diagnostics
            .abandoned
            .saturating_add(abandoned)
            .min(MAX_PORTABLE_INTEGER);
        diagnostics.queued = 0;
        diagnostics.queued_bytes = 0;
        Ok(())
    }
}

struct Size {
    bytes: usize,
}

impl std::io::Write for Size {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_NOTIFICATION_BYTES.saturating_sub(self.bytes) {
            return Err(std::io::Error::other("observation exceeds byte admission"));
        }
        self.bytes += bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(crate) struct Monitor(pub Rc<ObserverDispatcher>);

impl AgentObservationsProvider for Monitor {
    async fn diagnostics(&self, _context: Context) -> Result<AgentObservationDiagnostics> {
        Ok(self.0.diagnostics())
    }
}
