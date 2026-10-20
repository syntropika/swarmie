use std::collections::BTreeSet;

use swarmie_plugin_contracts::*;

use crate::{ManagerError, PluginManager, Result};

impl PluginManager {
    /// Filters cached desired activation before recovery on a fresh Host.
    /// Unselected packages remain cached and installed; their executable facets are not replayed.
    pub async fn recover_selected(
        &self,
        proof: &AdminProof,
        selected_ids: &BTreeSet<String>,
    ) -> Result<Inventory> {
        self.recover_selected_ordered(proof, &selected_ids.iter().cloned().collect::<Vec<_>>())
            .await
    }

    /// Replays selected packages in the already resolved application activation order.
    /// This consumes composition metadata; Tembra still owns object dependency readiness.
    pub async fn recover_selected_ordered(
        &self,
        proof: &AdminProof,
        selected_ids: &[String],
    ) -> Result<Inventory> {
        self.authorize(proof)?;
        self.require_composition_permission()?;
        if !self.fresh_session.get() {
            return Err(ManagerError::RecoveryRequired);
        }
        let selected: BTreeSet<_> = selected_ids.iter().collect();
        if selected.len() != selected_ids.len() {
            return Err(ManagerError::InvalidPackage(
                "duplicate ordered deployment identity".into(),
            ));
        }
        for id in &selected {
            if self
                .composition_selection
                .borrow()
                .as_ref()
                .is_some_and(|scope| {
                    !scope.contains(*id) || !self.composition_staged.borrow().contains(*id)
                })
            {
                return Err(ManagerError::CompositionManaged);
            }
            self.dynamic(id)?;
        }
        for mut plugin in self.storage()?.all()? {
            plugin.desired = if selected.contains(&plugin.package.manifest.id) {
                DesiredState::Active
            } else {
                DesiredState::Installed
            };
            plugin.observed = if plugin.desired == DesiredState::Active {
                ObservedState::Unavailable
            } else {
                ObservedState::Installed
            };
            plugin.activation_id.clear();
            self.storage()?.save(&plugin)?;
        }
        self.recover_with_order(proof, Some(selected_ids)).await
    }

    /// Replays active intent from exact cached bytes on the fresh Host without source access.
    ///
    /// Interrupted records are retained as interrupted observations. Replay uses current Host
    /// facts and actual installation results; it never claims to reverse prior external effects.
    /// # Errors
    /// Failed packages remain failed and other independent packages are still attempted. Missing
    /// dependencies and cycles are reported rather than synthesized as available capabilities.
    pub async fn recover(&self, proof: &AdminProof) -> Result<Inventory> {
        self.recover_with_order(proof, None).await
    }

    async fn recover_with_order(
        &self,
        proof: &AdminProof,
        order: Option<&[String]>,
    ) -> Result<Inventory> {
        self.authorize(proof)?;
        let _guard = self.recovery_guard()?;
        for mut operation in self.storage()?.operations()? {
            if matches!(
                operation.status,
                OperationStatus::Pending | OperationStatus::Running
            ) {
                operation.status = OperationStatus::Interrupted;
                operation.error="previous supervisor session ended before settlement was recorded; external effects are unknown".into();
                self.storage()?.operation(&operation)?;
            }
        }
        let mut pending = self
            .storage()?
            .all()?
            .into_iter()
            .filter(|plugin| {
                plugin.desired == DesiredState::Active && plugin.observed != ObservedState::Active
            })
            .collect::<Vec<_>>();
        if let Some(order) = order {
            pending.sort_by_key(|plugin| {
                order
                    .iter()
                    .position(|id| *id == plugin.package.manifest.id)
                    .unwrap_or(usize::MAX)
            });
        }
        let mut failure = None;
        while !pending.is_empty() {
            let index = pending
                .iter()
                .position(|plugin| self.dependencies(&plugin.package, true).is_ok());
            let Some(index) = index else {
                for mut plugin in pending.drain(..) {
                    plugin.observed = ObservedState::Failed;
                    plugin.error = "activation dependencies are unavailable or cyclic".into();
                    self.storage()?.save(&plugin)?;
                }
                failure = Some(ManagerError::Dependency(
                    "recovery dependencies are unavailable or cyclic".into(),
                ));
                break;
            };
            let plugin = pending.remove(index);
            let operation = self.begin(&plugin.package.manifest.id, AdminOperation::Recover)?;
            let result = self.activate_cached(plugin).await;
            if let Err(error) = self.finish(operation, result).await {
                failure = Some(error);
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
        self.snapshot()
    }
}
