use std::{future::Future, rc::Rc};

use swarmie_plugin_contracts::*;
use swarmie_plugin_manager::{ManagerError, PluginManager};
use tembra::{core, native::TokioDriver, prelude::*};

fn run<F, Fut>(test: F)
where
    F: FnOnce(Rc<TokioDriver>) -> Fut,
    Fut: Future<Output = ()>,
{
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(test(driver.clone())));
}

#[tembra::contract(local, name = "test.independent-source", version = 1)]
trait IndependentSource {
    #[portable]
    async fn search(&self, request: SearchRequest) -> Result<SearchResult>;
}

struct IndependentProvider;
impl IndependentSourceProvider for IndependentProvider {
    async fn search(&self, request: SearchRequest, _context: Context) -> Result<SearchResult> {
        Ok(SearchResult {
            plugins: vec![PluginSummary {
                id: format!("independent.{}", request.query),
                version: "1.0.0".into(),
                description: "Independent provider".into(),
            }],
        })
    }
}

fn independent_facet() -> Result<NativeFacet> {
    NativeFacet::new("test.independent-source", vec![], |_, setup| {
        setup.serve_with_options(
            IndependentSource,
            |_, _, declaration| {
                declaration.core_mut().register_method(
                    &source_registry()?,
                    "independent/search",
                    tembra::serde_json::to_value(SourceRegistryMetadata {
                        provider_id: "independent".into(),
                        operation: SourceOperation::Search,
                    })
                    .unwrap(),
                    "search",
                )?;
                Ok(IndependentProvider)
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )
    })
}

#[test]
fn independent_registry_provider_requires_no_manager_changes() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let host = Host::with_driver(RUNTIME_HOST, "supervisor", driver.clone()).unwrap();
        let proof = PluginManager::issue_proof();
        let manager = PluginManager::start(
            host.clone(),
            driver,
            directory.path().join("admin.sqlite"),
            proof.clone(),
        )
        .await
        .unwrap();
        manager
            .approve_trusted_source(&proof, "local".into())
            .unwrap();
        assert!(matches!(
            manager
                .source_request::<_, SearchResult>(
                    &proof,
                    "independent",
                    SourceOperation::Search,
                    SearchRequest {
                        query: "example".into()
                    }
                )
                .await,
            Err(ManagerError::SourceUnavailable { .. })
        ));
        host.install(independent_facet().unwrap(), core::LoadOptions::default())
            .unwrap()
            .await
            .unwrap();
        let result: SearchResult = manager
            .source_request(
                &proof,
                "independent",
                SourceOperation::Search,
                SearchRequest {
                    query: "example".into(),
                },
            )
            .await
            .unwrap();
        assert_eq!(result.plugins[0].id, "independent.example");
        assert_eq!(
            manager.inventory(&proof).unwrap().providers[0].provider_id,
            "independent"
        );
        host.close().await.unwrap();
    });
}

