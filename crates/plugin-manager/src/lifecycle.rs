use std::collections::BTreeSet;

use swarmie_plugin_contracts::*;
use tembra::core;
use tembra_executor_js::BundledJsArtifact;

use crate::{
    ManagerError, PluginManager, Result, store::CachedPlugin, validation::validate_package,
};

impl PluginManager {
    /// Acquires and durably caches an explicitly confirmed package without executing it.
    ///
    /// # Errors
    /// Authorization and confirmation are checked before contacting a provider. Integrity,
    /// compatibility and dependency refusals leave installed package selection unchanged.
    pub async fn install(&self, request: InstallRequest) -> Result<Inventory> {
        self.authorize(&request.proof)?;
        if !request.confirmed {
            return Err(ManagerError::ConfirmationRequired);
        }
        let _guard = self.guard()?;
        if self
            .cores
            .borrow()
            .iter()
            .any(|core| core.plugin_id == request.plugin_id)
        {
            return Err(ManagerError::CorePlugin(request.plugin_id));
        }
        if self.storage()?.load(&request.plugin_id)?.is_some() {
            return Err(ManagerError::InvalidPackage(
                "plugin is already installed; use update".into(),
            ));
        }
        let operation = self.begin(&request.plugin_id, AdminOperation::Install)?;
        let result: Result<()> = async {
            let package = self
                .acquire(
                    &request.proof,
                    &request.provider_id,
                    &request.plugin_id,
                    &request.version,
                )
                .await?;
            self.storage()?.save(&CachedPlugin {
                package,
                desired: DesiredState::Installed,
                observed: ObservedState::Installed,
                error: String::new(),
                activation_id: String::new(),
            })
        }
        .await;
        self.finish(operation, result).await
    }

    async fn prepared(&self, package: &AcquiredPackage) -> Result<Vec<core::ExecutableFacet>> {
        validate_package(package)?;
        self.dependencies(package, true)?;
        let mut facets = vec![];
        for descriptor in &package.manifest.facets {
            if descriptor.target != FacetTarget::Server {
                continue;
            }
            let file = package
                .files
                .iter()
                .find(|file| file.path == descriptor.artifact_path)
                .ok_or_else(|| ManagerError::Integrity(descriptor.artifact_path.clone()))?;
            let text = std::str::from_utf8(&file.bytes)
                .map_err(|error| ManagerError::InvalidPackage(error.to_string()))?;
            let artifact = BundledJsArtifact::from_json(text)?;
            let facet = self
                .executor
                .prepare(
                    self.host.core().id(),
                    artifact,
                    descriptor.selected_export.clone(),
                )
                .await?;
            if facet.name() != descriptor.id {
                return Err(ManagerError::Integrity(
                    "prepared facet identity differs from manifest".into(),
                ));
            }
            facets.push(facet);
        }
        Ok(facets)
    }

    async fn activate_cached(&self, mut plugin: CachedPlugin) -> Result<()> {
        plugin.desired = DesiredState::Active;
        self.storage()?.save(&plugin)?;
        let result: Result<()> = async {
            let facets = self.prepared(&plugin.package).await?;
            if !facets.is_empty() {
                self.host
                    .core()
                    .load_executable(facets, core::LoadOptions::default())?
                    .await?;
            }
            Ok(())
        }
        .await;
        match result {
            Ok(()) => {
                self.loaded.borrow_mut().extend(
                    plugin
                        .package
                        .manifest
                        .facets
                        .iter()
                        .filter(|facet| facet.target == FacetTarget::Server)
                        .map(|facet| facet.id.clone()),
                );
                plugin.observed = if plugin
                    .package
                    .manifest
                    .facets
                    .iter()
                    .any(|facet| facet.target == FacetTarget::Server)
                {
                    ObservedState::Active
                } else {
                    ObservedState::Installed
                };
                plugin.activation_id = uuid::Uuid::new_v4().to_string();
                plugin.error.clear();
            }
            Err(ref error) => {
                plugin.observed = ObservedState::Failed;
                plugin.error = error.to_string();
            }
        }
        if let Err(error) = self.storage()?.save(&plugin) {
            if result.is_ok()
                && plugin
                    .package
                    .manifest
                    .facets
                    .iter()
                    .any(|facet| facet.target == FacetTarget::Server)
            {
                return Err(self.publication_unrecorded(&plugin.package, plugin.desired, error));
            }
            return Err(error);
        }
        result
    }

