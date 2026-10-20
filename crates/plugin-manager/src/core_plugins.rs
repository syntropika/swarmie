use std::collections::BTreeSet;

use swarmie_plugin_contracts::*;
use tembra::core;

use crate::{CorePluginDeployment, ManagerError, PluginManager, Result};

impl PluginManager {
    /// Starts compiled acquisition infrastructure before the final mixed application batch.
    /// The fresh session remains available for [`Self::recover_composed`] afterwards.
    ///
    /// # Errors
    /// Refuses a nonfresh or uncertain session. Partial native startup cannot be unloaded;
    /// its typed cause and Unavailable observations require a fresh Host before more work.
    pub async fn install_core_batch(&self, cores: Vec<CorePluginDeployment>) -> Result<()> {
        self.require_composition_reconciliation()?;
        if !self.fresh_session.get() {
            return Err(ManagerError::RecoveryRequired);
        }
        let _guard = self.guard()?;
        let (ids, definitions) = self.reserve_cores(cores).await?;
        let result = self.load_initial(definitions).await;
        let diagnostic = result
            .as_ref()
            .err()
            .map(ToString::to_string)
            .unwrap_or_default();
        self.settle_cores(&ids, result.is_ok(), &diagnostic);
        result
    }

    pub(crate) async fn reserve_cores(
        &self,
        cores: Vec<CorePluginDeployment>,
    ) -> Result<(Vec<String>, Vec<core::ExecutableFacet>)> {
        let mut ids = Vec::new();
        let mut definitions = Vec::new();
        for plugin in cores {
            let names = plugin
                .facets
                .iter()
                .map(|facet| facet.core().name().to_owned())
                .collect();
            self.record_core_facets(plugin.plugin_id.clone(), plugin.version, names)
                .await?;
            self.settle_cores(
                std::slice::from_ref(&plugin.plugin_id),
                false,
                "startup has not established runtime readiness",
            );
            ids.push(plugin.plugin_id);
            definitions.extend(
                plugin
                    .facets
                    .into_iter()
                    .map(|facet| core::ExecutableFacet::from(facet.core().clone())),
            );
        }
        Ok((ids, definitions))
    }

    pub(crate) fn settle_cores(&self, ids: &[String], succeeded: bool, diagnostic: &str) {
        for record in self
            .cores
            .borrow_mut()
            .iter_mut()
            .filter(|record| ids.contains(&record.plugin_id))
        {
            record.observed = if succeeded {
                ObservedState::Active
            } else {
                ObservedState::Unavailable
            };
            record.errors = if diagnostic.is_empty() {
                vec![]
            } else {
                vec![diagnostic.into()]
            };
        }
    }

    /// Records an already admitted Core Plugin whose sole facet has the same identity.
    pub async fn record_core(&self, id: String, version: String) -> Result<()> {
        self.record_core_facets(id.clone(), version, vec![id]).await
    }

    /// Records a compiled logical plugin and the actual facets admitted by its caller.
    /// Facet identities are distinct from package identities and cannot share ownership.
    pub async fn record_core_facets(
        &self,
        id: String,
        version: String,
        facets: Vec<String>,
    ) -> Result<()> {
        let unique: BTreeSet<_> = facets.iter().cloned().collect();
        if id.is_empty()
            || facets.is_empty()
            || unique.len() != facets.len()
            || unique.iter().any(String::is_empty)
        {
            return Err(ManagerError::InvalidPackage(
                "invalid compiled plugin identities".into(),
            ));
        }
        semver::Version::parse(&version)
            .map_err(|error| ManagerError::InvalidPackage(error.to_string()))?;
        if self.storage()?.load(&id)?.is_some() && self.composition_candidate_relevant(&id) {
            return Err(ManagerError::CorePlugin(id));
        }
        if let Some(existing) = self.core_facets.borrow().get(&id) {
            if *existing == unique
                && self
                    .cores
                    .borrow()
                    .iter()
                    .any(|core| core.plugin_id == id && core.version == version)
            {
                return Ok(());
            }
            return Err(ManagerError::CorePlugin(id));
        }
        if self
            .core_facets
            .borrow()
            .values()
            .any(|owned| !owned.is_disjoint(&unique))
            || self.storage()?.all()?.iter().any(|plugin| {
                self.composition_candidate_relevant(&plugin.package.manifest.id)
                    && plugin
                        .package
                        .manifest
                        .facets
                        .iter()
                        .any(|facet| unique.contains(&facet.id))
            })
        {
            return Err(ManagerError::InvalidPackage(
                "facet identity is already owned by another plugin".into(),
            ));
        }
        self.core_facets.borrow_mut().insert(id.clone(), unique);
        self.cores.borrow_mut().push(PluginRecord {
            plugin_id: id,
            version,
            provider_id: "compiled".into(),
            kind: PluginKind::Core,
            desired: DesiredState::Active,
            observed: ObservedState::Active,
            errors: vec![],
        });
        Ok(())
    }
}
