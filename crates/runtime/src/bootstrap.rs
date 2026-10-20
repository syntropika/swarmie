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
        let manager = PluginManager::start(host.clone(), driver, configuration.database).await?;
        let persisted = manager.composition()?;
        let managed = configuration.application.is_some() || persisted.is_some();
        let application = match configuration.application {
            Some(application) => application,
            None => persisted
                .map(serde_json::from_value)
                .transpose()?
                .unwrap_or_default(),
        };
        let configured = manager.provider_configuration("local")?;
        let persist_source = configured.is_none();
        let source = configured
            .map(serde_json::from_value::<PathBuf>)
            .transpose()?
            .unwrap_or_else(|| configuration.source.clone());
        let compiled = compiled.unwrap_or_else(|| application::compiled(source));
        let mut prepared = crate::preparation::prepare(&application, &compiled)?;
        let permit = manager.begin_composition_reconciliation()?;
        if managed {
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
            host.install(
                configuration::facet(&prepared.effective)?,
                core::LoadOptions::default(),
            )?
            .await?;
            manager
                .record_core_facets(
                    configuration::PLUGIN_ID.into(),
                    env!("CARGO_PKG_VERSION").into(),
                    vec![configuration::FACET_ID.into()],
                )
                .await?;
        }
        let (recovery_error, deployment_errors) =
            deployment::apply(&host, &manager, &mut prepared, managed).await?;
        if persist_source {
            manager
                .configure_provider(ConfigureProviderRequest {
                    provider_id: "local".into(),
                    configuration: serde_json::to_value(&configuration.source)?,
                })
                .await?;
        }
        drop(permit);
        Ok::<_, Box<dyn std::error::Error>>(Runtime {
            host: host.clone(),
            manager,
            effective: prepared.effective,
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
