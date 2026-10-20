use std::collections::BTreeSet;

use swarmie_plugin_contracts::*;

use crate::{ManagerError, PluginManager, Result};

impl PluginManager {
    /// Records an already admitted Core Plugin whose sole facet has the same identity.
    pub async fn record_core(&self, proof: &AdminProof, id: String, version: String) -> Result<()> {
        self.record_core_facets(proof, id.clone(), version, vec![id])
            .await
    }

    /// Records a compiled logical plugin and the actual facets admitted by its caller.
    /// Facet identities are distinct from package identities and cannot share ownership.
    pub async fn record_core_facets(
        &self,
        proof: &AdminProof,
        id: String,
        version: String,
        facets: Vec<String>,
    ) -> Result<()> {
        self.authorize(proof)?;
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
