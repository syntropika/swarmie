use crate::authority::{Authority, Reason, error, storage_error};
use crate::{Definitions, TaskChange, TaskId, TaskSpec};
use futures::{
    FutureExt,
    channel::{mpsc, oneshot},
    future::{LocalBoxFuture, Shared},
    task::LocalSpawnExt,
};
use std::time::Duration;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    rc::Rc,
};
use swarmie_storage_core::{ChangeSource, Subscription};
use tembra::{Code, Context, Host, ResourceSignal};

mod attempts;
mod driver;
mod observation;
mod planning;
mod policy;

pub use policy::{Fifo, Options, SchedulerDiagnostic, SchedulerStatus, SchedulerStrategy, Timer};

struct Control {
    enabled: Cell<bool>,
    stopping: Cell<bool>,
    dirty: Cell<bool>,
    retry: Cell<bool>,
    retry_tasks: RefCell<BTreeSet<TaskId>>,
    active: RefCell<BTreeSet<TaskId>>,
    changes: ChangeSource,
    send: mpsc::UnboundedSender<()>,
    status: RefCell<SchedulerStatus>,
}

impl Control {
    fn wake(&self, retry: bool) {
        if self.stopping.get() {
            return;
        }
        if retry {
            self.retry.set(true);
        }
        if !self.dirty.replace(true) {
            let _ = self.send.unbounded_send(());
        }
    }

    fn stop(&self) {
        self.stopping.set(true);
        let _ = self.send.unbounded_send(());
        self.changes.invalidate();
    }
}

pub(crate) struct Engine {
    pub authority: Rc<Authority>,
    host: Host,
    definitions: Definitions,
    options: Options,
    signal: ResourceSignal,
    cursor: Cell<u64>,
    next_wait: Cell<Option<Duration>>,
    control: Rc<Control>,
    join: RefCell<Option<Shared<LocalBoxFuture<'static, tembra::Result<()>>>>>,
    subscriptions: RefCell<Vec<Subscription>>,
}

impl Engine {
    pub fn start(
        host: Host,
        authority: Rc<Authority>,
        definitions: Definitions,
        options: Options,
        signal: ResourceSignal,
    ) -> tembra::Result<Rc<Self>> {
        let (send, receive) = mpsc::unbounded();
        let control = Rc::new(Control {
            enabled: Cell::new(false),
            stopping: Cell::new(false),
            dirty: Cell::new(false),
            retry: Cell::new(false),
            retry_tasks: RefCell::new(BTreeSet::new()),
            active: RefCell::new(BTreeSet::new()),
            changes: ChangeSource::default(),
            send,
            status: RefCell::new(SchedulerStatus::default()),
        });
        let engine = Rc::new(Self {
            authority,
            host,
            definitions,
            options,
            signal,
            cursor: Cell::new(0),
            next_wait: Cell::new(None),
            control,
            join: RefCell::new(None),
            subscriptions: RefCell::new(vec![]),
        });
        // Attach before the first read; concurrent invalidation remains dirty until replanned.
        if let Some(subscription) = engine.definitions.attach() {
            engine.subscriptions.borrow_mut().push(subscription);
        }
        for (source, retry) in [
            (engine.definitions.changes(), true),
            (engine.authority.storage.changes(), false),
            (engine.authority.storage.owner().changes(), false),
        ] {
            let weak = Rc::downgrade(&engine.control);
            engine
                .subscriptions
                .borrow_mut()
                .push(source.subscribe(move || {
                    if let Some(control) = weak.upgrade() {
                        control.wake(retry);
                    }
                }));
        }
        let (done, settled) = oneshot::channel();
        *engine.join.borrow_mut() = Some(
            async move {
                settled.await.map_err(|_| {
                    error(
                        Code::Unavailable,
                        Reason::Closed,
                        "executor stopped before scheduler settlement",
                    )
                })?
            }
            .boxed_local()
            .shared(),
        );
        let driver = engine.clone();
        engine
            .options
            .spawner
            .spawn_local(async move {
                let result = driver.drive(receive).await;
                let _ = done.send(result);
            })
            .map_err(|e| error(Code::Unavailable, Reason::Closed, e))?;
        Ok(engine)
    }

    pub fn status(&self) -> SchedulerStatus {
        self.control.status.borrow().clone()
    }

    pub fn resume(&self, context: &Context) -> tembra::Result<()> {
        self.enable(context)?;
        self.control.wake(true);
        Ok(())
    }

    pub fn enable(&self, context: &Context) -> tembra::Result<()> {
        authorize(context)?;
        self.authority.check()?;
        self.signal.check()?;
        if self.control.stopping.get() {
            return Err(error(Code::Closed, Reason::Closed, "scheduler is closing"));
        }
        self.control.enabled.set(true);
        self.control.status.borrow_mut().enabled = true;
        // Enabling scheduling is not an explicit retry of held work.
        self.control.wake(false);
        Ok(())
    }

    pub fn wake(&self, context: &Context) -> tembra::Result<()> {
        authorize(context)?;
        self.authority.check()?;
        self.control.wake(true);
        Ok(())
    }

    pub async fn create(&self, context: Context, spec: TaskSpec) -> tembra::Result<TaskId> {
        context.check()?;
        let id = spec.id;
        self.authority
            .commit(vec![TaskChange::Create {
                spec,
                principal: context.principal()?,
            }])
            .await?
            .map_err(|r| error(Code::Conflict, Reason::Conflict, r))?;
        self.control.wake(false);
        Ok(id)
    }

    pub async fn signal(
        &self,
        context: Context,
        id: TaskId,
        key: crate::SignalKey,
        value: serde_json::Value,
    ) -> tembra::Result<()> {
        context.check()?;
        let task = self
            .authority
            .storage
            .get(id)
            .await
            .map_err(storage_error)?
            .ok_or_else(|| error(Code::Unavailable, Reason::Conflict, "task does not exist"))?;
        let principal = context.principal()?;
        if principal != crate::HOST_PRINCIPAL && principal != task.principal {
            return Err(error(
                Code::Forbidden,
                Reason::Forbidden,
                "signal requires task owner",
            ));
        }
        self.authority
            .commit(vec![TaskChange::Signal { id, key, value }])
            .await?
            .map_err(|r| error(Code::Conflict, Reason::Conflict, r))?;
        Ok(())
    }

    pub async fn close(&self) -> tembra::Result<()> {
        self.control.stop();
        let join = self.join.borrow().as_ref().cloned();
        let result = if let Some(join) = join {
            join.await
        } else {
            Ok(())
        };
        self.subscriptions.borrow_mut().clear();
        result
    }
    /// Reports shutdown or loss of the scheduler's invocation context.
    fn is_stopping(&self, context: &Context) -> bool {
        self.control.stopping.get() || context.check().is_err() || self.signal.is_cancelled()
    }
}

fn authorize(context: &Context) -> tembra::Result<()> {
    context.check()?;
    if context.principal()? != crate::HOST_PRINCIPAL {
        Err(error(
            Code::Forbidden,
            Reason::Forbidden,
            "scheduling requires host authority",
        ))
    } else {
        Ok(())
    }
}
