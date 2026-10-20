use std::{cell::RefCell, path::PathBuf, rc::Rc};

use futures::channel::oneshot;

use swarmie_counter::{DynamicConsumer, DynamicConsumerInput};
use swarmie_plugin_contracts::*;
use swarmie_plugin_manager::{CorePluginDeployment, PluginManager};
use tembra::{core, native::TokioDriver, prelude::*};

#[tembra::contract(local, name = "test.memory-package-source", version = 1)]
trait MemorySource {
    #[portable]
    async fn search(&self, request: SearchRequest) -> Result<SearchResult>;
    #[portable]
    async fn resolve(&self, request: ResolveRequest) -> Result<ResolvedPlugin>;
    #[portable]
    async fn inspect(&self, request: InspectRequest) -> Result<PluginManifest>;
    #[portable]
    async fn acquire(&self, request: AcquireRequest) -> Result<AcquiredPackage>;
}

struct AcquisitionBarrier {
    started: RefCell<Option<oneshot::Sender<()>>>,
    released: RefCell<Option<oneshot::Receiver<()>>>,
}

struct MemoryProvider {
    package: AcquiredPackage,
    acquisition: Option<Rc<AcquisitionBarrier>>,
}

impl MemorySourceProvider for MemoryProvider {
    async fn search(&self, _request: SearchRequest, _context: Context) -> Result<SearchResult> {
        Ok(SearchResult {
            plugins: vec![PluginSummary {
                id: self.package.manifest.id.clone(),
                version: self.package.manifest.version.clone(),
                description: "Independent memory provider".into(),
            }],
        })
    }
    async fn resolve(&self, request: ResolveRequest, _context: Context) -> Result<ResolvedPlugin> {
        if request.plugin_id != self.package.manifest.id {
            return Err(RuntimeError::invalid("release", "release not found"));
        }
        Ok(ResolvedPlugin {
            manifest: self.package.manifest.clone(),
        })
    }
    async fn inspect(&self, request: InspectRequest, _context: Context) -> Result<PluginManifest> {
        if request.plugin_id != self.package.manifest.id
            || request.version != self.package.manifest.version
        {
            return Err(RuntimeError::invalid("release", "release not found"));
        }
        Ok(self.package.manifest.clone())
    }
    async fn acquire(&self, request: AcquireRequest, _context: Context) -> Result<AcquiredPackage> {
        if request.plugin_id != self.package.manifest.id
            || request.version != self.package.manifest.version
        {
            return Err(RuntimeError::invalid("release", "release not found"));
        }
        if let Some(barrier) = &self.acquisition {
            let entered = barrier
                .started
                .borrow_mut()
                .take()
                .expect("acquire starts once");
            entered
                .send(())
                .expect("the test observes actual provider entry");
            let released = barrier
                .released
                .borrow_mut()
                .take()
                .expect("one pending acquire");
            released.await.map_err(|_| {
                RuntimeError::aborted(
                    "acquisition_interrupted",
                    "the acquisition boundary was closed",
                )
            })?;
        }
        Ok(self.package.clone())
    }
}

fn memory_facet(package: AcquiredPackage) -> Result<NativeFacet> {
    memory_facet_with_barrier(package, None)
}

