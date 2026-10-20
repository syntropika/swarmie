use std::collections::BTreeSet;

use swarmie_plugin_contracts::{
    AdminOperation, DesiredState, Inventory, ObservedState, OperationStatus,
};

use crate::{ManagerError, PluginManager, Result};

impl PluginManager {
    /// Activates installed Dynamic packages together through one Tembra declaration batch.
    /// Unrelated package selections and installed Facets are preserved. Every package must
    /// already have approved cached bytes; a previously admitted active package uses reload.
    ///
    /// # Errors
    /// Refuses duplicates, missing/Core packages and already admitted active packages before
    /// installation. Preparation/startup failures record all batch participants conservatively
    /// and attempt logical foreign retirement. Cancellation or cleanup uncertainty requires a
    /// fresh Host; no rollback of external effects or physical termination is promised.
    pub async fn activate_batch(&self, plugin_ids: &[String]) -> Result<Inventory> {
        self.require_composition_reconciliation()?;
        let _guard = self.guard()?;
        if plugin_ids.is_empty() {
            return self.snapshot();
        }
        if plugin_ids.iter().collect::<BTreeSet<_>>().len() != plugin_ids.len() {
            return Err(ManagerError::InvalidPackage(
                "duplicate activation package".into(),
            ));
        }
        let mut plugins = plugin_ids
            .iter()
            .map(|id| self.dynamic(id))
            .collect::<Result<Vec<_>>>()?;
        if plugins.iter().any(|plugin| {
            !plugin.activation_id.is_empty() && plugin.desired == DesiredState::Active
        }) {
            return Err(ManagerError::InvalidPackage(
                "an admitted active package requires reload".into(),
            ));
        }
        let mut operations = plugin_ids
            .iter()
            .map(|id| self.begin(id, AdminOperation::Activate))
            .collect::<Result<Vec<_>>>()?;
        self.fresh_session.set(false);
        for plugin in &mut plugins {
            plugin.desired = DesiredState::Active;
            plugin.observed = ObservedState::Unavailable;
            plugin.activation_id.clear();
            self.storage()?.save(plugin)?;
        }
        let result: Result<()> = async {
            let mut definitions = Vec::new();
            for plugin in &plugins {
                definitions.extend(self.prepared(&plugin.package).await?);
            }
            self.load_initial(definitions).await
        }
        .await;
        let diagnostic = result
            .as_ref()
            .err()
            .map(ToString::to_string)
            .unwrap_or_default();
        let mut recording_error = None;
        for plugin in plugins {
            if let Err(error) = self.record_composed_result(plugin, result.is_ok(), &diagnostic) {
                recording_error.get_or_insert(error);
            }
        }
        for operation in &mut operations {
            operation.status = if result.is_ok() {
                OperationStatus::Succeeded
            } else {
                OperationStatus::Failed
            };
            operation.error = diagnostic.clone();
            if let Err(error) = self.storage()?.operation(operation) {
                recording_error.get_or_insert(error);
            }
        }
        if let Some(recording) = recording_error {
            return Err(match result {
                Err(failure) => self.failure_unrecorded(failure, recording),
                Ok(()) => recording,
            });
        }
        result?;
        self.snapshot()
    }
}
