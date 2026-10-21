use std::{collections::BTreeMap, path::PathBuf, rc::Rc};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use swarmie_composition::*;
use swarmie_plugin_manager::LocalSource;
use tembra::NativeFacet;
use tembra_executor_js::Executor;

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
                        plugins: [
                            swarmie_memory_store::PLUGIN_ID,
                            "swarmie.local-plugin-source",
                        ]
                        .map(|plugin| Selection {
                            id: plugin.into(),
                            plugin: plugin.into(),
                            version: env!("CARGO_PKG_VERSION").into(),
                            provider: None,
                            config: if plugin == swarmie_memory_store::PLUGIN_ID {
                                json!({"database":"memory.sqlite"})
                            } else {
                                json!({})
                            },
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
type LanguageAdapter = dyn Fn(&Executor) -> tembra::Result<()>;

/// Compile-time adapter for selecting native Facets; never a runtime service container.
#[derive(Clone)]
pub struct CompiledPlugin {
    pub release: Release,
    factory: Rc<Factory>,
    language_adapter: Option<Rc<LanguageAdapter>>,
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
            language_adapter: None,
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

    /// Declares this Core Plugin's language codecs and typed Hook adapters.
    ///
    /// The runtime invokes the initializer once per fresh Host, after validating the catalog
    /// and before activating or recovering any Dynamic plugin. All compiled definitions are
    /// initialized, including unselected capabilities, so later Profile selection needs no
    /// bootstrap changes. This registers language types; it must not start services or work.
    /// A later call to this builder replaces the initializer. Any registration failure aborts
    /// startup and closes the partial Host before application Facets are installed.
    pub fn with_language_adapter(
        mut self,
        adapter: impl Fn(&Executor) -> tembra::Result<()> + 'static,
    ) -> Self {
        self.language_adapter = Some(Rc::new(adapter));
        self
    }

    pub(crate) fn register_language_adapter(&self, executor: &Executor) -> tembra::Result<()> {
        if let Some(adapter) = &self.language_adapter {
            adapter(executor)?;
        }
        Ok(())
    }

    pub(crate) fn prepare(&self, configuration: &Value) -> tembra::Result<Vec<NativeFacet>> {
        (self.factory)(configuration)
    }
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

pub(crate) fn compiled(
    source: PathBuf,
    driver: Rc<dyn tembra::core::Driver>,
) -> Vec<CompiledPlugin> {
    let session_driver = driver.clone();
    let bridge_driver = driver.clone();
    vec![
        CompiledPlugin::new(
            release(
                swarmie_task_session_integration::PLUGIN_ID,
                swarmie_task_session_integration::FACET_ID,
            ),
            move |value| {
                let configuration: swarmie_task_session_integration::Configuration =
                    serde_json::from_value(value.clone()).map_err(|error| {
                        tembra::RuntimeError::invalid("binding_configuration", error.to_string())
                    })?;
                Ok(vec![swarmie_task_session_integration::facet(
                    &configuration,
                    bridge_driver.clone(),
                )?])
            },
        ),
        CompiledPlugin::new(
            release(
                swarmie_session_store_sqlite::PLUGIN_ID,
                swarmie_session_store_sqlite::FACET_ID,
            ),
            move |value| {
                let configuration: swarmie_session_store_sqlite::Configuration =
                    serde_json::from_value(value.clone()).map_err(|error| {
                        tembra::RuntimeError::invalid(
                            "session_store_configuration",
                            error.to_string(),
                        )
                    })?;
                Ok(vec![swarmie_session_store_sqlite::facet(
                    &configuration,
                    session_driver.clone(),
                )?])
            },
        ),
        CompiledPlugin::new(
            release(
                swarmie_memory_store::PLUGIN_ID,
                swarmie_memory_store::FACET_ID,
            ),
            |value| {
                let configuration: swarmie_memory_store::Configuration =
                    serde_json::from_value(value.clone()).map_err(|error| {
                        tembra::RuntimeError::invalid(
                            "memory_store_configuration",
                            error.to_string(),
                        )
                    })?;
                Ok(vec![swarmie_memory_store::facet(&configuration)?])
            },
        ),
        CompiledPlugin::new(
            release(
                swarmie_task_storage_sqlite::PLUGIN_ID,
                swarmie_task_storage_sqlite::FACET_ID,
            ),
            |value| {
                let configuration: swarmie_task_storage_sqlite::Configuration =
                    serde_json::from_value(value.clone()).map_err(|error| {
                        tembra::RuntimeError::invalid(
                            "task_storage_configuration",
                            error.to_string(),
                        )
                    })?;
                Ok(vec![swarmie_task_storage_sqlite::facet(&configuration)?])
            },
        ),
        CompiledPlugin::new(
            release(
                swarmie_durable_tasks::PLUGIN_ID,
                swarmie_durable_tasks::FACET_ID,
            ),
            {
                let task_driver = driver.clone();
                move |value| {
                    let configuration: swarmie_durable_tasks::Configuration =
                        serde_json::from_value(value.clone()).map_err(|error| {
                            tembra::RuntimeError::invalid(
                                "durable_tasks_configuration",
                                error.to_string(),
                            )
                        })?;
                    Ok(vec![swarmie_durable_tasks::facet(
                        &configuration,
                        task_driver.clone(),
                    )?])
                }
            },
        )
        .with_language_adapter(swarmie_durable_tasks::register_language_hooks),
        CompiledPlugin::new(
            release(
                swarmie_model_provider_genai::PLUGIN_ID,
                swarmie_model_provider_genai::FACET_ID,
            ),
            {
                let model_driver = driver.clone();
                move |value| {
                    let configuration: swarmie_model_provider_genai::Configuration =
                        serde_json::from_value(value.clone()).map_err(|_| {
                            tembra::RuntimeError::invalid(
                                "model_configuration",
                                "invalid model provider configuration",
                            )
                        })?;
                    Ok(vec![swarmie_model_provider_genai::facet(
                        &configuration,
                        model_driver.clone(),
                    )?])
                }
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

/// The executable chooses default application data placement; bootstrap only composes plugins.
pub(crate) fn default_for(database: &std::path::Path) -> Application {
    let mut application = Application::default();
    if let Some(bundle) = application
        .composition
        .bundles
        .get_mut("swarmie.foundation")
    {
        for selection in &mut bundle.plugins {
            if selection.plugin == swarmie_memory_store::PLUGIN_ID {
                selection.config = json!({"database": database.with_extension("memory.sqlite")});
            }
        }
    }
    application
}
