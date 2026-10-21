//! Cancellation walks only the owned live subtree and batches monotone intent with settlement.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::Duration;

use swarmie_task_contracts::{
    CommitResult, MAX_PORTABLE_INTEGER, MAX_TASK_PAGE_BYTES, TaskOutcome, TaskQuery, TaskRecord,
    TaskStatus, TaskTransaction, TaskWrite, task_wire_bytes,
};
use tembra::{Context, Result, RuntimeError};

use crate::{ownership, reads::ACTIVE, reads::busy, scheduler::Engine};

impl Engine {
    /// The operation-scoped snapshot is discarded after CAS. No subtree records survive as
    /// scheduler state; a subsequent pass rereads any unfinished work from TaskStorage.
    pub async fn cancel_owned(&self, context: &Context, id: &str) -> Result<()> {
        let before = self.sequence(context).await?;
        let root = self.require(context, id).await?;
        if root.is_terminal() {
            return Ok(());
        }
        let mut records = BTreeMap::from([(id.to_owned(), root)]);
        let mut children = BTreeMap::<String, Vec<String>>::new();
        let mut pending = VecDeque::from([id.to_owned()]);
        let mut order = Vec::new();
        // Bound each pass. Discovered but unexpanded descendants receive cancellation
        // intent only; their own queued pass establishes child settlement before terminality.
        for index in 0..128 {
            let Some(id) = pending.pop_front() else { break };
            let (sequence, owned) = self
                .scan_records(
                    context,
                    TaskQuery {
                        limit: 256,
                        statuses: ACTIVE.to_vec(),
                        parent: Some(id.clone()),
                        ..Default::default()
                    },
                )
                .await?;
            if sequence != before {
                return Err(busy());
            }
            children.insert(
                id.clone(),
                owned.iter().map(|task| task.id.clone()).collect(),
            );
            order.push(id);
            for child in owned {
                if records.insert(child.id.clone(), child.clone()).is_some() {
                    return Err(RuntimeError::invalid(
                        "durable_tasks_ownership",
                        "ownership subtree contains a cycle",
                    ));
                }
                pending.push_back(child.id);
            }
            if index % 16 == 15 {
                self.driver.sleep(Duration::from_millis(1)).await;
            }
        }
        // The last page fenced all preceding reads. A yield after that page requires a
        // new closing probe before any decisions use this snapshot.
        if self.sequence(context).await? != before {
            return Err(busy());
        }
        let inflight: BTreeSet<_> = self.inflight.borrow().clone();
        let mut projected = records.clone();
        for record in projected.values_mut() {
            record.cancellation_requested = true;
        }
        for id in order.iter().rev() {
            if inflight.contains(id) {
                continue;
            }
            let owned: Vec<_> = children[id]
                .iter()
                .map(|id| projected[id].clone())
                .collect();
            let record = projected.get_mut(id).expect("expanded task is in snapshot");
            if record.status == TaskStatus::Completing {
                if owned.iter().all(TaskRecord::is_terminal) {
                    record.status = TaskStatus::Terminal;
                }
            } else {
                ownership::settle(
                    record,
                    &owned,
                    TaskOutcome::Aborted {
                        reason: "cancellation requested".into(),
                    },
                );
            }
        }
        // Expanded descendants precede parents, satisfying ownership across batches.
        // Unexpanded descendants receive intent only, keeping their parents Completing.
        // Conflicts abandon remaining writes and require a fresh subtree snapshot.
        order.reverse();
        order.extend(pending);
        let mut writes = Vec::new();
        let transaction_base = task_wire_bytes(&TaskTransaction {
            owner: self.owner.value()?.id.clone(),
            writes: vec![],
        })?;
        let receipt_base = task_wire_bytes(&CommitResult::Committed {
            sequence: MAX_PORTABLE_INTEGER,
            records: vec![],
        })?;
        let mut transaction_bytes = transaction_base;
        let mut receipt_bytes = receipt_base;
        for id in order {
            let mut record = projected.remove(&id).expect("task visited once");
            let old = &records[&id];
            if &record == old {
                continue;
            }
            record.revision = old.revision.checked_add(1).ok_or_else(|| {
                RuntimeError::invalid("durable_tasks_revision", "task revision is exhausted")
            })?;
            let write = TaskWrite {
                expected_revision: Some(old.revision),
                record,
            };
            let write_bytes = task_wire_bytes(&write)?;
            let record_bytes = task_wire_bytes(&write.record)?;
            let comma = usize::from(!writes.is_empty());
            if !writes.is_empty()
                && (writes.len() >= 64
                    || transaction_bytes + comma + write_bytes > MAX_TASK_PAGE_BYTES
                    || receipt_bytes + comma + record_bytes > MAX_TASK_PAGE_BYTES)
                && !self
                    .cancel_batch(context, std::mem::take(&mut writes))
                    .await?
            {
                self.schedule(id.as_str());
                return Ok(());
            }
            if writes.is_empty() {
                transaction_bytes = transaction_base;
                receipt_bytes = receipt_base;
            }
            let comma = usize::from(!writes.is_empty());
            transaction_bytes += comma + write_bytes;
            receipt_bytes += comma + record_bytes;
            writes.push(write);
        }
        if !writes.is_empty() {
            self.cancel_batch(context, writes).await?;
        }
        Ok(())
    }

    async fn cancel_batch(&self, context: &Context, writes: Vec<TaskWrite>) -> Result<bool> {
        let ids: Vec<_> = writes.iter().map(|write| write.record.id.clone()).collect();
        let result = match self.commit(context, writes).await {
            // An unseen child can invalidate a terminal relationship without changing the
            // parent's revision. No handler ran here: abandon the stale read and retry.
            Err(error) if error.reason == "task_storage_invalid" => {
                for id in ids {
                    self.schedule(&id);
                }
                return Err(busy());
            }
            result => result?,
        };
        match result {
            CommitResult::Conflict { .. } => {
                for id in ids {
                    self.schedule(&id);
                }
                Ok(false)
            }
            CommitResult::Committed { records, .. } => {
                for record in records {
                    if let Some(token) = self.active.borrow().get(&record.id).cloned() {
                        token.cancel(RuntimeError::aborted(
                            "task_cancelled",
                            "owned task cancellation requested",
                        ));
                    }
                }
                Ok(true)
            }
        }
    }
}
