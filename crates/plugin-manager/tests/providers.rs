use std::path::PathBuf;

use swarmie_plugin_contracts::*;
use swarmie_plugin_manager::PluginManager;
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

struct MemoryProvider {
    package: AcquiredPackage,
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
        Ok(self.package.clone())
    }
}

fn memory_facet(package: AcquiredPackage) -> Result<NativeFacet> {
    NativeFacet::new("test.memory-package-source", vec![], move |_, setup| {
        let package = package.clone();
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
            let proof = PluginManager::issue_proof();
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
                proof.clone(),
            )
            .await
            .unwrap();
            manager
                .record_core(&proof, "swarmie.counter".into(), "0.1.0".into())
                .await
                .unwrap();
            host.install(
                memory_facet(package()).unwrap(),
                core::LoadOptions::default(),
            )
            .unwrap()
            .await
            .unwrap();
            let invalid = manager
                .install(InstallRequest {
                    proof: proof.clone(),
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
                    .inventory(&proof)
                    .unwrap()
                    .plugins
                    .iter()
                    .all(|plugin| plugin.kind == PluginKind::Core)
            );
            let result = manager
                .install(InstallRequest {
                    proof: proof.clone(),
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
                    proof: proof.clone(),
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
