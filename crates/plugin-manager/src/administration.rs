use std::collections::BTreeMap;

use serde::{Serialize, de::DeserializeOwned};
use swarmie_plugin_contracts::*;
use tembra::ContractToken;

use crate::{ManagerError, PluginManager, Result, validation::validate_package};

impl PluginManager {
    /// Reads sanitized administrative inventory for trusted local callers.
    ///
    /// # Errors
    /// Returns SQLite, registry or decoding errors without mutation.
    pub fn inventory(&self) -> Result<Inventory> {
        self.snapshot()
    }

    pub(crate) fn snapshot(&self) -> Result<Inventory> {
        let mut plugins = self.cores.borrow().clone();
        for plugin in self.storage()?.all()? {
            if plugins.iter().any(|core| {
                core.kind == PluginKind::Core && core.plugin_id == plugin.package.manifest.id
            }) {
                continue;
            }
            plugins.push(PluginRecord {
                plugin_id: plugin.package.manifest.id,
                version: plugin.package.manifest.version,
                provider_id: plugin.package.provenance.provider_id,
                kind: PluginKind::Dynamic,
                desired: plugin.desired,
                observed: plugin.observed,
                errors: if plugin.error.is_empty() {
                    vec![]
                } else {
                    vec![plugin.error]
                },
            });
        }
        for observation in self.uncertain.borrow().values() {
            if let Some(plugin) = plugins
                .iter_mut()
                .find(|plugin| plugin.plugin_id == observation.plugin_id)
            {
                *plugin = observation.clone();
            }
        }
        for plugin in &mut plugins {
            if plugin.desired == DesiredState::Active
                && plugin.observed == ObservedState::Active
                && !self.uncertain.borrow().contains_key(&plugin.plugin_id)
                && let Some((observed, errors)) = self.reference_observation(&plugin.plugin_id)
            {
                plugin.observed = observed;
                plugin.errors.extend(errors);
            }
        }
        let mut providers = BTreeMap::<String, ProviderRecord>::new();
        for (id, _) in self.storage()?.configurations()? {
            providers.insert(
                id.clone(),
                ProviderRecord {
                    provider_id: id,
                    configuration: serde_json::Value::Null,
                    operations: vec![],
                },
            );
        }
        let registry = self.host.core().registry(&source_registry()?)?;
        for (_, entry) in registry.table(None)?.entries() {
            if let Ok(metadata) =
                serde_json::from_value::<SourceRegistryMetadata>(entry.data().clone())
            {
                if entry.operation().is_none() {
                    continue;
                }
                let provider =
                    providers
                        .entry(metadata.provider_id.clone())
                        .or_insert(ProviderRecord {
                            provider_id: metadata.provider_id,
                            configuration: serde_json::Value::Null,
                            operations: vec![],
                        });
                if !provider.operations.contains(&metadata.operation) {
                    provider.operations.push(metadata.operation);
                }
            }
        }
        Ok(Inventory {
            plugins,
            providers: providers.into_values().collect(),
            operations: self.storage()?.operations()?,
            browser_sessions: self.storage()?.browser_sessions()?,
        })
    }

    /// Reads durable provider settings without exposing them through observable inventory.
    ///
    /// # Errors
    /// Propagates SQLite failures.
    pub fn provider_configuration(&self, id: &str) -> Result<Option<serde_json::Value>> {
        Ok(self
            .storage()?
            .configurations()?
            .into_iter()
            .find(|(key, _)| key == id)
            .map(|(_, value)| value))
    }

    /// Persists provider-owned settings for the supervisor's next provider construction.
    ///
    /// Settings are deliberately not interpreted by the generic Manager. A running provider
    /// applies its own configuration lifecycle; persistence alone does not change its object.
    /// # Errors
    /// Reports SQLite/publication failures.
    pub async fn configure_provider(&self, request: ConfigureProviderRequest) -> Result<Inventory> {
        let _guard = self.guard()?;
        let operation = self.begin(&request.provider_id, AdminOperation::ConfigureProvider)?;
        let result = self
            .storage()?
            .configure(&request.provider_id, &request.configuration);
        self.finish(operation, result).await
    }