#[test]
fn capability_refusal_precedes_effects_and_sqlite_survives_restart() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("admin.sqlite");
        let proof = PluginManager::issue_proof();
        let wrong = AdminProof::new("a forged administrative capability".into());
        let host = Host::with_driver(RUNTIME_HOST, "supervisor", driver.clone()).unwrap();
        let manager = PluginManager::start(host.clone(), driver.clone(), &database, proof.clone())
            .await
            .unwrap();
        manager
            .approve_trusted_source(&proof, "local".into())
            .unwrap();
        let local = host
            .get(PluginAdministration, PluginAdministrationInput {})
            .await
            .unwrap();
        let rejected = local
            .configure_provider(ConfigureProviderRequest {
                proof: wrong.clone(),
                provider_id: "private".into(),
                configuration: tembra::serde_json::json!({"token":"sensitive"}),
            })
            .await
            .unwrap_err();
        assert_eq!(rejected.code, core::ErrorCode::Forbidden);
        assert!(!rejected.to_string().contains(wrong.as_str()));
        assert!(manager.inventory(&proof).unwrap().providers.is_empty());
        let pending = InstallRequest {
            proof: proof.clone(),
            provider_id: "unknown".into(),
            plugin_id: "example".into(),
            version: "1.0.0".into(),
            confirmed: false,
        };
        assert!(matches!(
            manager.install(pending).await,
            Err(ManagerError::ConfirmationRequired)
        ));
        assert!(manager.inventory(&proof).unwrap().operations.is_empty());
        manager
            .configure_provider(ConfigureProviderRequest {
                proof: proof.clone(),
                provider_id: "local".into(),
                configuration: tembra::serde_json::json!({"token":"sensitive","root":"packages"}),
            })
            .await
            .unwrap();
        assert!(
            local
                .inventory(InventoryRequest {
                    proof: wrong.clone()
                })
                .await
                .is_err()
        );
        let inventory = local
            .inventory(InventoryRequest {
                proof: proof.clone(),
            })
            .await
            .unwrap();
        assert!(
            !serde_json::to_string(&inventory)
                .unwrap()
                .contains("sensitive")
        );
        assert!(!format!("{proof:?}").contains(proof.as_str()));
        assert!(matches!(
            manager
                .deactivate(DeactivateRequest {
                    proof: proof.clone(),
                    plugin_id: "swarmie.plugin-manager".into()
                })
                .await,
            Err(ManagerError::CorePlugin(_))
        ));
        local.core().release();
        host.close().await.unwrap();
        assert!(manager.inventory(&proof).is_err());
        drop(manager);
        let next = Host::with_driver(RUNTIME_HOST, "supervisor", driver.clone()).unwrap();
        let recovered = PluginManager::start(next.clone(), driver, &database, proof.clone())
            .await
            .unwrap();
        assert_eq!(
            recovered
                .provider_configuration(&proof, "local")
                .unwrap()
                .unwrap()["token"],
            "sensitive"
        );
        assert_eq!(recovered.inventory(&proof).unwrap().operations.len(), 1);
        next.close().await.unwrap();
    });
}

#[tembra::contract(local, name = "test.loose-source", version = 1)]
trait LooseSource {
    #[portable]
    async fn search(&self, request: tembra::serde_json::Value) -> Result<SearchResult>;
}

struct LooseProvider {
    calls: Rc<std::cell::Cell<u32>>,
}
impl LooseSourceProvider for LooseProvider {
    async fn search(
        &self,
        _request: tembra::serde_json::Value,
        _context: Context,
    ) -> Result<SearchResult> {
        self.calls.set(self.calls.get() + 1);
        Ok(SearchResult { plugins: vec![] })
    }
}

#[test]
fn advertised_source_operation_must_match_canonical_schema_before_invocation() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let calls = Rc::new(std::cell::Cell::new(0));
        let host = Host::with_driver(RUNTIME_HOST, "supervisor", driver.clone()).unwrap();
        let captured = calls.clone();
        host.install(
            NativeFacet::new("test.loose-source", vec![], move |_, setup| {
                let calls = captured.clone();
                setup.serve_with_options(
                    LooseSource,
                    move |_, _, declaration| {
                        declaration.core_mut().register_method(
                            &source_registry()?,
                            "loose/search",
                            tembra::serde_json::to_value(SourceRegistryMetadata {
                                provider_id: "loose".into(),
                                operation: SourceOperation::Search,
                            })
                            .unwrap(),
                            "search",
                        )?;
                        Ok(LooseProvider {
                            calls: calls.clone(),
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
        let proof = PluginManager::issue_proof();
        let manager = PluginManager::start(
            host.clone(),
            driver,
            directory.path().join("admin.sqlite"),
            proof.clone(),
        )
        .await
        .unwrap();
        manager
            .approve_trusted_source(&proof, "local".into())
            .unwrap();
        let refused = manager
            .source_request::<_, SearchResult>(
                &proof,
                "loose",
                SourceOperation::Search,
                SearchRequest {
                    query: "example".into(),
                },
            )
            .await;
        assert!(matches!(refused, Err(ManagerError::SourceSchema { .. })));
        assert_eq!(calls.get(), 0);
        host.close().await.unwrap();
    });
}
