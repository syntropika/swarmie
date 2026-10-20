use std::collections::{BTreeMap, BTreeSet};

use swarmie_composition::{PluginKind, Release};
use swarmie_plugin_contracts::{DesiredState, InstallRequest, PluginManifest};
use swarmie_plugin_manager::{CorePluginDeployment, ManagerError, PluginManager};

use crate::preparation::Prepared;

type Error = Box<dyn std::error::Error>;

pub(crate) struct Deployment {
    pub recovery_error: Option<ManagerError>,
    pub errors: BTreeMap<String, String>,
    /// Instances submitted to an administrative admission call, not confirmed Facet presence.
    pub attempted: BTreeSet<String>,
}

pub(crate) fn check_manifest(release: &Release, manifest: &PluginManifest) -> Result<(), Error> {
    let mut actual = manifest
        .facets
        .iter()
        .map(|facet| (facet.id.clone(), format!("{:?}", facet.target)))
        .collect::<Vec<_>>();
    let mut expected = release
        .facets
        .iter()
        .map(|facet| (facet.id.clone(), format!("{:?}", facet.target)))
        .collect::<Vec<_>>();
    actual.sort();
    expected.sort();
    if manifest.id != release.plugin || manifest.version != release.version || actual != expected {
        return Err(format!(
            "approved package differs from selected release: {}",
            release.plugin
        )
        .into());
    }
    Ok(())
}

pub(crate) async fn apply(
    manager: &PluginManager,
    prepared: &mut Prepared,
    managed: bool,
) -> Result<Deployment, Error> {
    let mut errors = BTreeMap::new();
    let mut attempted = BTreeSet::new();
    let providers = prepared
        .effective
        .deployment_order
        .iter()
        .filter_map(|id| prepared.effective.instances[id].selection.provider.clone())
        .collect::<BTreeSet<_>>();
    // Acquisition must be available before its artifacts can be prepared. Other application
    // Core Facets remain in the mixed declaration batch, including Core-to-Dynamic consumers.
    let sources = prepared
        .trusted_sources
        .iter()
        .filter(|(_, sources)| sources.iter().any(|source| providers.contains(source)))
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    let mut source_plugins = Vec::new();
    for id in &sources {
        let release = prepared.effective.instances[id]
            .release
            .as_ref()
            .ok_or("source release absent")?;
        source_plugins.push(CorePluginDeployment {
            plugin_id: release.plugin.clone(),
            version: release.version.clone(),
            facets: prepared
                .native
                .remove(id)
                .ok_or("source adapter Facets are absent")?,
        });
    }
    if !source_plugins.is_empty() {
        attempted.extend(sources.iter().cloned());
        if let Err(error) = manager.install_core_batch(source_plugins).await {
            for id in &prepared.effective.deployment_order {
                errors.insert(id.clone(), error.to_string());
            }
            return Ok(Deployment {
                recovery_error: Some(error),
                errors,
                attempted,
            });
        }
        for id in &sources {
            for provider in &prepared.trusted_sources[id] {
                manager.approve_trusted_source(provider.clone())?;
            }
        }
    }
    let mut selected = Vec::new();
    for id in &prepared.effective.deployment_order {
        let instance = &prepared.effective.instances[id];
        let release = instance.release.as_ref().ok_or("selected release absent")?;
        if release.kind != PluginKind::Dynamic {
            continue;
        }
        let staged: Result<(), Error> = async {
            let manifest = manager
                .stage_release(InstallRequest {
                    provider_id: instance
                        .selection
                        .provider
                        .clone()
                        .ok_or("source identity absent")?,
                    plugin_id: release.plugin.clone(),
                    version: release.version.clone(),
                    confirmed: true,
                })
                .await?;
            check_manifest(release, &manifest)
        }
        .await;
        match staged {
            Ok(()) => selected.push(release.plugin.clone()),
            Err(error) => {
                errors.insert(id.clone(), error.to_string());
            }
        }
    }
    if !managed {
        selected = manager
            .inventory()?
            .plugins
            .into_iter()
            .filter(|plugin| {
                plugin.kind == swarmie_plugin_contracts::PluginKind::Dynamic
                    && plugin.desired == DesiredState::Active
            })
            .map(|plugin| plugin.plugin_id)
            .collect();
    }
    let mut cores = Vec::new();
    let mut batch_instances = Vec::new();
    for id in &prepared.effective.deployment_order {
        if let Some(facets) = prepared.native.remove(id) {
            batch_instances.push(id.clone());
            let release = prepared.effective.instances[id]
                .release
                .as_ref()
                .ok_or("compiled release absent")?;
            cores.push(CorePluginDeployment {
                plugin_id: release.plugin.clone(),
                version: release.version.clone(),
                facets,
            });
        }
    }
    for id in &prepared.effective.deployment_order {
        let release = prepared.effective.instances[id]
            .release
            .as_ref()
            .ok_or("selected release absent")?;
        if release.kind == PluginKind::Dynamic && selected.contains(&release.plugin) {
            batch_instances.push(id.clone());
        }
    }
    attempted.extend(batch_instances.iter().cloned());
    let recovery_error = manager.recover_composed(cores, &selected).await.err();
    if let Some(error) = &recovery_error {
        // A mixed batch can fail after earlier declarations have been installed. Preserve the
        // actual aggregate failure rather than inventing per-object readiness or rollback.
        for id in &batch_instances {
            errors
                .entry(id.clone())
                .or_insert_with(|| error.to_string());
        }
    } else {
        for id in &batch_instances {
            for source in prepared.trusted_sources.get(id).into_iter().flatten() {
                manager.approve_trusted_source(source.clone())?;
            }
        }
    }
    Ok(Deployment {
        recovery_error,
        errors,
        attempted,
    })
}
