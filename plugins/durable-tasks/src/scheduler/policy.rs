use crate::{ExecutorId, TaskFailure, TaskId, TaskSummary};
use futures::{future::LocalBoxFuture, task::LocalSpawn};
use serde::{Deserialize, Serialize};
use std::{num::NonZeroU64, rc::Rc, time::Duration};

/// Creates an embedding-owned delay without starting a timer thread.
pub type Timer = Rc<dyn Fn(Duration) -> LocalBoxFuture<'static, ()>>;

/// Orders one bounded page of eligible task metadata.
pub trait SchedulerStrategy {
    /// Pure ordering of the offered metadata window; return each candidate exactly once.
    fn order(&self, candidates: &[TaskSummary]) -> tembra::Result<Vec<TaskId>>;
}

/// Orders candidates by their persisted creation sequence.
pub struct Fifo;

impl SchedulerStrategy for Fifo {
    fn order(&self, candidates: &[TaskSummary]) -> tembra::Result<Vec<TaskId>> {
        let mut candidates: Vec<_> = candidates.iter().collect();
        candidates.sort_by_key(|t| t.sequence);
        Ok(candidates.into_iter().map(|t| t.id).collect())
    }
}

/// Embedding-owned policy. It creates no executor or timer thread.
#[derive(Clone)]
pub struct Options {
    /// Stable identity of this executor incarnation.
    pub executor: ExecutorId,
    /// Local physical executions, including completed handlers still settling; zero is unlimited.
    pub max_executions: usize,
    /// Finite authority-clock duration. Overflow is a typed acquisition refusal, not infinity.
    pub lease_duration_ms: NonZeroU64,
    /// Application timer, kept operational until physical shutdown settles.
    pub timer: Timer,
    /// Pure ordering within the currently offered bounded metadata window.
    pub strategy: Rc<dyn SchedulerStrategy>,
    pub(crate) spawner: Rc<dyn LocalSpawn>,
}

impl Options {
    /// Uses the application's executor. Zero capacity means no local configured limit.
    pub fn new(
        spawner: impl LocalSpawn + 'static,
        lease_duration_ms: NonZeroU64,
        timer: Timer,
    ) -> Self {
        Self {
            executor: ExecutorId::new(),
            max_executions: 0,
            lease_duration_ms,
            timer,
            strategy: Rc::new(Fifo),
            spawner: Rc::new(spawner),
        }
    }
}

/// Reports the latest scheduling or assignment failure.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SchedulerDiagnostic {
    Storage(TaskFailure),
    Strategy(TaskFailure),
    Definition(TaskFailure),
    Rejected {
        task: TaskId,
        reason: crate::Rejection,
    },
    Interrupted {
        task: TaskId,
        error: TaskFailure,
    },
}

/// Local observation only. Storage commits grant assignment authority.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SchedulerStatus {
    /// Whether resume has enabled admissions.
    pub enabled: bool,
    /// Handlers and managed writes not yet physically settled.
    pub active: usize,
    /// Latest diagnostic; an explicit recovery wake may clear it.
    pub diagnostic: Option<SchedulerDiagnostic>,
    /// At most 128 identities unavailable in the current metadata sweep.
    pub blocked: Vec<TaskId>,
    /// Local interrupted identities held until an explicit recovery wake.
    pub interrupted: Vec<TaskId>,
}
