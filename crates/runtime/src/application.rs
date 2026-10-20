use std::{collections::BTreeMap, path::PathBuf, rc::Rc};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use swarmie_composition::*;
use swarmie_plugin_manager::LocalSource;
use tembra::NativeFacet;

/// A complete imported desired revision. Catalog claims are checked against admitted packages.
///
/// Compiled metadata is supplied by the executable, never replaced by this document. Dynamic
/// plugins acquire selected configuration through the ordinary PluginConfiguration Contract
/// and validate it at their own typed factory boundary before becoming ready.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Application {
    pub composition: Composition,
    #[serde(default)]
    pub releases: Vec<Release>,
}

impl Default for Application {
    fn default() -> Self {
        Self {
            composition: Composition {
                schema_version: 1,
                profile: Profile {
                    id: "headless".into(),
                    bundles: vec!["swarmie.foundation".into()],
                    plugins: vec![],
                    overrides: vec![],
                    disable: vec![],
                },
                bundles: BTreeMap::from([(
                    "swarmie.foundation".into(),
                    Bundle {
                        bundles: vec![],
                        plugins: ["swarmie.counter", "swarmie.local-plugin-source"]
                            .map(|plugin| Selection {
                                id: plugin.into(),
                                plugin: plugin.into(),
                                version: env!("CARGO_PKG_VERSION").into(),
                                provider: None,
                                config: json!({}),
                            })
                            .into(),
                    },
                )]),
            },
            releases: vec![],
        }
    }
}

type Factory = dyn Fn(&Value) -> tembra::Result<Vec<NativeFacet>>;

/// Compile-time adapter for selecting native Facets; never a runtime service container.
pub struct CompiledPlugin {
    pub release: Release,
    factory: Rc<Factory>,
    pub(crate) trusted_sources: Vec<String>,
}

impl CompiledPlugin {
    /// Registers metadata and a pure configuration-to-Facet constructor.
    ///
    /// Construction must validate configuration without external effects. The runtime checks
    /// emitted Facet names against this metadata before installing any application Facets.
    pub fn new(
        release: Release,
        factory: impl Fn(&Value) -> tembra::Result<Vec<NativeFacet>> + 'static,
    ) -> Self {
        Self {
            release,
            factory: Rc::new(factory),
            trusted_sources: vec![],
        }
    }

    /// Attests one source operation identity supplied by this compiled adapter.
    ///
    /// The supervisor remains responsible for source/publisher authenticity. This declaration
    /// is not a package signature and cannot be supplied by an imported Dynamic release.
    pub fn with_trusted_source(mut self, provider_id: String) -> Self {
        self.trusted_sources.push(provider_id);
        self
    }

    pub(crate) fn prepare(&self, configuration: &Value) -> tembra::Result<Vec<NativeFacet>> {
        (self.factory)(configuration)
    }
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

pub(crate) fn compiled(source: PathBuf) -> Vec<CompiledPlugin> {
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
