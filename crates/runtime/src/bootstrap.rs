use std::{path::PathBuf, rc::Rc};

use swarmie_plugin_contracts::ConfigureProviderRequest;
use swarmie_plugin_manager::PluginManager;
use tembra::{Host, core};

use crate::{CompiledPlugin, Configuration, Runtime, application, configuration, deployment};

pub(crate) async fn start(
    driver: Rc<dyn core::Driver>,
    configuration: Configuration,
    compiled: Option<Vec<CompiledPlugin>>,
) -> Result<Runtime, Box<dyn std::error::Error>> {
    let host = Host::with_driver("swarmie-runtime", "runtime-supervisor", driver.clone())?;
    let result = async {
        let manager =
            PluginManager::start(host.clone(), driver.clone(), configuration.database.clone())
                .await?;
        let persisted = manager.composition()?;
        let application = match configuration.application {
            Some(application) => application,
            None => persisted
                .map(serde_json::from_value)
                .transpose()?
                .unwrap_or_else(|| application::default_for(&configuration.database)),
        };
        let configured = manager.provider_configuration("local")?;
        let persist_source = configured.is_none();
        let source = configured
            .map(serde_json::from_value::<PathBuf>)
            .transpose()?
            .unwrap_or_else(|| configuration.source.clone());
        let compiled = compiled.unwrap_or_else(|| application::compiled(source, driver.clone()));
        let mut prepared = crate::preparation::prepare(&application, &compiled)?;
        let permit = manager.begin_composition_reconciliation()?;
        let configuration_values =
            crate::configuration::ConfigurationValues::new(&prepared.effective);
        {
            let selected = prepared
                .effective
                .deployment_order
                .iter()
                .filter_map(|id| {
                    let release = prepared.effective.instances[id].release.as_ref()?;
                    (release.kind == swarmie_composition::PluginKind::Dynamic)
                        .then_some(release.plugin.clone())
                })
                .collect();
            manager.set_composition_selection(&selected)?;
            manager.save_composition(&serde_json::to_value(&application)?)?;
            host.install(configuration_values.facet()?, core::LoadOptions::default())?
                .await?;
            manager
                .record_core_facets(
                    configuration::PLUGIN_ID.into(),
                    env!("CARGO_PKG_VERSION").into(),
                    vec![configuration::FACET_ID.into()],
                )
                .await?;
        }
        if persist_source {
            manager
                .configure_provider(ConfigureProviderRequest {
                    provider_id: "local".into(),
                    configuration: serde_json::to_value(&configuration.source)?,
                })
                .await?;
        }
        let deployment::Deployment {
            recovery_error,
            errors: deployment_errors,
            attempted,
        } = deployment::apply(&manager, &mut prepared, true).await?;
        drop(permit);
        let applied = prepared
            .effective
            .instances
            .iter()
            .filter(|(id, instance)| {
                instance.state == swarmie_composition::SelectionState::Enabled
                    && !deployment_errors.contains_key(*id)
            })
            .map(|(id, instance)| (id.clone(), instance.clone()))
            .collect();
        let installed = host.core().installed_facets();
        for id in deployment_errors.keys() {
            // Keep initialization input for confirmed survivors or uncertain cleanup.
            // Its availability never acknowledges the failed application selection.
            let remains_installed = prepared.effective.instances[id]
                .release
                .iter()
                .flat_map(|release| &release.facets)
                .any(|facet| installed.iter().any(|actual| actual.name() == facet.id));
            if !remains_installed && (!attempted.contains(id) || !manager.requires_recovery()) {
                configuration_values.remove(id);
            }
        }
        Ok::<_, Box<dyn std::error::Error>>(Runtime {
            host: host.clone(),
            manager,
            effective: prepared.effective,
            applied,
            desired: application,
            compiled,
            configuration_values,
            reconciliation_uncertain: Rc::new(std::cell::Cell::new(false)),
            deployment_errors,
            recovery_error,
        })
    }
    .await;
    if result.is_err() {
        host.close().await?;
    }
    result
}