    /// Activates exact cached bytes through Tembra's executor and installation lifecycle.
    ///
    /// # Errors
    /// Failed preparation records failure without claiming runtime publication. Errors after
    /// admission may have external effects; the durable desired state remains active.
    pub async fn activate(&self, request: ActivateRequest) -> Result<Inventory> {
        self.authorize(&request.proof)?;
        let _guard = self.guard()?;
        let plugin = self.dynamic(&request.plugin_id)?;
        if plugin.desired == DesiredState::Active && !plugin.activation_id.is_empty() {
            return self.snapshot();
        }
        let operation = self.begin(&request.plugin_id, AdminOperation::Activate)?;
        let result = self.activate_cached(plugin).await;
        self.finish(operation, result).await
    }

    /// Replaces a confirmed release through the actual coordinated Tembra reload operation.
    ///
    /// # Errors
    /// Failed acquisition or preparation retains the old selected package and records the
    /// observed failure. A runtime error does not assert rollback of plugin external effects.
    pub async fn update(&self, request: UpdateRequest) -> Result<Inventory> {
        self.authorize(&request.proof)?;
        if !request.confirmed {
            return Err(ManagerError::ConfirmationRequired);
        }
        let _guard = self.guard()?;
        let old = self.dynamic(&request.plugin_id)?;
        let operation = self.begin(&request.plugin_id, AdminOperation::Update)?;
        let mut committed = None;
        let result: Result<()> = async {
            let package = self
                .acquire(
                    &request.proof,
                    &request.provider_id,
                    &request.plugin_id,
                    &request.version,
                )
                .await?;
            if package.manifest.version == old.package.manifest.version && package != old.package {
                return Err(ManagerError::Integrity(
                    "an installed version cannot change its approved bytes".into(),
                ));
            }
            self.storage()?.retain_version(&package)?;
            if old.desired == DesiredState::Active {
                let facets = self.prepared(&package).await?;
                let next_names = facets
                    .iter()
                    .map(|facet| facet.name().to_string())
                    .collect::<BTreeSet<_>>();
                let remove = old
                    .package
                    .manifest
                    .facets
                    .iter()
                    .filter(|facet| {
                        facet.target == FacetTarget::Server && !next_names.contains(&facet.id)
                    })
                    .map(|facet| facet.id.clone())
                    .collect::<Vec<_>>();
                if !facets.is_empty() || !remove.is_empty() {
                    self.host
                        .core()
                        .reload(
                            facets,
                            core::ReloadOptions {
                                remove,
                                ..Default::default()
                            },
                        )
                        .await?;
                    committed = Some(package.clone());
                }
                let mut loaded = self.loaded.borrow_mut();
                for facet in &old.package.manifest.facets {
                    loaded.remove(&facet.id);
                }
                loaded.extend(next_names);
            }
            let observed = if old.desired == DesiredState::Active {
                if package
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
                old.observed
            };
            self.storage()?.save(&CachedPlugin {
                package,
                desired: old.desired,
                observed,
                error: String::new(),
                activation_id: if old.desired == DesiredState::Active {
                    uuid::Uuid::new_v4().to_string()
                } else {
                    String::new()
                },
            })
        }
        .await;
        let result = match (result, committed) {
            (Err(cause), Some(package)) => {
                Err(self.publication_unrecorded(&package, DesiredState::Active, cause))
            }
            (Err(error), None) => {
                let mut retained = old;
                retained.error = error.to_string();
                self.storage()?.save(&retained)?;
                Err(error)
            }
            (Ok(()), _) => Ok(()),
        };
        self.finish(operation, result).await
    }

