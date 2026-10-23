use serde::{Deserialize, Serialize};
use tembra::{Result, RuntimeError};

/// Scheduling limits. Wake timestamps are persisted as UTC milliseconds. `poll_interval_ms`
/// bounds the delay before observing an elapsed wake time, paces contention retries and checks
/// scheduler liveness for waiting callers. Waiting callers also reread at this interval to observe
/// external writers. This scheduler's own commits wake its loop and the callers waiting on a
/// terminal task immediately; global discovery uses a separate bounded sequence probe.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Configuration {
    pub restart_limit: u32,
    pub poll_interval_ms: u64,
    pub max_parallel: u32,
}

impl Default for Configuration {
    fn default() -> Self {
        Self {
            restart_limit: 3,
            poll_interval_ms: 50,
            max_parallel: 16,
        }
    }
}

impl Configuration {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.restart_limit == 0
            || self.restart_limit > 1_000
            || !(1..=60_000).contains(&self.poll_interval_ms)
            || !(1..=128).contains(&self.max_parallel)
        {
            return Err(RuntimeError::invalid(
                "durable_tasks_configuration",
                "restart_limit must be 1..=1000, poll_interval_ms 1..=60000, and max_parallel 1..=128",
            ));
        }
        Ok(())
    }
}
