use swarmie_plugin_contracts::*;

use crate::{
    ManagerError, PluginManager, Result, store::CachedPlugin, validation::validate_package,
};

impl PluginManager {
    /// Verifies an exact candidate and prepares its JavaScript without installing Facets.
    ///
    /// Only immutable approved version-cache bytes may be written. Installed selections,
    /// desired composition and live Facets remain unchanged. Trusted module preparation and
    /// provider acquisition can have external effects; this is not sandboxed execution.
    /// Current package dependency and ownership constraints are checked at actual admission,
    /// so a caller can validate a complete replacement composition before applying it.
    ///
    /// # Errors
    /// Refuses missing confirmation, non-exact versions, untrusted acquisition, changed
    /// provenance, integrity, artifact preparation and cache failures.
    pub async fn validate_release(&self, request: InstallRequest) -> Result<PluginManifest> {
        if !request.confirmed {
            return Err(ManagerError::ConfirmationRequired);
        }
        semver::Version::parse(&request.version)
            .map_err(|error| ManagerError::InvalidPackage(error.to_string()))?;
        let _guard = self.guard()?;
        let package = self
            .approved_release(&request.provider_id, &request.plugin_id, &request.version)
            .await?;
        self.prepare_definitions(&package).await?;
        self.storage()?.retain_version(&package)?;
        Ok(package.manifest)
    }

    pub(crate) async fn approved_release(
        &self,
        provider: &str,
        plugin: &str,
        version: &str,
    ) -> Result<AcquiredPackage> {
        // Immutable approved cache remains usable when its acquisition provider is offline.
        if semver::Version::parse(version).is_ok()
            && let Some(package) = self.storage()?.cached_version(plugin, version)?
        {
            validate_package(&package)?;
            if package.manifest.id != plugin
                || package.manifest.version != version
                || package.provenance.provider_id != provider
            {
                return Err(ManagerError::Integrity(
                    "cached release identity or source differs from the request".into(),
                ));
            }
            return Ok(package);
        }
        self.acquire_verified(provider, plugin, version).await
    }

    /// Replaces the current executable definitions from their immutable approved cache.
    /// Provider connectivity is unnecessary; actual compatibility and retirement use Tembra.
    pub async fn reload_cached(&self, plugin_id: &str) -> Result<Inventory> {
        let plugin = self.dynamic(plugin_id)?;
        self.update(UpdateRequest {
            provider_id: plugin.package.provenance.provider_id,
            plugin_id: plugin_id.into(),
            version: plugin.package.manifest.version,
            confirmed: true,
        })
        .await
    }

    /// Stages an exact approved release for a fresh composition reconciliation without execution.
    /// Cached approved bytes support offline reconciliation. The caller must compare package
    /// metadata with its resolved catalog before permitting recovery of the staged selection.
    pub async fn stage_release(&self, request: InstallRequest) -> Result<PluginManifest> {
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
                    self.acquire(&request.provider_id, &request.plugin_id, &request.version)
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
