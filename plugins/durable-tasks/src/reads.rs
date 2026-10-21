//! Sequence-fenced relation reads exclude unrelated retained terminal history.

use std::collections::BTreeSet;

use swarmie_task_contracts::{TaskQuery, TaskRecord, TaskStatus};
use tembra::{Context, Result, RuntimeError};

use crate::scheduler::Engine;

pub(crate) const ACTIVE: [TaskStatus; 7] = [
    TaskStatus::Pending,
    TaskStatus::Running,
    TaskStatus::Waiting,
    TaskStatus::Sleeping,
    TaskStatus::Completing,
    TaskStatus::Blocked,
    TaskStatus::Parked,
];

pub(crate) struct TaskFamily {
    pub task: TaskRecord,
    pub ancestors: Vec<TaskRecord>,
    pub children: Vec<TaskRecord>,
}

pub(crate) fn is_contention(error: &RuntimeError) -> bool {
    matches!(
        error.reason.as_str(),
        "task_storage_cursor"
            | "task_storage_cursor_invalidated"
            | "durable_tasks_snapshot_busy"
            | "task_storage_outbox_full"
    )
}

pub(crate) fn busy() -> RuntimeError {
    RuntimeError::unavailable(
        "durable_tasks_snapshot_busy",
        "committed relation read changed; retry from current records",
    )
}

impl Engine {
    pub async fn sequence(&self, context: &Context) -> Result<u64> {
        self.storage
            .with_context(context)
            .scan(TaskQuery {
                limit: 1,
                statuses: vec![TaskStatus::Pending],
                ..Default::default()
            })
            .await
            .map(|page| page.sequence)
    }

    pub async fn scan_records(
        &self,
        context: &Context,
        mut query: TaskQuery,
    ) -> Result<(u64, Vec<TaskRecord>)> {
        let mut sequence = None;
        let mut records = Vec::new();
        loop {
            let page = self
                .storage
                .with_context(context)
                .scan(query.clone())
                .await?;
            if sequence.is_some_and(|previous| previous != page.sequence) {
                return Err(busy());
            }
            sequence = Some(page.sequence);
            records.extend(page.records);
            if let Some(next) = page.next {
                query.cursor = Some(next);
                self.driver.sleep(std::time::Duration::from_millis(1)).await;
            } else {
                return Ok((page.sequence, records));
            }
        }
    }

    pub async fn family(&self, context: &Context, id: &str) -> Result<TaskFamily> {
        let before = self.sequence(context).await?;
        let task = self.require(context, id).await?;
        if task.is_terminal() {
            return Ok(TaskFamily {
                task,
                ancestors: vec![],
                children: vec![],
            });
        }
        let mut ancestors = Vec::new();
        let mut parent = task.parent.clone();
        let mut visited = BTreeSet::from([task.id.clone()]);
        while let Some(id) = parent {
            if !visited.insert(id.clone()) {
                return Err(RuntimeError::invalid(
                    "durable_tasks_ownership",
                    "ownership cycle in committed records",
                ));
            }
            let record = self.require(context, &id).await?;
            parent = record.parent.clone();
            ancestors.push(record);
        }
        let (sequence, children) = self
            .scan_records(
                context,
                TaskQuery {
                    limit: 256,
                    parent: Some(task.id.clone()),
                    ..Default::default()
                },
            )
            .await?;
        // The final relation page is also the closing sequence fence: every earlier
        // read follows `before`, and no records are read after this page.
        if sequence != before {
            return Err(busy());
        }
        Ok(TaskFamily {
            task,
            ancestors,
            children,
        })
    }

    pub fn cancelled(&self, family: &TaskFamily) -> bool {
        family.task.cancellation_requested
            || family.ancestors.iter().any(|ancestor| {
                ancestor.cancellation_requested
                    || matches!(
                        ancestor.outcome,
                        Some(
                            swarmie_task_contracts::TaskOutcome::Failed { .. }
                                | swarmie_task_contracts::TaskOutcome::Aborted { .. }
                                | swarmie_task_contracts::TaskOutcome::Faulted { .. }
                        )
                    )
            })
    }
}
