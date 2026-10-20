//! Minimal node bootstrap: composition, compiled capabilities, administration and recovery.

mod transport;

use std::{path::PathBuf, rc::Rc};

use swarmie_plugin_contracts::{AdminProof, ConfigureProviderRequest};
use swarmie_plugin_manager::{LocalSource, ManagerError, PluginManager};
use tembra::{Host, core};

pub use transport::serve;

/// Bootstrap settings supplied by the embedding supervisor.
pub struct Configuration {
    pub database: PathBuf,
    pub source: PathBuf,
    pub proof: AdminProof,
}

/// One running node; retaining this value keeps administrative ownership alive.
pub struct Runtime {
    pub host: Host,
    pub manager: Rc<PluginManager>,
    /// A failed cached activation; administration remains available for repair.
    pub recovery_error: Option<ManagerError>,
}

/// Starts one node without requiring a browser, hosted service or agent capabilities.
///
/// Installs compiled capabilities before recovering cached dynamic packages. Recovery uses
/// SQLite bytes and does not consult a source. A provider's configured directory is restored
/// for subsequent acquisitions. The caller owns its proof and must never publish it in State.
///
/// # Errors
/// Returns declaration, database or provider-configuration errors. Cached activation failures
/// are retained in inventory and `Runtime::recovery_error`, allowing administrative repair.
/// On a fatal startup failure the
/// partially started Host is closed before returning; existing cached data is preserved.
pub async fn start(
    driver: Rc<dyn core::Driver>,
    configuration: Configuration,
) -> Result<Runtime, Box<dyn std::error::Error>> {
    let host = Host::with_driver("swarmie-runtime", "runtime-supervisor", driver.clone())?;
    let result = async {
        let manager = PluginManager::start(
            host.clone(),
            driver,
            configuration.database,
            configuration.proof.clone(),
        )
        .await?;
        host.install(swarmie_counter::facet()?, core::LoadOptions::default())?
            .await?;
        manager
            .record_core(
                &configuration.proof,
                swarmie_counter::PLUGIN_ID.into(),
                env!("CARGO_PKG_VERSION").into(),
            )
            .await?;
        let configured = manager.provider_configuration(&configuration.proof, "local")?;
        let persist_source = configured.is_none();
        let source = match configured {
            Some(value) => serde_json::from_value::<PathBuf>(value)?,
            None => configuration.source.clone(),
        };
        host.install(
            LocalSource::new("local", source).facet()?,
            core::LoadOptions::default(),
        )?
        .await?;
        manager
            .record_core(
                &configuration.proof,
                "swarmie.local-plugin-source".into(),
                env!("CARGO_PKG_VERSION").into(),
            )
            .await?;
        let recovery_error = match manager.recover(&configuration.proof).await {
            Ok(_) => None,
            Err(
                error @ (ManagerError::Runtime(_)
                | ManagerError::InvalidPackage(_)
                | ManagerError::Integrity(_)
                | ManagerError::Compatibility(_)
                | ManagerError::Dependency(_)),
            ) => Some(error),
            Err(error) => return Err(error.into()),
        };
        if persist_source {
            manager
                .configure_provider(ConfigureProviderRequest {
                    proof: configuration.proof.clone(),
                    provider_id: "local".into(),
                    configuration: serde_json::to_value(&configuration.source)?,
                })
                .await?;
        }
        Ok::<_, Box<dyn std::error::Error>>(Runtime {
            host: host.clone(),
            manager,
            recovery_error,
        })
    }
    .await;
    if result.is_err() {
        host.close().await?;
    }
    result
}