    /// Invokes a source operation discovered from its live Tembra Registry entry.
    ///
    /// # Errors
    /// Refuses absent/ambiguous operations, schema mismatch and actual
    /// provider errors. The Manager never selects a vendor or installs callbacks of its own.
    pub async fn source_request<A: Serialize, R: DeserializeOwned>(
        &self,
        provider: &str,
        operation: SourceOperation,
        request: A,
    ) -> Result<R> {
        let registry = self.host.core().registry(&source_registry()?)?;
        let table = registry.table(None)?;
        let mut found = None;
        for (_, entry) in table.entries() {
            if let Ok(metadata) =
                serde_json::from_value::<SourceRegistryMetadata>(entry.data().clone())
                && metadata.provider_id == provider
                && metadata.operation == operation
            {
                if found.is_some() {
                    return Err(ManagerError::InvalidPackage(
                        "ambiguous source operation".into(),
                    ));
                }
                found = entry.operation().cloned();
            }
        }
        let method = found.ok_or_else(|| ManagerError::SourceUnavailable {
            provider: provider.into(),
            operation: format!("{operation:?}").to_lowercase(),
        })?;
        let expected_name = match operation {
            SourceOperation::Search => "search",
            SourceOperation::Resolve => "resolve",
            SourceOperation::Inspect => "inspect",
            SourceOperation::Acquire => "acquire",
        };
        let canonical = PluginSourceSchema.descriptor()?;
        let expected = canonical
            .members("")
            .and_then(|members| members.get(expected_name));
        let actual = method
            .contract()
            .members("")
            .and_then(|members| members.get(method.method()));
        if actual != expected || expected.is_none() {
            return Err(ManagerError::SourceSchema {
                provider: provider.into(),
                operation: expected_name.into(),
            });
        }
        let payload = serde_json::to_value(request)?;
        let result = self
            .host
            .root(
                move |context| async move { method.call_json(context.core(), Some(payload)).await },
            )
            .await?;
        Ok(serde_json::from_value(result.ok_or_else(|| {
            ManagerError::InvalidPackage("source returned no payload".into())
        })?)?)
    }

    pub(crate) fn begin(&self, id: &str, operation: AdminOperation) -> Result<OperationRecord> {
        let record = OperationRecord {
            id: uuid::Uuid::new_v4().to_string(),
            plugin_id: id.into(),
            operation,
            status: OperationStatus::Running,
            error: String::new(),
        };
        self.storage()?.operation(&record)?;
        Ok(record)
    }

    pub(crate) async fn finish(
        &self,
        mut operation: OperationRecord,
        result: Result<()>,
    ) -> Result<Inventory> {
        operation.status = if result.is_ok() {
            OperationStatus::Succeeded
        } else {
            OperationStatus::Failed
        };
        if let Err(error) = &result {
            operation.error = error.to_string();
        }
        if let Err(recording) = self.storage()?.operation(&operation) {
            return Err(match result {
                Err(failure) => self.failure_unrecorded(failure, recording),
                Ok(()) => recording,
            });
        }
        result?;
        self.snapshot()
    }

    /// Validates immutable package version requirements, independently of object readiness.
    /// Tembra owns capability acquisition and availability; installed package metadata does
    /// not establish that a service is active.
    pub(crate) fn package_dependencies(&self, package: &AcquiredPackage) -> Result<()> {
        for dependency in &package.manifest.dependencies {
            let compiled = self
                .cores
                .borrow()
                .iter()
                .find(|core| core.plugin_id == dependency.id)
                .map(|core| core.version.clone());
            let version = if let Some(version) = compiled {
                version
            } else {
                if self
                    .composition_selection
                    .borrow()
                    .as_ref()
                    .is_some_and(|selected| !selected.contains(&dependency.id))
                {
                    return Err(ManagerError::Dependency(format!(
                        "{} is outside the selected composition",
                        dependency.id
                    )));
                }
                self.storage()?
                    .load(&dependency.id)?
                    .ok_or_else(|| ManagerError::Dependency(dependency.id.clone()))?
                    .package
                    .manifest
                    .version
            };
            let requirement = semver::VersionReq::parse(&dependency.version)
                .map_err(|error| ManagerError::Dependency(error.to_string()))?;
            let version = semver::Version::parse(&version)
                .map_err(|error| ManagerError::Dependency(error.to_string()))?;
            if !requirement.matches(&version) {
                return Err(ManagerError::Dependency(dependency.id.clone()));
            }
        }
        Ok(())
    }

