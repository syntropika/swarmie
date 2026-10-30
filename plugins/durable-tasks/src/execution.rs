use crate::{
    Rejection, TaskChange, TaskRecord,
    authority::{Authority, Reason, error},
};
use futures::{
    FutureExt,
    channel::oneshot,
    future::{LocalBoxFuture, Shared},
};
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use tembra::{Code, Context, Reference};

pub(crate) struct Scope {
    record: RefCell<Option<TaskRecord>>,
    grant: crate::AssignmentGrant,
    pub authority: Rc<Authority>,
    root: RefCell<Option<Context>>,
    pub lost: Cell<bool>,
    closed: Cell<bool>,
    busy: Cell<bool>,
    idle: RefCell<Vec<oneshot::Sender<()>>>,
    stop: RefCell<Option<oneshot::Sender<()>>>,
    stopped: Shared<LocalBoxFuture<'static, ()>>,
}

impl Scope {
    pub fn new(grant: crate::AssignmentGrant, authority: Rc<Authority>) -> Rc<Self> {
        let (send, receive) = oneshot::channel();
        Rc::new(Self {
            record: RefCell::new(None),
            grant,
            authority,
            root: RefCell::new(None),
            lost: Cell::new(false),
            closed: Cell::new(false),
            busy: Cell::new(false),
            idle: RefCell::new(vec![]),
            stop: RefCell::new(Some(send)),
            stopped: async move {
                let _ = receive.await;
            }
            .boxed_local()
            .shared(),
        })
    }

    pub fn id(&self) -> crate::TaskId {
        self.grant.id
    }

    pub fn attempt(&self) -> crate::AttemptId {
        self.grant.assignment.attempt
    }

    pub fn record(&self) -> tembra::Result<TaskRecord> {
        self.record.borrow().clone().ok_or_else(|| {
            error(
                Code::Invalid,
                Reason::Scope,
                "execution input has not been loaded",
            )
        })
    }

    pub fn adopt(&self, record: TaskRecord) {
        *self.record.borrow_mut() = Some(record);
    }

    pub fn enter(&self, context: Context) {
        *self.root.borrow_mut() = Some(context);
    }

    pub fn revoke(&self) {
        self.lost.set(true);
        self.stop();
    }

    pub fn is_stopped(&self) -> bool {
        self.stop.borrow().is_none()
    }

    pub fn stop(&self) {
        if let Some(context) = self.root.borrow().as_ref() {
            let _ = context.cancel();
        }
        if let Some(send) = self.stop.borrow_mut().take() {
            let _ = send.send(());
        }
    }

    pub async fn settle(&self) {
        self.closed.set(true);
        while self.busy.get() {
            let (send, receive) = oneshot::channel();
            self.idle.borrow_mut().push(send);
            let _ = receive.await;
        }
    }

    fn context(&self, context: &Context) -> tembra::Result<()> {
        context.check()?;
        let same = self
            .root
            .borrow()
            .as_ref()
            .is_some_and(|root| root.root == context.root);
        if !same || context.principal()? != self.record()?.principal {
            return Err(error(
                Code::Forbidden,
                Reason::Scope,
                "execution belongs to another managed root",
            ));
        }
        Ok(())
    }
}

/// A native, attempt-scoped capability supplied to an ordinary Tembra task handler.
/// It contains input and the last confirmed checkpoint, never a database transaction.
/// Portable construction is rejected; JavaScript task authoring is not exposed yet.
#[derive(Clone)]
pub struct Execution {
    pub(crate) scope: Rc<Scope>,
    pub(crate) writer: Rc<Reference>,
}

impl<'de> Deserialize<'de> for Execution {
    fn deserialize<D: Deserializer<'de>>(_: D) -> Result<Self, D::Error> {
        Err(serde::de::Error::custom(
            "execution is an owner-issued native capability",
        ))
    }
}

impl Execution {
    /// Returns the stable task identity.
    pub fn id(&self) -> crate::TaskId {
        self.scope.id()
    }

    /// Returns the signal consumed by this attempt, if present.
    pub fn signal(&self) -> Option<crate::SignalDelivery> {
        self.scope.record().ok().and_then(|record| record.delivery)
    }

    /// Returns the input confirmed when the task was created.
    pub fn input(&self) -> Value {
        self.scope
            .record
            .borrow()
            .as_ref()
            .expect("issued execution has loaded input")
            .spec
            .input
            .clone()
    }

    /// Returns the last confirmed checkpoint.
    pub fn checkpoint(&self) -> Option<Value> {
        self.scope
            .record
            .borrow()
            .as_ref()
            .expect("issued execution has loaded input")
            .checkpoint
            .clone()
    }

    /// Checks logical invocation validity. Expiry is rechecked by storage on every commit.
    pub fn check(&self, context: &Context) -> tembra::Result<()> {
        self.scope.context(context)?;
        self.scope.authority.check()?;
        if self.scope.closed.get() || self.scope.stop.borrow().is_none() {
            return Err(error(
                Code::Cancelled,
                Reason::Closed,
                "execution is stopping or has settled",
            ));
        }
        Ok(())
    }

    /// Cooperative interruption signal. It does not prove an external operation stopped.
    pub async fn cancelled(&self) {
        self.scope.stopped.clone().await;
    }

