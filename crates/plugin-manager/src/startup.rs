use std::collections::BTreeSet;

use swarmie_plugin_contracts::*;
use tembra::{NativeFacet, core};

use crate::{ManagerError, PluginManager, Result, store::CachedPlugin};

/// One selected compiled package and its executable Facets, ready for a shared Host batch.
/// This carries application metadata and actual Tembra definitions, not a second lifecycle.
pub struct CorePluginDeployment {
    pub plugin_id: String,
    pub version: String,
    pub facets: Vec<NativeFacet>,
}

impl PluginManager {
    /// Replays durable active package selections together on the fresh Host.
    /// Tembra resolves actual object dependencies inside the shared declaration batch.
    pub async fn recover(&self) -> Result<Inventory> {
        let selected = self
            .storage()?
            .all()?
            .into_iter()
            .filter(|plugin| plugin.desired == DesiredState::Active)
            .map(|plugin| plugin.package.manifest.id)
            .collect::<Vec<_>>();
        self.recover_composed(vec![], &selected).await
    }

    /// Recovers selected application packages through one mixed Tembra declaration batch.
    ///
    /// All native and foreign providers are declared before any eager factory starts. Tembra
    /// owns object dependency readiness, including Core consumers of Dynamic providers. A
    /// failed initial batch can leave installed work; aggregate failure never proves rollback
    /// or establishes that any unobserved package is active.
    pub async fn recover_composed(
        &self,
        cores: Vec<CorePluginDeployment>,
        selected_ids: &[String],
    ) -> Result<Inventory> {
        self.require_composition_reconciliation()?;
        if !self.fresh_session.get() {
            return Err(ManagerError::RecoveryRequired);
        }
        let unique: BTreeSet<_> = selected_ids.iter().collect();
        if unique.len() != selected_ids.len() {
            return Err(ManagerError::InvalidPackage(
                "duplicate selected package identity".into(),
            ));
        }
        for id in selected_ids {
            if self
                .composition_selection
                .borrow()
                .as_ref()
                .is_some_and(|scope| {
                    !scope.contains(id) || !self.composition_staged.borrow().contains(id)
                })
            {
                return Err(ManagerError::CompositionManaged);
            }
            self.dynamic(id)?;
        }
        // Reserve and validate compiled ownership before execution. These records immediately
        // become Unavailable; only the awaited shared startup can establish Active.
        let core_ids = cores
            .iter()
            .map(|plugin| plugin.plugin_id.clone())
            .collect::<Vec<_>>();
        let mut definitions = vec![];
        for plugin in cores {
            let names = plugin
                .facets
                .iter()
                .map(|facet| facet.core().name().to_string())
                .collect();
            self.record_core_facets(plugin.plugin_id.clone(), plugin.version, names)
                .await?;
            if let Some(record) = self
                .cores
                .borrow_mut()
                .iter_mut()
                .find(|record| record.plugin_id == plugin.plugin_id)
            {
                record.observed = ObservedState::Unavailable;
                record.errors =
                    vec!["composed startup has not established runtime readiness".into()];
            }
            definitions.extend(
                plugin
                    .facets
                    .into_iter()
                    .map(|facet| core::ExecutableFacet::from(facet.core().clone())),
            );
        }
        let _guard = self.recovery_guard()?;
        for mut operation in self.storage()?.operations()? {
            if matches!(
                operation.status,
                OperationStatus::Pending | OperationStatus::Running
            ) {
                operation.status = OperationStatus::Interrupted;
                operation.error = "previous supervisor session ended before settlement was recorded; external effects are unknown".into();
                self.storage()?.operation(&operation)?;
            }
        }
        let mut selected = vec![];
        for mut plugin in self.storage()?.all()? {
            if unique.contains(&plugin.package.manifest.id) {
                plugin.desired = DesiredState::Active;
                plugin.observed = ObservedState::Unavailable;
                plugin.activation_id.clear();
                self.storage()?.save(&plugin)?;
                selected.push(plugin);
            } else {
                plugin.desired = DesiredState::Installed;
                plugin.observed = ObservedState::Installed;
                plugin.activation_id.clear();
                self.storage()?.save(&plugin)?;
            }
        }
        selected.sort_by_key(|plugin| {
            selected_ids
                .iter()
                .position(|id| *id == plugin.package.manifest.id)
                .unwrap_or(usize::MAX)
        });
        let mut operations = selected
            .iter()
            .map(|plugin| self.begin(&plugin.package.manifest.id, AdminOperation::Recover))
            .collect::<Result<Vec<_>>>()?;
        let result: Result<()> = async {
            for plugin in &selected {
                definitions.extend(self.prepared(&plugin.package).await?);
            }
            if !definitions.is_empty() {
                self.host
                    .core()
                    .load_executable(definitions, core::LoadOptions::default())?
                    .await?;
            }
            Ok(())
        }
        .await;
        let diagnostic = result
            .as_ref()
            .err()
            .map(ToString::to_string)
            .unwrap_or_default();
        for id in core_ids {
            if let Some(record) = self
                .cores
                .borrow_mut()
                .iter_mut()
                .find(|record| record.plugin_id == id)
            {
                record.observed = if result.is_ok() {
                    ObservedState::Active
                } else {
                    ObservedState::Unavailable
                };
                record.errors = if diagnostic.is_empty() {
                    vec![]
                } else {
                    vec![diagnostic.clone()]
                };
            }
        }
        for plugin in selected {
            self.record_composed_result(plugin, result.is_ok(), &diagnostic)?;
        }
        for operation in &mut operations {
            operation.status = if result.is_ok() {
                OperationStatus::Succeeded
            } else {
                OperationStatus::Failed
            };
            operation.error = diagnostic.clone();
            self.storage()?.operation(operation)?;
        }
        result?;
        self.snapshot()
    }

    fn record_composed_result(
        &self,
        mut plugin: CachedPlugin,
        succeeded: bool,
        diagnostic: &str,
    ) -> Result<()> {
        plugin.observed = if succeeded {
            if plugin
                .package
                .manifest
                .facets
                .iter()
                .any(|facet| facet.target == FacetTarget::Server)
            {
                ObservedState::Active
            } else {
                ObservedState::Installed
            }
        } else {
            ObservedState::Failed
        };
        plugin.error = diagnostic.into();
        plugin.activation_id = if succeeded {
            uuid::Uuid::new_v4().to_string()
        } else {
            String::new()
        };
        if let Err(error) = self.storage()?.save(&plugin) {
            return if succeeded {
                Err(self.publication_unrecorded(&plugin.package, plugin.desired, error))
            } else {
                Err(error)
            };
        }
        Ok(())
    }
}