    pub(crate) fn admission(&self, package: &AcquiredPackage) -> Result<()> {
        let manifest = &package.manifest;
        if self
            .cores
            .borrow()
            .iter()
            .any(|core| manifest.id == core.plugin_id)
        {
            return Err(ManagerError::CorePlugin(manifest.id.clone()));
        }
        for (owner, owned) in self.core_facets.borrow().iter() {
            if manifest
                .facets
                .iter()
                .any(|facet| owned.contains(&facet.id))
            {
                return Err(ManagerError::CorePlugin(owner.clone()));
            }
        }
        let version = semver::Version::parse(&manifest.version)
            .map_err(|error| ManagerError::InvalidPackage(error.to_string()))?;
        for plugin in self.storage()?.all()? {
            if plugin.package.manifest.id == manifest.id
                || !self.composition_candidate_relevant(&plugin.package.manifest.id)
            {
                continue;
            }
            if manifest.facets.iter().any(|facet| {
                plugin
                    .package
                    .manifest
                    .facets
                    .iter()
                    .any(|existing| existing.id == facet.id)
            }) {
                return Err(ManagerError::InvalidPackage(
                    "facet identity is already owned by another installed plugin".into(),
                ));
            }
            for dependency in &plugin.package.manifest.dependencies {
                if dependency.id == manifest.id {
                    let requirement = semver::VersionReq::parse(&dependency.version)
                        .map_err(|error| ManagerError::Dependency(error.to_string()))?;
                    if !requirement.matches(&version) {
                        return Err(ManagerError::Dependency(format!(
                            "candidate violates {} requirement",
                            plugin.package.manifest.id
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) async fn acquire(
        &self,
        provider: &str,
        id: &str,
        version: &str,
    ) -> Result<AcquiredPackage> {
        let package = self.acquire_verified(provider, id, version).await?;
        if self.composition_selection.borrow().is_none() {
            self.package_dependencies(&package)?;
        }
        self.admission(&package)?;
        Ok(package)
    }

    /// Verifies provider bytes without changing or comparing the installed selection.
    pub(crate) async fn acquire_verified(
        &self,
        provider: &str,
        id: &str,
        version: &str,
    ) -> Result<AcquiredPackage> {
        if !self.trusted_sources.borrow().contains(provider) {
            return Err(ManagerError::UntrustedSource(provider.into()));
        }
        let exact = semver::Version::parse(version).ok();
        let requested = semver::VersionReq::parse(version)
            .map_err(|error| ManagerError::InvalidPackage(error.to_string()))?;
        let resolved: ResolvedPlugin = self
            .source_request(
                provider,
                SourceOperation::Resolve,
                ResolveRequest {
                    plugin_id: id.into(),
                    version: version.into(),
                },
            )
            .await?;
        if resolved.manifest.id != id {
            return Err(ManagerError::Integrity(
                "resolved plugin identity differs".into(),
            ));
        }
        let selected = semver::Version::parse(&resolved.manifest.version)
            .map_err(|error| ManagerError::InvalidPackage(error.to_string()))?;
        if !exact
            .as_ref()
            .map_or_else(|| requested.matches(&selected), |exact| exact == &selected)
        {
            return Err(ManagerError::Integrity(
                "resolved version does not satisfy the confirmed request".into(),
            ));
        }
        let inspected: PluginManifest = self
            .source_request(
                provider,
                SourceOperation::Inspect,
                InspectRequest {
                    plugin_id: id.into(),
                    version: resolved.manifest.version.clone(),
                },
            )
            .await?;
        let package: AcquiredPackage = self
            .source_request(
                provider,
                SourceOperation::Acquire,
                AcquireRequest {
                    plugin_id: id.into(),
                    version: resolved.manifest.version.clone(),
                },
            )
            .await?;
        if inspected != resolved.manifest
            || package.manifest != resolved.manifest
            || package.provenance.provider_id != provider
        {
            return Err(ManagerError::Integrity(
                "source changed between resolution, inspection and acquisition".into(),
            ));
        }
        validate_package(&package)?;
        Ok(package)
    }
}
