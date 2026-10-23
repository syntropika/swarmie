//! Disposable scheduling hints. Storage records and their revisions remain authoritative.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    time::Duration,
};

use swarmie_task_contracts::{TaskRecord, TaskStatus};

#[derive(Clone)]
struct Hint {
    revision: u64,
    status: TaskStatus,
    parent: Option<String>,
    wake: Option<u64>,
}

#[derive(Default)]
pub(crate) struct Work {
    hints: BTreeMap<String, Hint>,
    pending: VecDeque<String>,
    queued: BTreeSet<String>,
    deadlines: BTreeMap<u64, BTreeSet<String>>,
    pub initialized: bool,
    pub sequence: u64,
    pub next_probe: Duration,
    pub rescan: bool,
    pub recover: BTreeSet<String>,
}

impl Work {
    pub fn schedule(&mut self, id: String) {
        if self.queued.insert(id.clone()) {
            self.pending.push_back(id);
        }
    }

    pub fn prioritize(&mut self, id: &str) {
        if self.hints.contains_key(id) {
            self.pending.retain(|queued| queued != id);
            self.queued.insert(id.to_owned());
            self.pending.push_front(id.to_owned());
        }
    }

    pub fn settled(&mut self, id: &str) {
        if self.hints.contains_key(id) {
            self.schedule(id.to_owned());
        }
    }

    pub fn pop(&mut self) -> Option<String> {
        loop {
            let id = self.pending.pop_front()?;
            self.queued.remove(&id);
            // Terminal observations remove their hints. A previously queued identity
            // cannot become live again, so it needs no redundant storage read.
            if self.hints.contains_key(&id) {
                return Some(id);
            }
        }
    }

    pub fn observe(&mut self, task: &TaskRecord) {
        self.record(task, true);
    }

    /// Records this engine's own Running reservation of an in-flight root without queuing it.
    /// Its managed execution queues the task again when it settles, and a later cancellation
    /// commit is observed on its own, so a pass now could only reread this reservation.
    pub fn observe_reservation(&mut self, task: &TaskRecord) {
        self.record(task, false);
    }

    fn record(&mut self, task: &TaskRecord, queue: bool) {
        if self
            .hints
            .get(&task.id)
            .is_some_and(|hint| hint.revision > task.revision)
        {
            return;
        }
        if let Some(parent) = &task.parent {
            self.schedule(parent.clone());
        }
        if let Some(wake) = self.hints.get(&task.id).and_then(|hint| hint.wake)
            && let Some(ids) = self.deadlines.get_mut(&wake)
        {
            ids.remove(&task.id);
            if ids.is_empty() {
                self.deadlines.remove(&wake);
            }
        }
        if task.is_terminal() {
            self.hints.remove(&task.id);
            self.recover.remove(&task.id);
        } else {
            self.hints.insert(
                task.id.clone(),
                Hint {
                    revision: task.revision,
                    status: task.status,
                    parent: task.parent.clone(),
                    wake: task.wake_at_ms,
                },
            );
            if queue {
                self.schedule(task.id.clone());
            }
            if task.status == TaskStatus::Sleeping
                && let Some(wake) = task.wake_at_ms
            {
                self.deadlines
                    .entry(wake)
                    .or_default()
                    .insert(task.id.clone());
            }
        }
        if task.cancellation_requested
            || matches!(
                task.outcome,
                Some(
                    swarmie_task_contracts::TaskOutcome::Failed { .. }
                        | swarmie_task_contracts::TaskOutcome::Aborted { .. }
                        | swarmie_task_contracts::TaskOutcome::Faulted { .. }
                )
            )
        {
            let children: Vec<_> = self
                .hints
                .iter()
                .filter(|(_, hint)| hint.parent.as_deref() == Some(&task.id))
                .map(|(id, _)| id.clone())
                .collect();
            for child in children {
                self.schedule(child);
            }
        }
    }

    pub fn discover(&mut self, records: &[TaskRecord], sequence: u64, startup: bool) -> bool {
        if self.sequence > sequence {
            return false;
        }
        self.hints.clear();
        self.deadlines.clear();
        for task in records {
            self.observe(task);
            if startup && task.status == TaskStatus::Running {
                self.recover.insert(task.id.clone());
            }
        }
        self.sequence = sequence;
        self.initialized = true;
        self.rescan = false;
        true
    }

    /// Whether a task last observed as Pending, other than the given in-flight executions,
    /// waits for an execution slot.
    pub fn runnable_waiting(&self, inflight: &BTreeSet<String>) -> bool {
        self.hints
            .iter()
            .any(|(id, hint)| hint.status == TaskStatus::Pending && !inflight.contains(id))
    }

    pub fn definitions_changed(&mut self) {
        let ids: Vec<_> = self
            .hints
            .iter()
            .filter(|(_, hint)| hint.status == TaskStatus::Blocked)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            self.schedule(id);
        }
    }

    pub fn due(&mut self, now: u64) {
        for _ in 0..128 {
            let Some((&deadline, _)) = self.deadlines.first_key_value() else {
                break;
            };
            if deadline > now {
                break;
            }
            let mut entry = self
                .deadlines
                .first_entry()
                .expect("observed deadline entry");
            let id = entry.get_mut().pop_first();
            if entry.get().is_empty() {
                entry.remove();
            }
            if let Some(id) = id {
                self.schedule(id);
            }
        }
    }
}
