//! Task-scoped observation and explicit readmission through the scheduler owner.
use super::Engine;
use crate::{
    TaskId, TaskObservation, TaskStatus,
    authority::{Reason, error, storage_error},
};
use futures::{FutureExt, channel::oneshot};
use std::{cell::RefCell, rc::Rc};
use tembra::{Code, Context};

impl Engine {
    /// This is local physical observation, not cross-executor effect reconciliation.
    pub async fn observe(&self, context: &Context, id: TaskId) -> tembra::Result<TaskObservation> {
        context.check()?;
        self.authority.check()?;
        self.signal.check()?;
        if self.control.stopping.get() {
            return Err(error(Code::Closed, Reason::Closed, "scheduler is closing"));
        }
        let task = self
            .authority
            .storage
            .get(id)
            .await
            .map_err(storage_error)?
            .ok_or_else(|| error(Code::Invalid, Reason::Scope, "task does not exist"))?;
        context.check()?;
        self.authority.check()?;
        let principal = context.principal()?;
        if principal != crate::HOST_PRINCIPAL && task.principal != principal {
            return Err(error(
                Code::Forbidden,
                Reason::Forbidden,
                "observation requires task owner",
            ));
        }
        if matches!(
            task.status,
            TaskStatus::Completed(_) | TaskStatus::Failed(_)
        ) && !self.control.active.borrow().contains(&id)
            && task
                .assignment
                .as_ref()
                .is_none_or(|assignment| assignment.executor != self.options.executor)
        {
            Ok(TaskObservation::Settled)
        } else if self.control.status.borrow().interrupted.contains(&id)
            && !self.control.retry_tasks.borrow().contains(&id)
        {
            Ok(TaskObservation::Interrupted)
        } else {
            Ok(TaskObservation::Pending)
        }
    }

    /// Losing this observer never cancels the attempt. Subscribe before reading,
    /// including scheduler changes emitted after the final storage release notice.
    pub async fn wait(&self, context: &Context, id: TaskId) -> tembra::Result<TaskObservation> {
        loop {
            let (send, receive) = oneshot::channel();
            let notify = Rc::new(RefCell::new(Some(send)));
            let _subscriptions: Vec<_> = [
                self.authority.storage.changes(),
                self.authority.storage.owner().changes(),
                self.control.changes.clone(),
            ]
            .into_iter()
            .map(|source| {
                let notify = notify.clone();
                source.subscribe(move || {
                    if let Some(send) = notify.borrow_mut().take() {
                        let _ = send.send(());
                    }
                })
            })
            .collect();
            let observed = self.observe(context, id).await?;
            if observed != TaskObservation::Pending {
                return Ok(observed);
            }
            let signal = context.signal();
            futures::select_biased! {
                () = signal.cancelled().fuse() => { context.check()?; },
                () = self.signal.cancelled().fuse() => { self.signal.check()?; },
                _ = receive.fuse() => {},
            }
        }
    }

    /// Clear only this task's local hold after an explicit consumer recovery decision.
    /// The task owner must reconcile unknown effects before choosing readmission.
    /// This call cannot resume the scheduler, reclaim another executor or change input.
    pub async fn wake_task(&self, context: &Context, id: TaskId) -> tembra::Result<()> {
        if self.observe(context, id).await? == TaskObservation::Interrupted {
            self.control.retry_tasks.borrow_mut().insert(id);
            self.control.wake(false);
        }
        Ok(())
    }
}
