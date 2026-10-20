//! Explicit synthetic application and compiled adapters used only by acceptance tests.

use std::{path::PathBuf, rc::Rc};

use serde::Deserialize;
use serde_json::json;
use swarmie_composition::{FacetIdentity, FacetTarget, PluginKind, Release, Selection};
use swarmie_plugin_manager::LocalSource;
use swarmie_runtime::{Application, CompiledPlugin, Configuration, Runtime};
use tembra::core;

/// Selects the Counter fixture instead of the production storage plugin.
pub fn application() -> Application {
    let mut application = Application::default();
    application.composition.profile.id = "counter-acceptance".into();
    application
        .composition
        .bundles
        .get_mut("swarmie.foundation")
        .expect("foundation bundle")
        .plugins = [swarmie_counter::PLUGIN_ID, "swarmie.local-plugin-source"]
        .map(|plugin| Selection {
            id: plugin.into(),
            plugin: plugin.into(),
            version: env!("CARGO_PKG_VERSION").into(),
            provider: None,
            config: json!({}),
        })
        .into();
    application
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct CounterConfiguration {
    #[serde(default)]
    initial: u32,
}

fn release(plugin: &str, facet: &str) -> Release {
    Release {
        plugin: plugin.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        kind: PluginKind::Core,
        facets: vec![FacetIdentity {
            id: facet.into(),
            target: FacetTarget::Server,
        }],
    }
}

/// Provides fixture-owned Counter and the genuine local package source.
pub fn catalog(source: PathBuf) -> Vec<CompiledPlugin> {
    vec![
        CompiledPlugin::new(
            release(swarmie_counter::PLUGIN_ID, swarmie_counter::FACET_ID),
            |value| {
                let configuration: CounterConfiguration = serde_json::from_value(value.clone())
                    .map_err(|error| {
                        tembra::RuntimeError::invalid("counter_configuration", error.to_string())
                    })?;
                Ok(vec![swarmie_counter::facet_with_initial(
                    configuration.initial,
                )?])
            },
        ),
        CompiledPlugin::new(
            release("swarmie.local-plugin-source", "swarmie.local-plugin-source"),
            move |value| {
                if value != &json!({}) {
                    return Err(tembra::RuntimeError::invalid(
                        "source_configuration",
                        "source location belongs to supervisor settings",
                    ));
                }
                Ok(vec![LocalSource::new("local", source.clone()).facet()?])
            },
        )
        .with_trusted_source("local".into()),
    ]
}

/// Starts an explicit fixture Profile, or restores its persisted application on a fresh Host.
pub async fn start(
    driver: Rc<dyn core::Driver>,
    configuration: Configuration,
) -> Result<Runtime, Box<dyn std::error::Error>> {
    let compiled = catalog(configuration.source.clone());
    swarmie_runtime::start_with_catalog(driver, configuration, compiled).await
}
