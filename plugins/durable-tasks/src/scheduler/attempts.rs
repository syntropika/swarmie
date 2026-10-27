use super::{Engine, SchedulerDiagnostic};
use crate::{
    Execution, TaskChange, TaskId, TaskStatus,
    authority::{Reason, error, storage_error},
    execution::Scope,
};
use futures::FutureExt;
use std::{collections::BTreeMap, rc::Rc};
use tembra::Code;

impl Engine {
    pub(super) async fn invoke(
        self: Rc<Self>,
        scope: Rc<Scope>,
    ) -> (TaskId, Option<SchedulerDiagnostic>) {
        let id = scope.id();
        let outcome = self.execute(scope.clone()).await;
        if outcome.is_err() && self.authority.check().is_ok() {
            scope.settle().await;
            // Also release claims whose targeted input read or dispatch failed.
            // The token check makes this harmless if settlement already released it.
            let _ = self
                .authority
                .commit(vec![TaskChange::Release {
                    id,
                    attempt: scope.attempt(),
                }])
                .await;
        }
        (
            id,
            outcome.err().map(|error| SchedulerDiagnostic::Interrupted {
                task: id,
                error: error.into(),
            }),
        )
    }

    async fn execute(&self, scope: Rc<Scope>) -> tembra::Result<()> {
        let id = scope.id();
        let attempt = scope.attempt();
        let record = self
            .authority
            .storage
            .get(id)
            .await
            .map_err(storage_error)?;
        let Some(record) = record else {
            return Err(error(
                Code::Unavailable,
                Reason::Scope,
                "acquired task disappeared",
            ));
        };
        if record.assignment.as_ref().map(|a| a.attempt) != Some(attempt) {
            scope.revoke();
            return Ok(());
        }
        scope.adopt(record.clone());
        let execution = async {
            if self.control.stopping.get() || scope.is_stopped() {
                return Err(error(
                    Code::Cancelled,
                    Reason::Closed,
                    "dispatch interrupted",
                ));
            }
            // Each assignment owns a fresh instance; prior asynchronous cleanup cannot
            // prevent a resumed attempt from acquiring its own dependencies.
            let reference = Rc::new(self.host.get(
                record.spec.definition.service(),
                vec![id.to_string(), attempt.to_string()],
                &record.principal,
            )?);
            let writer = Rc::new(self.host.get(
                crate::SERVICE,
                vec![crate::INSTANCE_KEY.into()],
                crate::HOST_PRINCIPAL,
            )?);
            let execution = Execution {
                scope: scope.clone(),
                writer,
            };
            let owned = scope.clone();
            self.host
                .root(&record.principal, move |context| {
                    async move {
                        owned.enter(context.clone());
                        let result: tembra::Result<crate::TaskOutcome> = reference
                            .call(&context, crate::TaskMethod::Execute.as_str(), execution)
                            .await;
                        owned.settle().await;
                        if context.check().is_err() {
                            owned.stop();
                        }
                        Ok(result)
                    }
                    .boxed_local()
                })
                .await?
        }
        .await;
        scope.settle().await;
        let record = scope.record()?;
        let interrupted = scope.is_stopped()
            || self.control.stopping.get()
            || self.signal.is_cancelled()
            || self.authority.check().is_err();
        let mut deferred = None;
        let mut changes = vec![];
        if !interrupted && record.status == TaskStatus::Ready {
            match execution {
                Ok(crate::TaskOutcome::Completed(result)) => changes.push(TaskChange::Complete {
                    id,
                    attempt,
                    revision: record.revision,
                    result,
                }),
                Ok(crate::TaskOutcome::Waiting {
                    checkpoint,
                    condition,
                }) => changes.push(TaskChange::Suspend {
                    id,
                    attempt,
                    revision: record.revision,
                    checkpoint,
                    condition,
                }),
                Err(reason)
                    if matches!(
                        reason.code,
                        Code::Unavailable | Code::Closed | Code::Cancelled
                    ) =>
                {
                    deferred = Some(reason)
                }
                Err(reason) => changes.push(TaskChange::Fail {
                    id,
                    attempt,
                    revision: record.revision,
                    error: reason.into(),
                }),
            }
        }
        changes.push(TaskChange::Release { id, attempt });
        if self.authority.check().is_ok() {
            match self.authority.commit(changes).await? {
                Ok(_) => {}
                Err(crate::Rejection::StaleAttempt) => scope.revoke(),
                Err(reason) => return Err(error(Code::Conflict, Reason::Conflict, reason)),
            }
        }
        // A cancelled Ready execution needs an explicit recovery decision. Releasing
        // its assignment must not silently dispatch it again through storage notices.
        if (scope.lost.get() || (scope.is_stopped() && record.status == TaskStatus::Ready))
            && !self.control.stopping.get()
            && deferred.is_none()
        {
            deferred = Some(error(
                Code::Cancelled,
                Reason::Scope,
                if scope.lost.get() {
                    "assignment authority was lost"
                } else {
                    "execution was cancelled"
                },
            ));
        }
        deferred.map_or(Ok(()), Err)
    }
    pub(super) async fn renew(&self, active: &BTreeMap<TaskId, Rc<Scope>>) -> tembra::Result<()> {
        let changes: Vec<_> = active
            .values()
            .filter(|s| !s.lost.get())
            .map(|s| TaskChange::Renew {
                id: s.id(),
                attempt: s.attempt(),
                lease_ms: self.options.lease_duration_ms,
            })
            .collect();
        if changes.is_empty() {
            return Ok(());
        }
        if let Err(reason) = self.authority.commit(changes).await? {
            if reason != crate::Rejection::StaleAttempt {
                return Err(error(Code::Conflict, Reason::Conflict, reason));
            }
            for scope in active.values() {
                let record = self
                    .authority
                    .storage
                    .get(scope.id())
                    .await
                    .map_err(storage_error)?;
                // A rejected renewal cannot revive an expired token. The commit boundary
                // already refused at least one; independently validate each with renewal.
                if record
                    .as_ref()
                    .and_then(|r| r.assignment.as_ref())
                    .map(|a| a.attempt)
                    != Some(scope.attempt())
                {
                    scope.revoke();
                    continue;
                }
                if self
                    .authority
                    .commit(vec![TaskChange::Renew {
                        id: scope.id(),
                        attempt: scope.attempt(),
                        lease_ms: self.options.lease_duration_ms,
                    }])
                    .await?
                    .is_err()
                {
                    scope.revoke();
                }
            }
        }
        Ok(())
    }
}
