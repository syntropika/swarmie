use std::{cell::Cell as Flag, rc::Rc};

use swarmie_counter::{
    Counter, CounterInput, DynamicConsumer, DynamicConsumerInput, DynamicConsumerProvider,
};
use swarmie_plugin_contracts::{InstallRequest, ObservedState, RUNTIME_HOST};
use swarmie_plugin_manager::{CorePluginDeployment, ManagerError, PluginManager};
use tembra::{core, native::TokioDriver, prelude::*};

struct FailingConsumer {
    invocations: Cell<u32>,
}

impl DynamicConsumerProvider for FailingConsumer {
    fn invocations(&self) -> Readable<u32> {
        self.invocations.readable()
    }

    async fn run(&self, _context: Context) -> Result<u32> {
        Ok(0)
    }
}

#[test]
fn earlier_native_facet_is_used_before_later_failure_and_requires_a_fresh_host() {
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(async {
            let directory = tempfile::tempdir().unwrap();
            let database = directory.path().join("administration.sqlite");
            let host = Host::with_driver(RUNTIME_HOST, "partial", driver.clone()).unwrap();
            let manager = PluginManager::start(host.clone(), driver.clone(), &database)
                .await
                .unwrap();
            let read_before_failure = Rc::new(Flag::new(0));
            let observed = read_before_failure.clone();
            let failing = NativeFacet::new("later-failure", vec![], move |_, setup| {
                let observed = observed.clone();
                let invocations_definition = setup.state("invocations")?;
                setup.serve_with_options(
                    DynamicConsumer,
                    move |_, _, declaration| {
                        let counter = declaration.get(Counter, CounterInput {})?;
                        let observed = observed.clone();
                        Ok(FailingConsumer {
                            invocations: declaration.state(
                                invocations_definition.with_initializer(move |context| {
                                    let counter = counter.clone();
                                    let observed = observed.clone();
                                    async move {
                                        observed.set(counter.with_context(&context).read().await?);
                                        Err(RuntimeError::invalid(
                                            "later_failure",
                                            "later Facet failed after using the earlier provider",
                                        ))
                                    }
                                }),
                            )?,
                        })
                    },
                    core::ServeOptions {
                        lifetime: core::Lifetime::Host,
                        ..Default::default()
                    },
                )
            })
            .unwrap();
            let error = manager
                .recover_composed(
                    vec![CorePluginDeployment {
                        plugin_id: "two-facet-package".into(),
                        version: "1.0.0".into(),
                        facets: vec![swarmie_counter::facet_with_initial(73).unwrap(), failing],
                    }],
                    &[],
                )
                .await
                .unwrap_err();
            assert_eq!(read_before_failure.get(), 73);
            let ManagerError::StartupFailed {
                cause,
                cleanup,
                native_facets,
                ..
            } = &error
            else {
                panic!("unexpected startup error: {error:?}");
            };
            assert_eq!(cause.reason, "later_failure");
            assert!(cleanup.is_none());
            assert_eq!(native_facets.len(), 2);
            assert!(error.requires_restart());
            let retained = host.get(Counter, CounterInput {}).await.unwrap();
            assert_eq!(retained.read().await.unwrap(), 73);
            let inventory = manager.inventory().unwrap();
            let failed = inventory
                .plugins
                .iter()
                .find(|plugin| plugin.plugin_id == "two-facet-package")
                .unwrap();
            assert_eq!(failed.observed, ObservedState::Unavailable);
            for name in [swarmie_counter::FACET_ID, "later-failure"] {
                assert!(inventory.installed_facets.iter().any(|facet| {
                    facet.facet_id == name
                        && facet.kind == swarmie_plugin_contracts::InstalledFacetKind::Native
                }));
            }
            assert!(failed.errors[0].contains("confirmed installed native facets"));
            assert!(matches!(
                manager
                    .install(InstallRequest {
                        plugin_id: "unrelated".into(),
                        provider_id: "local".into(),
                        version: "1.0.0".into(),
                        confirmed: true,
                    })
                    .await,
                Err(ManagerError::RecoveryRequired)
            ));
            host.close().await.unwrap();
            assert!(retained.read().await.is_err());
            drop(manager);

            let restarted = Host::with_driver(RUNTIME_HOST, "restored", driver.clone()).unwrap();
            let manager = PluginManager::start(restarted.clone(), driver.clone(), &database)
                .await
                .unwrap();
            manager
                .recover_composed(
                    vec![CorePluginDeployment {
                        plugin_id: "counter-only".into(),
                        version: "1.0.0".into(),
                        facets: vec![swarmie_counter::facet().unwrap()],
                    }],
                    &[],
                )
                .await
                .unwrap();
            assert_eq!(
                restarted
                    .get(Counter, CounterInput {})
                    .await
                    .unwrap()
                    .read()
                    .await
                    .unwrap(),
                0
            );
            assert!(
                restarted
                    .get(DynamicConsumer, DynamicConsumerInput {})
                    .await
                    .is_err()
            );
            restarted.close().await.unwrap();
        }));
}