fn memory_facet_with_barrier(
    package: AcquiredPackage,
    acquisition: Option<Rc<AcquisitionBarrier>>,
) -> Result<NativeFacet> {
    NativeFacet::new("test.memory-package-source", vec![], move |_, setup| {
        let package = package.clone();
        let acquisition = acquisition.clone();
        setup.serve_with_options(
            MemorySource,
            move |_, _, declaration| {
                for (operation, method) in [
                    (SourceOperation::Search, "search"),
                    (SourceOperation::Resolve, "resolve"),
                    (SourceOperation::Inspect, "inspect"),
                    (SourceOperation::Acquire, "acquire"),
                ] {
                    declaration.core_mut().register_method(
                        &source_registry()?,
                        format!("memory/{method}"),
                        tembra::serde_json::to_value(SourceRegistryMetadata {
                            provider_id: "memory".into(),
                            operation,
                        })
                        .unwrap(),
                        method,
                    )?;
                }
                Ok(MemoryProvider {
                    package: package.clone(),
                    acquisition: acquisition.clone(),
                })
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )
    })
}

fn package() -> AcquiredPackage {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/fixtures/dynamic-consumer/1.0.0");
    let manifest: PluginManifest =
        serde_json::from_slice(&std::fs::read(root.join("manifest.json")).unwrap()).unwrap();
    let files = manifest
        .files
        .iter()
        .map(|file| PackageFile {
            path: file.path.clone(),
            bytes: std::fs::read(root.join(&file.path)).unwrap(),
        })
        .collect();
    AcquiredPackage {
        manifest,
        files,
        provenance: PackageProvenance {
            provider_id: "memory".into(),
            source: "memory:approved-fixture".into(),
            integrity: "fixture supplied from independently registered provider".into(),
        },
    }
}

#[test]
fn independent_memory_provider_installs_real_artifacts_through_generic_registry_dispatch() {
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(async {
            let directory = tempfile::tempdir().unwrap();

            let host = Host::with_driver(RUNTIME_HOST, "supervisor", driver.clone()).unwrap();
            host.install(
                swarmie_counter::facet().unwrap(),
                core::LoadOptions::default(),
            )
            .unwrap()
            .await
            .unwrap();
            let manager = PluginManager::start(
                host.clone(),
                driver.clone(),
                directory.path().join("admin.sqlite"),
            )
            .await
            .unwrap();
            manager.approve_trusted_source("local".into()).unwrap();
            manager
                .record_core_facets(
                    "swarmie.counter".into(),
                    "0.1.0".into(),
                    vec![swarmie_counter::FACET_ID.into()],
                )
                .await
                .unwrap();
            host.install(
                memory_facet(package()).unwrap(),
                core::LoadOptions::default(),
            )
            .unwrap()
            .await
            .unwrap();
            assert!(matches!(
                manager
                    .install(InstallRequest {
                        provider_id: "memory".into(),
                        plugin_id: "dynamic-consumer".into(),
                        version: "1.0.0".into(),
                        confirmed: true
                    })
                    .await,
                Err(swarmie_plugin_manager::ManagerError::UntrustedSource(_))
            ));
            manager.approve_trusted_source("memory".into()).unwrap();
            let invalid = manager
                .install(InstallRequest {
                    provider_id: "memory".into(),
                    plugin_id: "dynamic-consumer".into(),
                    version: "2.0.0".into(),
                    confirmed: true,
                })
                .await;
            assert!(matches!(
                invalid,
                Err(swarmie_plugin_manager::ManagerError::Integrity(_))
            ));
            assert!(
                manager
                    .inventory()
                    .unwrap()
                    .plugins
                    .iter()
                    .all(|plugin| plugin.kind == PluginKind::Core)
            );
            let result = manager
                .install(InstallRequest {
                    provider_id: "memory".into(),
                    plugin_id: "dynamic-consumer".into(),
                    version: "1.0.0".into(),
                    confirmed: true,
                })
                .await
                .unwrap();
            assert_eq!(result.providers[0].operations.len(), 4);
            assert_eq!(
                result
                    .plugins
                    .iter()
                    .find(|plugin| plugin.plugin_id == "dynamic-consumer")
                    .unwrap()
                    .provider_id,
                "memory"
            );
            manager
                .activate(ActivateRequest {
                    plugin_id: "dynamic-consumer".into(),
                })
                .await
                .unwrap();
            let consumer = host
                .get(DynamicConsumer, DynamicConsumerInput {})
                .await
                .unwrap();
            assert_eq!(consumer.run().await.unwrap(), 1);
            consumer.core().release();
            host.close().await.unwrap();
        }));
}

