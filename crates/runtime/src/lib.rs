//! Minimal Tembra bootstrap and application deployment through compiled or dynamic adapters.

mod application;
mod bootstrap;
mod configuration;
mod deployment;
mod preparation;
mod transport;

use std::{collections::BTreeMap, path::PathBuf, rc::Rc};

use swarmie_composition::EffectiveComposition;
use swarmie_plugin_contracts::AdminProof;
use swarmie_plugin_manager::{ManagerError, PluginManager};
use tembra::{Host, contracts::Contract, core};

pub use application::{Application, CompiledPlugin, application_contracts};
pub use transport::serve;

/// Inspects the default catalog's effective configuration without starting a Host or plugin.
///
/// # Errors
/// Refuses invalid composition, unsupported capability claims and invalid compiled settings.
/// Release metadata remains declarative; package integrity and runtime readiness require
/// deployment. No dynamic code is evaluated and no administrative database is opened.
pub fn resolve_application(
    application: &Application,
) -> Result<EffectiveComposition, Box<dyn std::error::Error>> {
    Ok(preparation::prepare(
        application,
        &application::compiled(PathBuf::new()),
        &application_contracts()?,
    )?
    .effective)
}

/// Supervisor settings. An imported application explicitly replaces persisted desired input.
pub struct Configuration {
    pub database: PathBuf,
    pub source: PathBuf,
    pub proof: AdminProof,
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
/// Refuses invalid composition, unqualified capabilities, configuration or owned Facet metadata.
/// A fatal startup error closes the partial Host and preserves cached bytes and desired input.
/// Dynamic activation failures remain visible in the returned runtime; they do not establish
/// rollback of external effects. Dropping startup is not proof of physical termination.
pub async fn start(
    driver: Rc<dyn core::Driver>,
    configuration: Configuration,
) -> Result<Runtime, Box<dyn std::error::Error>> {
    bootstrap::start(driver, configuration, None, application_contracts()?).await
}

/// Starts an embedding's compiled factory catalog and canonical singleton capability probes.
///
/// Factories only construct declarations from typed configuration. Contract descriptors are
/// actual Tembra artifacts; the catalog neither dispatches calls nor routes object instances.
/// All probed inputs are empty objects, so parameterized services require an owning adapter.
///
/// # Errors
/// Has [`start`]'s startup and partial-effect guarantees. Refuses imported Core metadata,
/// duplicate selections, mismatched factory Facets and capabilities without qualified probes.
pub async fn start_with_catalog(
    driver: Rc<dyn core::Driver>,
    configuration: Configuration,
    compiled: Vec<CompiledPlugin>,
    contracts: Vec<Rc<Contract>>,
) -> Result<Runtime, Box<dyn std::error::Error>> {
    bootstrap::start(driver, configuration, Some(compiled), contracts).await
}
