use tembra::core;

use crate::{ManagerError, PluginManager, Result};

impl PluginManager {
    /// Installs an owned initial batch and logically retires foreign definitions on failure.
    /// The embedding must preserve this manager's exclusive ownership of those identities.
    /// Tembra has no public installed-Facet snapshot at the pinned revision; this does not
    /// infer which declarations survived from package metadata. Native definitions cannot
    /// be removed through reload, and failed cleanup always requires a fresh Host.
    pub(crate) async fn load_initial(&self, definitions: Vec<core::ExecutableFacet>) -> Result<()> {
        if definitions.is_empty() {
            return Ok(());
        }
        let native_facets = definitions
            .iter()
            .filter(|facet| facet.kind() == core::FacetKind::Native)
            .map(|facet| facet.name().to_owned())
            .collect::<Vec<_>>();
        let foreign_facets = definitions
            .iter()
            .filter(|facet| facet.kind() == core::FacetKind::Foreign)
            .map(|facet| facet.name().to_owned())
            .collect::<Vec<_>>();
        // A dropped readiness/cleanup waiter does not stop Tembra-owned work. Keep a
        // conservative session barrier until actual settlement establishes continuation.
        self.restart_required.set(true);
        let result = match self
            .host
            .core()
            .load_executable(definitions, core::LoadOptions::default())
        {
            Ok(startup) => startup.await,
            Err(error) => Err(error),
        };
        let Err(cause) = result else {
            self.restart_required.set(false);
            return Ok(());
        };
        let prior_reports = self.host.core().errors().len();
        let (cleanup, lingering_physical) = if foreign_facets.is_empty() {
            (None, 0)
        } else {
            match self
                .host
                .core()
                .reload(
                    vec![],
                    core::ReloadOptions {
                        remove: foreign_facets,
                        ..Default::default()
                    },
                )
                .await
            {
                Ok(report) => (None, report.lingering_physical()),
                Err(error) => (Some(error), 0),
            }
        };
        let error = ManagerError::StartupFailed {
            cause: Box::new(cause),
            cleanup: cleanup.map(Box::new),
            cleanup_reports: self
                .host
                .core()
                .errors()
                .into_iter()
                .skip(prior_reports)
                .collect(),
            native_facets,
            lingering_physical,
        };
        self.restart_required.set(error.requires_restart());
        Err(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tembra::{Host, native::TokioDriver};

    #[test]
    fn actual_cleanup_refusal_preserves_both_runtime_causes_and_preexisting_native_code() {
        let driver = TokioDriver::new();
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(driver.run_until(async {
                let directory = tempfile::tempdir().unwrap();
                let host = Host::with_driver(
                    swarmie_plugin_contracts::RUNTIME_HOST,
                    "cleanup",
                    driver.clone(),
                )
                .unwrap();
                let manager = PluginManager::start(
                    host.clone(),
                    driver.clone(),
                    directory.path().join("administration.sqlite"),
                )
                .await
                .unwrap();
                let native = core::NativeFacet::new("occupied", vec![], |_, _| Ok(())).unwrap();
                host.core()
                    .install(native, core::LoadOptions::default())
                    .unwrap()
                    .await
                    .unwrap();
                let candidate =
                    core::ExecutableFacet::foreign("occupied", vec![], |_, _| Ok(())).unwrap();
                let failure = manager.load_initial(vec![candidate]).await.unwrap_err();
                let ManagerError::StartupFailed {
                    cause,
                    cleanup: Some(cleanup),
                    ..
                } = &failure
                else {
                    panic!("unexpected cleanup result: {failure:?}");
                };
                assert_eq!(cause.reason, "duplicate_facet");
                assert_eq!(cleanup.reason, "native_reload");
                assert!(failure.requires_restart());
                assert!(manager.restart_required.get());
                // Native ownership remains authoritative after both refusals.
                let duplicate = core::NativeFacet::new("occupied", vec![], |_, _| Ok(())).unwrap();
                assert_eq!(
                    host.core()
                        .install(duplicate, core::LoadOptions::default())
                        .err()
                        .unwrap()
                        .reason,
                    "duplicate_facet"
                );
                host.close().await.unwrap();
            }));
    }
}
