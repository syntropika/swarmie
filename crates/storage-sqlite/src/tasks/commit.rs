//! Atomic task progress and staged domain writes on one connection.

use super::{
    SqliteTaskStorage,
    records::{insert, read, read_signal_receipts, sql_error, time, update, write_signal_receipts},
};
use futures::{FutureExt, future::LocalBoxFuture};
use rusqlite::TransactionBehavior;
use std::collections::{BTreeMap, BTreeSet};
use swarmie_durable_tasks::{
    AssignmentGrant, CommitOutcome, StorageError, TaskChange, TaskStorage,
};

impl SqliteTaskStorage {
    pub(super) fn commit_batch(
        &self,
        changes: Vec<TaskChange>,
        mut domain: Vec<crate::SqliteWrite>,
    ) -> LocalBoxFuture<'_, Result<CommitOutcome, StorageError>> {
        async move {
            self.lease.check()?;
            self.owner().check()?;
            let mut stored = self.lease.database.connection.borrow_mut();
            let connection = stored.as_mut().ok_or(StorageError::Closed)?;
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(sql_error)?;
            let now = time(&tx)?;
            let mut ids = BTreeSet::new();
            let mut acquired = Vec::new();
            let mut created = false;
            let mut notify = false;
            for change in &changes {
                ids.insert(change.id());
                match change {
                    TaskChange::Acquire { id, .. } => acquired.push(*id),
                    TaskChange::Create { .. } => {
                        created = true;
                        notify = true;
                    }
                    TaskChange::Release { .. } | TaskChange::Signal { .. } => notify = true,
                    _ => {}
                }
            }
            let sequence: u64 = if created {
                tx.query_row(
                    "SELECT sequence FROM swarmie_tasks_meta WHERE singleton=1",
                    [],
                    |r| r.get(0),
                )
                .map_err(sql_error)?
            } else {
                0
            };
            let mut current = BTreeMap::new();
            for id in ids {
                if let Some(record) = read(&tx, id)? {
                    current.insert(id, record);
                }
            }
            let receipts = read_signal_receipts(&tx, &changes)?;
            let prepared = match swarmie_durable_tasks::apply_changes(
                &current, &receipts, now, sequence, changes,
            ) {
                Ok(value) => value,
                Err(reason) => return Ok(CommitOutcome::Rejected(reason)),
            };
            let grants = acquired
                .into_iter()
                .filter_map(|id| {
                    prepared.records[&id]
                        .assignment
                        .clone()
                        .map(|assignment| AssignmentGrant { id, assignment })
                })
                .collect();
            write_signal_receipts(&tx, &receipts, &prepared.signals)?;
            for (id, record) in prepared.records {
                match current.get(&id) {
                    None => insert(&tx, &record)?,
                    Some(previous) if previous != &record => update(&tx, previous, &record)?,
                    Some(_) => {}
                }
            }
            if created {
                tx.execute(
                    "UPDATE swarmie_tasks_meta SET sequence=?1 WHERE singleton=1",
                    [prepared.sequence],
                )
                .map_err(sql_error)?;
            }
            for command in &mut domain {
                command.apply(&tx).map_err(StorageError::Refused)?;
            }
            let outcome = match tx.commit() {
                Ok(()) => CommitOutcome::Applied { grants },
                Err(error) => CommitOutcome::Unknown {
                    message: error.to_string(),
                },
            };
            drop(stored);
            if matches!(outcome, CommitOutcome::Unknown { .. }) {
                self.owner().require_recovery();
            } else {
                if notify {
                    self.changes().invalidate();
                }
                for command in domain {
                    command.notify();
                }
            }
            Ok(outcome)
        }
        .boxed_local()
    }
}
