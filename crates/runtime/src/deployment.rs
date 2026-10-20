use std::{collections::BTreeMap, rc::Rc};

use serde_json::json;
use swarmie_composition::{PluginKind, Release};
use swarmie_plugin_contracts::{AdminProof, InstallRequest, PluginManifest};
use swarmie_plugin_manager::PluginManager;
use tembra::{Host, contracts::Contract, core};

use crate::preparation::Prepared;

type Error = Box<dyn std::error::Error>;

fn check_manifest(release: &Release, manifest: &PluginManifest) -> Result<(), Error> {
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
            "approved package differs from composition release facts: {}",
            release.plugin
        )
        .into());
    }
    Ok(())
}

pub(crate) async fn apply(
    host: &Host,
    manager: &PluginManager,
    proof: &AdminProof,
    prepared: &mut Prepared,
    contracts: &[Rc<Contract>],
    managed: bool,
) -> Result<
    (
        Option<swarmie_plugin_manager::ManagerError>,
        BTreeMap<String, String>,
    ),
    Error,
> {
    for id in &prepared.effective.activation_order {
        if let Some(facets) = prepared.native.remove(id) {
            let release = prepared.effective.instances[id]
                .release
                .as_ref()
                .ok_or("compiled release absent")?;
            host.core()
                .load(
                    facets.into_iter().map(Into::into).collect(),
                    core::LoadOptions::default(),
                )?
                .await?;
            manager
                .record_core_facets(
                    proof,
                    release.plugin.clone(),
                    release.version.clone(),
                    release
                        .facets
                        .iter()
                        .map(|facet| facet.id.clone())
                        .collect(),
                )
                .await?;
            for provider in prepared.trusted_sources.get(id).into_iter().flatten() {
                manager.approve_trusted_source(proof, provider.clone())?;
            }
        }
    }
    let mut errors = BTreeMap::new();
    let mut selected = Vec::new();
    for id in &prepared.effective.activation_order {
        let instance = &prepared.effective.instances[id];
        let release = instance.release.as_ref().ok_or("eligible release absent")?;
        if release.kind != PluginKind::Dynamic {
            continue;
        }
        let staged: Result<(), Error> = async {
            let manifest = manager
                .stage_release(InstallRequest {
                    proof: proof.clone(),
                    provider_id: instance
                        .selection
                        .provider
                        .clone()
                        .ok_or("source provider is absent")?,
                    plugin_id: release.plugin.clone(),
                    version: release.version.clone(),
                    confirmed: true,
                })
                .await?;
            check_manifest(release, &manifest)
        }
        .await;
        match staged {
            Ok(()) => {
                selected.push(release.plugin.clone());
            }
            Err(error) => {
                errors.insert(id.clone(), error.to_string());
            }
        }
    }
    let recovery = if managed {
        manager.recover_selected_ordered(proof, &selected).await
    } else {
        manager.recover(proof).await
    };
    let recovery_error = match recovery {
        Ok(_) => None,
        Err(
            error @ (swarmie_plugin_manager::ManagerError::Runtime(_)
            | swarmie_plugin_manager::ManagerError::InvalidPackage(_)
            | swarmie_plugin_manager::ManagerError::Integrity(_)
            | swarmie_plugin_manager::ManagerError::Compatibility(_)
            | swarmie_plugin_manager::ManagerError::Dependency(_)),
        ) => Some(error),
        Err(error) => return Err(error.into()),
    };
    for id in &prepared.effective.activation_order {
        let instance = &prepared.effective.instances[id];
        let release = instance.release.as_ref().ok_or("eligible release absent")?;
        let result: Result<(), Error> = async {
            if let Some(error) = errors.get(id) {
                return Err(error.clone().into());
            }
            for capability in &release.provides {
                let contract = contracts
                    .iter()
                    .find(|contract| {
                        contract.name() == capability.contract
                            && contract.version() == u64::from(capability.version)
                    })
                    .ok_or("qualified Contract is absent")?;
                let reference =
                    host.core()
                        .hold(contract.clone(), json!({}), core::GetOptions::default())?;
                manager.observe_reference(proof, &release.plugin, reference.clone())?;
                reference.ready().await?;
                let registry = core::Registry::new(
                    swarmie_plugin_contracts::CAPABILITY_REGISTRY,
                    core::ConflictPolicy::Error,
                )?;
                let table = host.core().registry(&registry)?.table(None)?;
                if !table.entries().any(|(_, entry)| {
                    entry.owner_id() == reference.object_id()
                        && entry
                            .data()
                            .get("plugin_id")
                            .and_then(serde_json::Value::as_str)
                            == Some(release.plugin.as_str())
                        && entry
                            .data()
                            .get("contract")
                            .and_then(serde_json::Value::as_str)
                            == Some(contract.name())
                        && entry.operation().is_some_and(|operation| {
                            operation.contract().fingerprint() == contract.fingerprint()
                        })
                }) {
                    return Err(format!(
                        "capability ownership is not established for {}: {}",
                        release.plugin,
                        contract.name()
                    )
                    .into());
                }
            }
            Ok(())
        }
        .await;
        if let Err(error) = result {
            if manager
                .inventory(proof)?
                .plugins
                .iter()
                .any(|plugin| plugin.plugin_id == release.plugin)
            {
                manager.observe_failure(proof, &release.plugin, error.to_string())?;
            }
            errors.insert(id.clone(), error.to_string());
        }
    }
    Ok((recovery_error, errors))
}