#[test]
fn dropping_native_startup_waiter_retains_recovery_requirement() {
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(async {
            let directory = tempfile::tempdir().unwrap();
            let host = Host::with_driver(RUNTIME_HOST, "pending-startup", driver.clone()).unwrap();
            let manager = PluginManager::start(
                host.clone(),
                driver.clone(),
                directory.path().join("administration.sqlite"),
            )
            .await
            .unwrap();
            let entered = Rc::new(tokio::sync::Notify::new());
            let release = Rc::new(tokio::sync::Notify::new());
            let factory_entered = entered.clone();
            let factory_release = release.clone();
            let facet = NativeFacet::new("pending-startup", vec![], move |_, setup| {
                let entered = factory_entered.clone();
                let release = factory_release.clone();
                let invocations_definition = setup.state("invocations")?;
                setup.serve_with_options(
                    DynamicConsumer,
                    move |_, _, declaration| {
                        let entered = entered.clone();
                        let release = release.clone();
                        Ok(FailingConsumer {
                            invocations: declaration.state(
                                invocations_definition.with_initializer(move |_| {
                                    let entered = entered.clone();
                                    let release = release.clone();
                                    async move {
                                        entered.notify_one();
                                        release.notified().await;
                                        Ok(0)
                                    }
                                }),
                            )?,
                        })
                    },
                    core::ServeOptions {
                        lifetime: core::Lifetime::Host,
                        ..Default::default()
                    },
                )
            })
            .unwrap();
            let mut startup = Box::pin(manager.recover_composed(
                vec![CorePluginDeployment {
                    plugin_id: "pending-package".into(),
                    version: "1.0.0".into(),
                    facets: vec![facet],
                }],
                &[],
            ));
            tokio::select! {
                result = &mut startup => panic!("startup unexpectedly settled: {result:?}"),
                _ = entered.notified() => {}
            }
            drop(startup);
            assert!(matches!(
                manager
                    .install(InstallRequest {
                        plugin_id: "unrelated".into(),
                        provider_id: "local".into(),
                        version: "1.0.0".into(),
                        confirmed: true,
                    })
                    .await,
                Err(ManagerError::RecoveryRequired)
            ));
            release.notify_one();
            host.close().await.unwrap();
        }));
}

#[test]
fn acquisition_batch_failure_cannot_be_hidden_by_later_empty_application_recovery() {
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(async {
            let directory = tempfile::tempdir().unwrap();
            let host = Host::with_driver(RUNTIME_HOST, "source-partial", driver.clone()).unwrap();
            let manager = PluginManager::start(
                host.clone(),
                driver.clone(),
                directory.path().join("administration.sqlite"),
            )
            .await
            .unwrap();
            let first = NativeFacet::new("source-first", vec![], |_, _| Ok(())).unwrap();
            let second = NativeFacet::new("source-second", vec![], |_, _| {
                Err(RuntimeError::invalid(
                    "source_setup",
                    "later acquisition adapter setup failed",
                ))
            })
            .unwrap();
            let error = manager
                .install_core_batch(vec![CorePluginDeployment {
                    plugin_id: "source-package".into(),
                    version: "1.0.0".into(),
                    facets: vec![first, second],
                }])
                .await
                .unwrap_err();
            assert!(matches!(error, ManagerError::StartupFailed { .. }));
            assert!(manager.requires_recovery());
            assert!(matches!(
                manager.recover_composed(vec![], &[]).await,
                Err(ManagerError::RecoveryRequired)
            ));
            let source = manager
                .inventory()
                .unwrap()
                .plugins
                .into_iter()
                .find(|plugin| plugin.plugin_id == "source-package")
                .unwrap();
            assert_eq!(source.observed, ObservedState::Unavailable);
            assert!(
                source
                    .errors
                    .iter()
                    .any(|error| error.contains("source_setup"))
            );
            host.close().await.unwrap();
        }));
}
