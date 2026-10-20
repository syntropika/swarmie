use std::collections::BTreeSet;

use swarmie_plugin_contracts::*;
use tembra::core;

use crate::{ManagerError, PluginManager, Result, store::CachedPlugin};

impl PluginManager {
    /// Replaces one admitted Dynamic package with a distinct package in one Tembra reload.
    ///
    /// Candidate definitions and obsolete server Facet removal are submitted together, so
    /// Tembra can preserve compatible objects, State and retained References. Exact approved
    /// bytes may be cached first; both package acknowledgements and operation outcomes settle
    /// in one SQLite transaction only after runtime adoption. Browser retirement stays an
    /// explicit request, not an observation of physical removal.
    ///
    /// # Errors
    /// Refuses unconfirmed/nonexact input, Core packages, an inactive predecessor, an already
    /// active replacement and integrity or compatibility errors. A pre-adoption refusal keeps
    /// prior selection. Cancellation, failed restoration or publication failure retain both
    /// participants as uncertain and require recovery; no external-effect rollback is implied.
    pub async fn replace_dynamic(
        &self,
        previous_id: &str,
        request: InstallRequest,
    ) -> Result<Inventory> {
        self.require_composition_reconciliation()?;
        if !request.confirmed {
            return Err(ManagerError::ConfirmationRequired);
        }
        if previous_id == request.plugin_id {
            return Err(ManagerError::InvalidPackage(
                "same-package replacement uses update".into(),
            ));
        }
        semver::Version::parse(&request.version)
            .map_err(|error| ManagerError::InvalidPackage(error.to_string()))?;
        let _guard = self.guard()?;
        if self
            .cores
            .borrow()
            .iter()
            .any(|plugin| plugin.plugin_id == request.plugin_id)
        {
            return Err(ManagerError::CorePlugin(request.plugin_id));
        }
        let old = self.dynamic(previous_id)?;
        if old.desired != DesiredState::Active || old.activation_id.is_empty() {
            return Err(ManagerError::InvalidPackage(
                "replacement requires an admitted active predecessor".into(),
            ));
        }
        let installed = self
            .host
            .core()
            .installed_facets()
            .into_iter()
            .map(|facet| facet.name().to_owned())
            .collect::<BTreeSet<_>>();
        if self
            .storage()?
            .load(&request.plugin_id)?
            .is_some_and(|plugin| {
                plugin.desired == DesiredState::Active
                    && !plugin.activation_id.is_empty()
                    && plugin
                        .package
                        .manifest
                        .facets
                        .iter()
                        .any(|facet| match facet.target {
                            FacetTarget::Server => installed.contains(&facet.id),
                            FacetTarget::Browser => true,
                        })
            })
        {
            return Err(ManagerError::InvalidPackage(
                "replacement package already has installed server Facets or admitted Browser intent".into(),
            ));
        }
        let package = self
            .approved_release(&request.provider_id, &request.plugin_id, &request.version)
            .await?;
        self.package_dependencies(&package)?;
        self.admission_replacing(&package, Some(previous_id))?;
        let definitions = self.prepared(&package).await?;
        self.storage()?.retain_version(&package)?;
        let names = definitions
            .iter()
            .map(|facet| facet.name().to_owned())
            .collect::<BTreeSet<_>>();
        let remove = old
            .package
            .manifest
            .facets
            .iter()
            .filter(|facet| facet.target == FacetTarget::Server && !names.contains(&facet.id))
            .map(|facet| facet.id.clone())
            .collect();
        let mut operations = [
            self.begin(previous_id, AdminOperation::Deactivate)?,
            self.begin(&request.plugin_id, AdminOperation::Activate)?,
        ];
        self.fresh_session.set(false);
        let mut retired = old.clone();
        retired.desired = DesiredState::Installed;
        retired.observed = ObservedState::Inactive;
        retired.activation_id.clear();
        retired.error.clear();
        let mut next = CachedPlugin {
            package,
            desired: DesiredState::Active,
            observed: ObservedState::Active,
            error: String::new(),
            activation_id: uuid::Uuid::new_v4().to_string(),
        };
        if definitions.is_empty() {
            next.observed = ObservedState::Installed;
        }
        self.replacement_uncertain(
            &retired,
            &next,
            "replacement is in flight; runtime and durable outcomes are unconfirmed",
        );
        let reports = self.host.core().errors().len();
        let result = self
            .host
            .core()
            .reload(
                definitions,
                core::ReloadOptions {
                    remove,
                    ..Default::default()
                },
            )
            .await;
        let diagnostic = match result {
            Ok(report) => {
                if report.lingering_physical() == 0 {
                    String::new()
                } else {
                    format!(
                        "logical replacement committed; {} physical executions remain; termination unconfirmed",
                        report.lingering_physical()
                    )
                }
            }
            Err(cause) => {
                let cleanup_reports = self
                    .host
                    .core()
                    .errors()
                    .into_iter()
                    .skip(reports)
                    .collect::<Vec<_>>();
                let failure = if cleanup_reports.is_empty() {
                    ManagerError::Runtime(cause)
                } else {
                    ManagerError::ReloadFailed {
                        cause: Box::new(cause),
                        cleanup_reports,
                    }
                };
                for operation in &mut operations {
                    operation.status = OperationStatus::Failed;
                    operation.error = failure.to_string();
                }
                if let Err(recording) = self
                    .storage()
                    .and_then(|store| store.settle(&[], &operations, &[]))
                {
                    let error = self.failure_unrecorded(failure, recording);
                    self.replacement_uncertain(&retired, &next, &error.to_string());
                    return Err(error);
                }
                if failure.requires_restart() {
                    self.replacement_uncertain(&retired, &next, &failure.to_string());
                } else {
                    self.clear_replacement_uncertainty(previous_id, &request.plugin_id);
                }
                return Err(failure);
            }
        };
        retired.error = diagnostic.clone();
        next.error = diagnostic.clone();
        for operation in &mut operations {
            operation.status = OperationStatus::Succeeded;
            operation.error = diagnostic.clone();
        }
        if let Err(cause) = self.storage().and_then(|store| {
            store.settle(
                &[retired.clone(), next.clone()],
                &operations,
                &[previous_id.into()],
            )
        }) {
            let error = ManagerError::PublicationUnrecorded {
                plugin_id: next.package.manifest.id.clone(),
                version: next.package.manifest.version.clone(),
                cause: Box::new(cause),
            };
            self.replacement_uncertain(
                &retired,
                &next,
                &format!(
                    "replacement {previous_id} -> {} committed; {error}",
                    request.plugin_id
                ),
            );
            return Err(error);
        }
        self.clear_replacement_uncertainty(previous_id, &request.plugin_id);
        for id in [previous_id, &request.plugin_id] {
            self.references.borrow_mut().remove(id);
            self.reference_failures.borrow_mut().remove(id);
        }
        self.snapshot().map_err(|cause| {
            let error = ManagerError::CommittedObservationFailed {
                cause: Box::new(cause),
            };
            self.replacement_uncertain(&retired, &next, &error.to_string());
            error
        })
    }

    fn replacement_uncertain(&self, retired: &CachedPlugin, next: &CachedPlugin, diagnostic: &str) {
        for plugin in [retired, next] {
            self.uncertain.borrow_mut().insert(
                plugin.package.manifest.id.clone(),
                PluginRecord {
                    plugin_id: plugin.package.manifest.id.clone(),
                    version: plugin.package.manifest.version.clone(),
                    provider_id: plugin.package.provenance.provider_id.clone(),
                    kind: PluginKind::Dynamic,
                    desired: plugin.desired,
                    observed: ObservedState::Unavailable,
                    errors: vec![diagnostic.into()],
                },
            );
        }
    }

    fn clear_replacement_uncertainty(&self, previous: &str, next: &str) {
        self.uncertain.borrow_mut().remove(previous);
        self.uncertain.borrow_mut().remove(next);
    }
}
