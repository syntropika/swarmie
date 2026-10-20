use std::collections::BTreeSet;

use tembra::core;

use crate::{ManagerError, PluginManager, Result};

impl PluginManager {
    /// Installs an owned initial batch and logically retires foreign definitions on failure.
    /// The embedding must preserve this manager's exclusive ownership of those identities.
    /// Cleanup compares actual installed catalogs before and after admission; preexisting
    /// definitions are never removed by a failed initial batch. Native definitions cannot
    /// be removed through reload, and failed cleanup always requires a fresh Host.
    pub(crate) async fn load_initial(&self, definitions: Vec<core::ExecutableFacet>) -> Result<()> {
        if definitions.is_empty() {
            return Ok(());
        }
        let owned = definitions
            .iter()
            .map(|facet| facet.name().to_owned())
            .collect::<BTreeSet<_>>();
        let previous = self
            .host
            .core()
            .installed_facets()
            .into_iter()
            .map(|facet| facet.name().to_owned())
            .collect::<BTreeSet<_>>();
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
        let newly_installed = self
            .host
            .core()
            .installed_facets()
            .into_iter()
            .filter(|facet| owned.contains(facet.name()) && !previous.contains(facet.name()))
            .collect::<Vec<_>>();
        let foreign_facets = newly_installed
            .iter()
            .filter(|facet| facet.kind() == core::FacetKind::Foreign)
            .map(|facet| facet.name().to_owned())
            .collect::<Vec<_>>();
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
            native_facets: self
                .host
                .core()
                .installed_facets()
                .into_iter()
                .filter(|facet| {
                    facet.kind() == core::FacetKind::Native
                        && owned.contains(facet.name())
                        && !previous.contains(facet.name())
                })
                .map(|facet| facet.name().to_owned())
                .collect(),
            lingering_physical,
        };
        self.restart_required.set(error.requires_restart());
        Err(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{future::Future, rc::Rc};
    use tembra::{Host, NativeFacet, native::TokioDriver};

    #[tembra::contract(local, name = "test.cleanup-blocker", version = 1)]
    trait CleanupBlocker {
        #[portable]
        async fn ping(&self) -> tembra::Result<()>;
    }

    struct Blocker {
        _resource: core::Resource<()>,
    }

    impl CleanupBlockerProvider for Blocker {
        async fn ping(&self, _context: tembra::Context) -> tembra::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn duplicate_refusal_never_removes_preexisting_native_or_foreign_definitions() {
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
                    cleanup,
                    native_facets,
                    ..
                } = &failure
                else {
                    panic!("unexpected cleanup result: {failure:?}");
                };
                assert_eq!(cause.reason, "duplicate_facet");
                assert!(cleanup.is_none());
                assert!(native_facets.is_empty());
                assert!(!failure.requires_restart());
                assert!(!manager.restart_required.get());
                // Native ownership remains authoritative after the refused duplicate admission.
                let duplicate = core::NativeFacet::new("occupied", vec![], |_, _| Ok(())).unwrap();
                assert_eq!(
                    host.core()
                        .install(duplicate, core::LoadOptions::default())
                        .err()
                        .unwrap()
                        .reason,
                    "duplicate_facet"
                );
                let foreign =
                    core::ExecutableFacet::foreign("occupied-foreign", vec![], |_, _| Ok(()))
                        .unwrap();
                host.core()
                    .load_executable(vec![foreign], core::LoadOptions::default())
                    .unwrap()
                    .await
                    .unwrap();
                let duplicate =
                    core::ExecutableFacet::foreign("occupied-foreign", vec![], |_, _| Ok(()))
                        .unwrap();
                let refusal = manager.load_initial(vec![duplicate]).await.unwrap_err();
                assert!(!refusal.requires_restart());
                assert!(
                    host.core()
                        .installed_facets()
                        .iter()
                        .any(|facet| facet.name() == "occupied-foreign"
                            && facet.kind() == core::FacetKind::Foreign)
                );
                host.close().await.unwrap();
            }));
    }

    #[test]
    fn cleanup_refusal_preserves_original_error_when_host_closes_after_partial_admission() {
        let driver = TokioDriver::new();
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(driver.run_until(async {
                let directory = tempfile::tempdir().unwrap();
                let host = Host::with_driver(
                    swarmie_plugin_contracts::RUNTIME_HOST,
                    "closing-cleanup",
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
                let release = Rc::new(tokio::sync::Notify::new());
                let closer = release.clone();
                host.install(
                    NativeFacet::new("cleanup-blocker", vec![], move |_, setup| {
                        let closer = closer.clone();
                        setup.serve_with_options(
                            CleanupBlocker,
                            move |_, _, declaration| {
                                let closer = closer.clone();
                                let resource = declaration.core_mut().resource(
                                    "pending-close",
                                    core::ResourceDeclaration::new(|_| Box::pin(async { Ok(()) }))
                                        .with_close(move |_| {
                                            let closer = closer.clone();
                                            Box::pin(async move {
                                                closer.notified().await;
                                                Ok(())
                                            })
                                        }),
                                )?;
                                Ok(Blocker {
                                    _resource: resource,
                                })
                            },
                            core::ServeOptions {
                                lifetime: core::Lifetime::Host,
                                ..Default::default()
                            },
                        )
                    })
                    .unwrap(),
                    core::LoadOptions::default(),
                )
                .unwrap()
                .await
                .unwrap();
                let owned =
                    core::ExecutableFacet::foreign("new-owned", vec![], |_, _| Ok(())).unwrap();
                let closing = host.clone();
                let later = core::ExecutableFacet::foreign("later-fails", vec![], move |_, _| {
                    let mut close = Box::pin(closing.core().close());
                    let waker = futures::task::noop_waker();
                    assert!(
                        close
                            .as_mut()
                            .poll(&mut std::task::Context::from_waker(&waker))
                            .is_pending()
                    );
                    Err(core::RuntimeError::invalid(
                        "later_setup",
                        "later setup failed after Host closure began",
                    ))
                })
                .unwrap();
                let failure = manager.load_initial(vec![owned, later]).await.unwrap_err();
                let ManagerError::StartupFailed {
                    cause,
                    cleanup: Some(cleanup),
                    ..
                } = &failure
                else {
                    panic!("unexpected cleanup failure: {failure:?}");
                };
                assert_eq!(cause.reason, "later_setup");
                assert_eq!(cleanup.reason, "closed");
                assert!(failure.requires_restart());
                assert!(manager.restart_required.get());
                release.notify_one();
                host.close().await.unwrap();
            }));
    }
}
