//! Minimal Tembra bootstrap and application deployment through compiled or dynamic adapters.

mod application;
mod bootstrap;
mod configuration;
mod deployment;
mod preparation;
mod transport;

use std::{collections::BTreeMap, path::PathBuf, rc::Rc};

use swarmie_composition::EffectiveComposition;
use swarmie_plugin_manager::{ManagerError, PluginManager};
use tembra::{Host, core};

pub use application::{Application, CompiledPlugin};
pub use transport::serve;

/// Inspects the default catalog's effective configuration without starting a Host or plugin.
///
/// # Errors
/// Refuses invalid composition and invalid compiled settings.
/// Release metadata remains declarative; Contract compatibility and runtime readiness require
/// deployment. No dynamic code is evaluated and no administrative database is opened.
pub fn resolve_application(
    application: &Application,
) -> Result<EffectiveComposition, Box<dyn std::error::Error>> {
    Ok(preparation::prepare(application, &application::compiled(PathBuf::new()))?.effective)
}

/// Supervisor settings. An imported application explicitly replaces persisted desired input.
pub struct Configuration {
    pub database: PathBuf,
    pub source: PathBuf,
    /// None restores a persisted application, or uses the compiled default when unconfigured.
    pub application: Option<Application>,
}

/// One node with administrative ownership and an inspectable desired application.
pub struct Runtime {
    pub host: Host,
    pub manager: Rc<PluginManager>,
    pub effective: EffectiveComposition,
    /// Actual deployment refusals keyed by configured instance, separate from metadata readiness.
    pub deployment_errors: BTreeMap<String, String>,
    /// A failed cached activation; administration remains available for supervised repair.
    pub recovery_error: Option<ManagerError>,
}

/// Starts the default compiled application catalog on a fresh Tembra Host.
///
/// Explicit or persisted Profiles select Core and Dynamic providers before installation.
/// Native provider substitution requires a fresh Host. With no imported Profile, the compiled
/// default is resolved through the same model and standalone administration remains available.
///
/// # Errors
/// Refuses invalid composition, configuration or owned Facet metadata.
/// A fatal startup error closes the partial Host and preserves cached bytes and desired input.
/// Dynamic activation failures remain visible in the returned runtime; they do not establish
/// rollback of external effects. Dropping startup is not proof of physical termination.
pub async fn start(
    driver: Rc<dyn core::Driver>,
    configuration: Configuration,
) -> Result<Runtime, Box<dyn std::error::Error>> {
    bootstrap::start(driver, configuration, None).await
}

/// Starts an embedding's compiled factory catalog through Tembra's actual batch installation.
///
/// Factories construct declarations from typed configuration. The catalog neither dispatches
/// calls nor routes object instances. Ordinary consumers acquire their actual Tembra Contracts;
/// no parallel capability metadata determines their compatibility or readiness.
///
/// # Errors
/// Has [`start`]'s startup and partial-effect guarantees. Refuses imported Core metadata,
/// duplicate selections and mismatched factory Facets.
pub async fn start_with_catalog(
    driver: Rc<dyn core::Driver>,
    configuration: Configuration,
    compiled: Vec<CompiledPlugin>,
) -> Result<Runtime, Box<dyn std::error::Error>> {
    bootstrap::start(driver, configuration, Some(compiled)).await
}
