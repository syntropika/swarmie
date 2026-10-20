use swarmie_plugin_contracts::*;

use crate::{
    ManagerError, PluginManager, Result, store::CachedPlugin, validation::validate_package,
};

impl PluginManager {
    /// Stages an exact approved release for a fresh composition reconciliation without execution.
    /// Cached approved bytes support offline reconciliation. The caller must compare package
    /// metadata with its resolved catalog before permitting recovery of the staged selection.
    pub async fn stage_release(&self, request: InstallRequest) -> Result<PluginManifest> {
        self.authorize(&request.proof)?;
        if !self.composition_reconciliation_active() || !self.fresh_session.get() {
            return Err(ManagerError::CompositionManaged);
        }
        if self
            .composition_selection
            .borrow()
            .as_ref()
            .is_some_and(|selected| !selected.contains(&request.plugin_id))
        {
            return Err(ManagerError::CompositionManaged);
        }
        if !request.confirmed {
            return Err(ManagerError::ConfirmationRequired);
        }
        semver::Version::parse(&request.version)
            .map_err(|error| ManagerError::InvalidPackage(error.to_string()))?;
        if self
            .cores
            .borrow()
            .iter()
            .any(|core| core.plugin_id == request.plugin_id)
        {
            return Err(ManagerError::CorePlugin(request.plugin_id));
        }
        let _guard = self.guard()?;
        let operation = self.begin(&request.plugin_id, AdminOperation::Install)?;
        let mut manifest = None;
        let result: Result<()> = async {
            let package = match self
                .storage()?
                .cached_version(&request.plugin_id, &request.version)?
            {
                Some(package) => {
                    validate_package(&package)?;
                    if package.provenance.provider_id != request.provider_id {
                        return Err(ManagerError::Integrity(
                            "cached release belongs to a different source".into(),
                        ));
                    }
                    self.admission(&package)?;
                    package
                }
                None => {
                    self.acquire(
                        &request.proof,
                        &request.provider_id,
                        &request.plugin_id,
                        &request.version,
                    )
                    .await?
                }
            };
            manifest = Some(package.manifest.clone());
            self.storage()?.save(&CachedPlugin {
                package,
                desired: DesiredState::Active,
                observed: ObservedState::Unavailable,
                error: "staged composition awaits runtime activation".into(),
                activation_id: String::new(),
            })
        }
        .await;
        self.finish(operation, result).await?;
        self.composition_staged
            .borrow_mut()
            .insert(request.plugin_id);
        manifest
            .ok_or_else(|| ManagerError::InvalidPackage("staged release has no manifest".into()))
    }
}