    fn required_by(&self, id: &str, only_active: bool) -> Result<()> {
        for plugin in self.storage()?.all()? {
            if plugin.package.manifest.id != id
                && (!only_active || plugin.observed == ObservedState::Active)
                && plugin
                    .package
                    .manifest
                    .dependencies
                    .iter()
                    .any(|dependency| dependency.id == id)
            {
                return Err(ManagerError::Dependency(format!(
                    "{id} is required by {}",
                    plugin.package.manifest.id
                )));
            }
        }
        Ok(())
    }

    async fn deactivate_cached(&self, mut plugin: CachedPlugin) -> Result<()> {
        self.required_by(&plugin.package.manifest.id, true)?;
        plugin.desired = DesiredState::Installed;
        self.storage()?.save(&plugin)?;
        let remove = plugin
            .package
            .manifest
            .facets
            .iter()
            .filter(|facet| self.loaded.borrow().contains(&facet.id))
            .map(|facet| facet.id.clone())
            .collect::<Vec<_>>();
        let result = if remove.is_empty() {
            Ok(())
        } else {
            self.host
                .core()
                .reload(
                    vec![],
                    core::ReloadOptions {
                        remove,
                        ..Default::default()
                    },
                )
                .await
                .map(|_| ())
                .map_err(ManagerError::from)
        };
        if result.is_ok() {
            for facet in &plugin.package.manifest.facets {
                self.loaded.borrow_mut().remove(&facet.id);
            }
            plugin.observed = ObservedState::Inactive;
            plugin.error.clear();
        } else {
            plugin.error = result
                .as_ref()
                .err()
                .map(ToString::to_string)
                .unwrap_or_default();
        }
        if let Err(error) = self.storage()?.save(&plugin) {
            if result.is_ok() {
                return Err(self.publication_unrecorded(&plugin.package, plugin.desired, error));
            }
            return Err(error);
        }
        self.storage()?
            .browser_removal_requested(&plugin.package.manifest.id)?;
        result
    }

    /// Removes running foreign facets through Tembra reload while retaining cached bytes.
    ///
    /// # Errors
    /// Refuses Core Plugins and active dependents. Runtime failures retain their exact cause.
    pub async fn deactivate(&self, request: DeactivateRequest) -> Result<Inventory> {
        self.authorize(&request.proof)?;
        let _guard = self.guard()?;
        let plugin = self.dynamic(&request.plugin_id)?;
        let operation = self.begin(&request.plugin_id, AdminOperation::Deactivate)?;
        let result = self.deactivate_cached(plugin).await;
        self.finish(operation, result).await
    }

    /// Deactivates a dynamic plugin and removes its installed selection, retaining history.
    ///
    /// # Errors
    /// Refuses installed dependents and Core Plugins. Deactivation effects can precede a later
    /// SQLite failure; operation evidence reports the actual failed stage rather than rollback.
    pub async fn uninstall(&self, request: UninstallRequest) -> Result<Inventory> {
        self.authorize(&request.proof)?;
        let _guard = self.guard()?;
        let plugin = self.dynamic(&request.plugin_id)?;
        self.required_by(&request.plugin_id, false)?;
        let operation = self.begin(&request.plugin_id, AdminOperation::Uninstall)?;
        let result: Result<()> = async {
            self.deactivate_cached(plugin).await?;
            self.storage()?.remove(&request.plugin_id)
        }
        .await;
        self.finish(operation, result).await
    }

    /// Replays active intent from exact cached bytes on the fresh Host without source access.
    ///
    /// Interrupted records are retained as interrupted observations. Replay uses current Host
    /// facts and actual installation results; it never claims to reverse prior external effects.
    /// # Errors
    /// Failed packages remain failed and other independent packages are still attempted. Missing
    /// dependencies and cycles are reported rather than synthesized as available capabilities.
    pub async fn recover(&self, proof: &AdminProof) -> Result<Inventory> {
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