#[test]
fn dropping_pending_acquisition_retains_running_evidence_until_fresh_host_recovery() {
    use std::{future::Future, task::Poll};

    use swarmie_plugin_manager::ManagerError;

    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(async {
            let directory = tempfile::tempdir().unwrap();
            let database = directory.path().join("admin.sqlite");

            let (entered, mut started) = oneshot::channel();
            let (release, released) = oneshot::channel();
            let barrier = Rc::new(AcquisitionBarrier {
                started: RefCell::new(Some(entered)),
                released: RefCell::new(Some(released)),
            });
            let host = Host::with_driver(RUNTIME_HOST, "supervisor", driver.clone()).unwrap();
            let manager = PluginManager::start(host.clone(), driver.clone(), &database)
                .await
                .unwrap();
            manager.approve_trusted_source("memory".into()).unwrap();
            host.install(
                memory_facet_with_barrier(package(), Some(barrier)).unwrap(),
                core::LoadOptions::default(),
            )
            .unwrap()
            .await
            .unwrap();
            let mut installation = Box::pin(manager.install(InstallRequest {
                provider_id: "memory".into(),
                plugin_id: "dynamic-consumer".into(),
                version: "1.0.0".into(),
                confirmed: true,
            }));
            // Drive actual source dispatch until Acquire confirms entry, rather than inferring
            // progress from elapsed time or assuming a single poll reaches the provider.
            futures::future::poll_fn(|context| {
                assert!(installation.as_mut().poll(context).is_pending());
                match std::pin::Pin::new(&mut started).poll(context) {
                    Poll::Ready(result) => {
                        result.expect("Acquire reached its pending boundary");
                        Poll::Ready(())
                    }
                    Poll::Pending => Poll::Pending,
                }
            })
            .await;
            let running = manager.inventory().unwrap();
            let operation = running.operations.last().unwrap();
            let operation_id = operation.id.clone();
            assert_eq!(operation.operation, AdminOperation::Install);
            assert_eq!(operation.status, OperationStatus::Running);
            assert!(
                running
                    .plugins
                    .iter()
                    .all(|plugin| plugin.kind == PluginKind::Core)
            );
            assert!(
                host.get(DynamicConsumer, DynamicConsumerInput {})
                    .await
                    .is_err()
            );
            drop(installation);
            assert!(manager.requires_recovery());
            let retained = manager.inventory().unwrap();
            assert_eq!(
                retained.operations.last().unwrap().status,
                OperationStatus::Running
            );
            assert!(matches!(
                manager
                    .configure_provider(ConfigureProviderRequest {
                        provider_id: "memory".into(),
                        configuration: serde_json::json!({}),
                    })
                    .await,
                Err(ManagerError::RecoveryRequired)
            ));
            // Closing the barrier lets any physically retained source invocation settle. Dropping
            // its administrative observer was not treated as proof of rollback or termination.
            drop(release);
            host.close().await.unwrap();
            drop(manager);
            let fresh_host = Host::with_driver(RUNTIME_HOST, "supervisor", driver.clone()).unwrap();
            let restarted = PluginManager::start(fresh_host.clone(), driver.clone(), &database)
                .await
                .unwrap();
            assert!(!restarted.requires_recovery());
            restarted
                .install_core_batch(vec![CorePluginDeployment {
                    plugin_id: "memory-provider".into(),
                    version: "1.0.0".into(),
                    facets: vec![memory_facet(package()).unwrap()],
                }])
                .await
                .unwrap();
            let recovered = restarted.recover().await.unwrap();
            let interrupted = recovered
                .operations
                .iter()
                .find(|operation| operation.id == operation_id)
                .unwrap();
            assert_eq!(interrupted.status, OperationStatus::Interrupted);
            assert!(interrupted.error.contains("external effects are unknown"));
            assert!(
                recovered
                    .plugins
                    .iter()
                    .all(|plugin| plugin.kind == PluginKind::Core)
            );
            assert!(
                fresh_host
                    .get(DynamicConsumer, DynamicConsumerInput {})
                    .await
                    .is_err()
            );
            restarted
                .configure_provider(ConfigureProviderRequest {
                    provider_id: "memory".into(),
                    configuration: serde_json::json!({}),
                })
                .await
                .unwrap();
            fresh_host.close().await.unwrap();
        }));
}