    /// Confirms a checkpoint before continuing the handler.
    ///
    /// # Guarantees
    /// A refusal leaves the last confirmed in-memory checkpoint unchanged. An unknown
    /// result freezes every capability of the shared storage owner. Dropping the caller's future does not
    /// abandon an already dispatched managed write; final settlement joins that write.
    /// This operation includes no independent domain writes or external effects.
    pub async fn commit(&self, context: &Context, checkpoint: Value) -> tembra::Result<()> {
        self.dispatch(context, Write::Progress(Progress::Checkpoint(checkpoint)))
            .await
    }

    /// Prepares orb writes and confirms them atomically with the returned checkpoint.
    ///
    /// The callback runs inside a managed write, with an owner-issued derived context.
    /// Await every participating orb call and use that supplied context. Callback failure
    /// discards preparation. Orb success means staged, not durable; only this method's
    /// success confirms the batch. External effects and detached calls are not atomic.
    /// Dropping the waiter does not abandon dispatched work; settlement joins it.
    /// Only native Rust callbacks are supported.
    pub async fn commit_with<F>(&self, context: &Context, prepare: F) -> tembra::Result<()>
    where
        F: FnOnce(Context) -> LocalBoxFuture<'static, tembra::Result<Value>> + 'static,
    {
        self.dispatch(context, Write::Prepared(Box::new(prepare)))
            .await
    }

    /// Confirms a result even if the handler has further physical cleanup to perform.
    /// The execution slot and assignment remain held until physical settlement.
    /// An in-flight checkpoint settles before this completion is dispatched.
    pub async fn complete(&self, context: &Context, result: Value) -> tembra::Result<()> {
        while self.scope.busy.get() {
            self.check(context)?;
            let (send, receive) = oneshot::channel();
            self.scope.idle.borrow_mut().push(send);
            let signal = context.signal();
            futures::select_biased! {
                () = signal.cancelled().fuse() => { self.check(context)?; },
                () = self.cancelled().fuse() => { self.check(context)?; },
                _ = receive.fuse() => {},
            }
        }
        self.dispatch(context, Write::Progress(Progress::Complete(result)))
            .await
    }

    async fn dispatch(&self, context: &Context, write: Write) -> tembra::Result<()> {
        self.check(context)?;
        if self.scope.busy.replace(true) {
            return Err(error(
                Code::Conflict,
                Reason::Busy,
                "another checkpoint is still settling",
            ));
        }
        let guard = WriteGuard(self.scope.clone());
        self.writer
            .call(
                context,
                crate::orb::WriteMethod::Commit.as_str(),
                WriteRequest { guard, write },
            )
            .await
    }
}

use crate::model::Transition as Progress;

struct WriteGuard(Rc<Scope>);

impl Drop for WriteGuard {
    fn drop(&mut self) {
        self.0.busy.set(false);
        for send in self.0.idle.borrow_mut().drain(..) {
            let _ = send.send(());
        }
    }
}

type Prepare = Box<dyn FnOnce(Context) -> LocalBoxFuture<'static, tembra::Result<Value>>>;

enum Write {
    Progress(Progress),
    Prepared(Prepare),
}

pub(crate) struct WriteRequest {
    guard: WriteGuard,
    write: Write,
}

impl<'de> Deserialize<'de> for WriteRequest {
    fn deserialize<D: Deserializer<'de>>(_: D) -> Result<Self, D::Error> {
        Err(serde::de::Error::custom(
            "task writes require an owner-issued native capability",
        ))
    }
}

impl WriteRequest {
    pub async fn apply(self, context: Context) -> tembra::Result<()> {
        let scope = &self.guard.0;
        scope.context(&context)?;
        scope.authority.check()?;
        let (progress, prepared) = match self.write {
            Write::Progress(progress) => (progress, None),
            Write::Prepared(prepare) => {
                let pending = scope.authority.storage.owner().prepare(&context)?;
                let checkpoint = prepare(pending.context().clone()).await?;
                scope.context(&context)?;
                (Progress::Checkpoint(checkpoint), Some(pending.seal()?))
            }
        };
        let record = scope.record()?;
        let attempt = record
            .assignment
            .as_ref()
            .ok_or_else(|| error(Code::Conflict, Reason::Scope, "missing attempt"))?
            .attempt;
        let change = match &progress {
            Progress::Checkpoint(checkpoint) => TaskChange::Checkpoint {
                id: record.spec.id,
                attempt,
                revision: record.revision,
                checkpoint: checkpoint.clone(),
            },
            Progress::Complete(result) => TaskChange::Complete {
                id: record.spec.id,
                attempt,
                revision: record.revision,
                result: result.clone(),
            },
            Progress::Fail(_) => unreachable!("execution authors do not submit failure changes"),
        };
        scope.context(&context)?;
        scope.authority.check()?;
        let outcome = if let Some(prepared) = prepared {
            scope.authority.confirm(
                scope
                    .authority
                    .storage
                    .commit_with(vec![change], prepared)
                    .await,
            )?
        } else {
            scope.authority.commit(vec![change]).await?
        };
        if let Err(reason) = outcome {
            if matches!(
                reason,
                Rejection::StaleAttempt | Rejection::Revision | Rejection::Terminal
            ) {
                scope.revoke();
            }
            return Err(error(Code::Conflict, Reason::Conflict, reason));
        }
        let mut record = scope.record()?;
        record
            .confirm(progress)
            .map_err(|r| error(Code::Conflict, Reason::Conflict, r))?;
        scope.adopt(record);
        Ok(())
    }
}
