use super::{Engine, SchedulerDiagnostic};
use crate::{
    AttemptId, TaskChange, TaskId, TaskSummary,
    authority::{Reason, error, storage_error},
    execution::Scope,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    time::Duration,
};
use tembra::{Code, Context};

impl Engine {
    pub(super) async fn plan(
        &self,
        active: &BTreeMap<TaskId, Rc<Scope>>,
        held: &BTreeSet<TaskId>,
        capacity: usize,
    ) -> Result<(Vec<TaskSummary>, bool), SchedulerDiagnostic> {
        let after = self.cursor.get();
        let snapshot = self
            .authority
            .storage
            .candidates(after, 64)
            .await
            .map_err(|e| SchedulerDiagnostic::Storage(storage_error(e).into()))?;
        self.next_wait.set(
            snapshot
                .next_wake
                .map(|at| Duration::from_millis(at.saturating_sub(snapshot.now).max(1))),
        );
        let more = snapshot.candidates.len() == 64;
        let installed: BTreeSet<_> = self
            .host
            .kernel()
            .installed_orbs()
            .into_iter()
            .flat_map(|f| {
                f.declaration.services.into_iter().filter_map(move |s| {
                    s.methods
                        .contains(crate::TaskMethod::Execute.as_str())
                        .then_some((s.name, f.declaration.api_version))
                })
            })
            .collect();
        let mut available = BTreeMap::new();
        let mut cursor = after;
        if after == 0 {
            self.control.status.borrow_mut().blocked.clear();
        }
        for candidate in snapshot.candidates {
            if candidate.sequence <= cursor {
                return Err(SchedulerDiagnostic::Storage(
                    error(
                        Code::Invalid,
                        Reason::Storage,
                        "candidate cursor did not advance",
                    )
                    .into(),
                ));
            }
            cursor = candidate.sequence;
            if active.contains_key(&candidate.id) || held.contains(&candidate.id) {
                continue;
            }
            let known = self
                .definitions
                .contains(&candidate.definition)
                .map_err(|e| SchedulerDiagnostic::Definition(e.into()))?;
            if known
                && installed.contains(&(
                    candidate.definition.service().to_owned(),
                    candidate.definition.version.get(),
                ))
            {
                available.insert(candidate.id, candidate);
            } else {
                let mut status = self.control.status.borrow_mut();
                if status.blocked.len() < 128 {
                    status.blocked.push(candidate.id);
                }
            }
        }
        let candidates: Vec<_> = available.values().cloned().collect();
        let order = self
            .options
            .strategy
            .order(&candidates)
            .map_err(|e| SchedulerDiagnostic::Strategy(e.into()))?;
        let unique: BTreeSet<_> = order.iter().copied().collect();
        if order.len() != available.len() || !unique.iter().eq(available.keys()) {
            return Err(SchedulerDiagnostic::Strategy(
                error(
                    Code::Invalid,
                    Reason::Strategy,
                    "strategy must return each candidate exactly once",
                )
                .into(),
            ));
        }
        let selected: Vec<_> = order
            .into_iter()
            .take(capacity.min(64))
            .filter_map(|id| available.remove(&id))
            .collect();
        // Revisit unselected eligible rows once capacity becomes available.
        self.cursor.set(if available.is_empty() && more {
            cursor
        } else {
            0
        });
        Ok((selected, more))
    }

    pub(super) async fn claim(
        &self,
        context: &Context,
        candidates: &[TaskSummary],
        held: &mut BTreeSet<TaskId>,
    ) -> Vec<crate::AssignmentGrant> {
        if candidates.is_empty() || self.is_stopping(context) {
            return vec![];
        }
        let change = |task: &TaskSummary| TaskChange::Acquire {
            id: task.id,
            revision: task.revision,
            executor: self.options.executor,
            attempt: AttemptId::new(),
            lease_ms: self.options.lease_duration_ms,
        };
        let outcome = self
            .authority
            .commit(candidates.iter().map(change).collect())
            .await;
        match outcome {
            Ok(Ok(grants)) => grants,
            outcome
                if candidates.len() > 1
                    && matches!(
                        &outcome,
                        Ok(Err(_))
                            | Err(tembra::Error {
                                code: Code::Invalid,
                                ..
                            })
                    ) =>
            {
                let mut grants = vec![];
                for task in candidates {
                    if self.is_stopping(context) || self.authority.check().is_err() {
                        break;
                    }
                    match self.authority.commit(vec![change(task)]).await {
                        Ok(Ok(mut claimed)) => grants.append(&mut claimed),
                        outcome => self.claim_failure(task.id, outcome, held),
                    }
                }
                grants
            }
            outcome => {
                for task in candidates {
                    self.claim_failure(task.id, outcome.clone(), held);
                }
                vec![]
            }
        }
    }

    fn claim_failure(
        &self,
        id: TaskId,
        outcome: tembra::Result<Result<Vec<crate::AssignmentGrant>, crate::Rejection>>,
        held: &mut BTreeSet<TaskId>,
    ) {
        match outcome {
            Ok(Err(reason)) => {
                if !matches!(
                    reason,
                    crate::Rejection::Assigned | crate::Rejection::Revision
                ) {
                    held.insert(id);
                    self.control.status.borrow_mut().interrupted.push(id);
                }
                self.control.status.borrow_mut().diagnostic =
                    Some(SchedulerDiagnostic::Rejected { task: id, reason });
            }
            Err(reason) => {
                if reason.code == Code::Invalid {
                    held.insert(id);
                    self.control.status.borrow_mut().interrupted.push(id);
                    self.control.status.borrow_mut().diagnostic =
                        Some(SchedulerDiagnostic::Interrupted {
                            task: id,
                            error: reason.into(),
                        });
                } else {
                    self.control.status.borrow_mut().diagnostic =
                        Some(SchedulerDiagnostic::Storage(reason.into()));
                }
            }
            Ok(Ok(_)) => {}
        }
        self.control.changes.invalidate();
    }
}
