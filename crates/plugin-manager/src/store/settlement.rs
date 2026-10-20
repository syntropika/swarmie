use rusqlite::params;
use swarmie_plugin_contracts::{BrowserObservation, OperationRecord};

use super::{CachedPlugin, Store};
use crate::Result;

impl Store {
    /// Settles package selection, operation acknowledgement and browser retirement together.
    /// Approved immutable bytes can be retained before this transaction, independently of
    /// installed selection. Failure never publishes only one half of a replacement pair.
    pub(crate) fn settle(
        &self,
        plugins: &[CachedPlugin],
        operations: &[OperationRecord],
        retired: &[String],
    ) -> Result<()> {
        for plugin in plugins {
            self.retain_version(&plugin.package)?;
        }
        let mut sessions = self.browser_sessions()?;
        sessions.retain(|session| {
            retired.contains(&session.plugin_id) && session.observed != BrowserObservation::Inactive
        });
        self.with(|connection| {
            let transaction = connection.unchecked_transaction()?;
            for plugin in plugins {
                transaction.execute(
                    "INSERT INTO installed_plugins(plugin_id,version,desired,observed,error,activation_id)
                     VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(plugin_id) DO UPDATE SET
                     version=excluded.version, desired=excluded.desired, observed=excluded.observed,
                     error=excluded.error, activation_id=excluded.activation_id",
                    params![plugin.package.manifest.id, plugin.package.manifest.version,
                        serde_json::to_string(&plugin.desired)?, serde_json::to_string(&plugin.observed)?,
                        plugin.error, plugin.activation_id],
                )?;
            }
            for operation in operations {
                transaction.execute(
                    "INSERT INTO operations(id,plugin_id,operation,status,error) VALUES (?1,?2,?3,?4,?5)
                     ON CONFLICT(id) DO UPDATE SET status=excluded.status,error=excluded.error",
                    params![operation.id, operation.plugin_id, serde_json::to_string(&operation.operation)?,
                        serde_json::to_string(&operation.status)?, operation.error],
                )?;
            }
            for session in &mut sessions {
                let pending = "browser removal requested; awaiting browser acknowledgement";
                if !session.error.contains(pending) {
                    session.error = if session.error.is_empty() { pending.into() }
                        else { format!("{}; {pending}", session.error) };
                }
                transaction.execute(
                    "UPDATE browser_sessions SET record=?1 WHERE plugin_id=?2 AND session_id=?3 AND activation_id=?4",
                    params![serde_json::to_string(session)?, session.plugin_id, session.session_id, session.activation_id],
                )?;
            }
            transaction.commit()?;
            Ok(())
        })
    }
}
