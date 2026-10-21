//! Real packaged TypeScript contributions use their Core Plugin's language adapter on startup,
//! coordinated reload and source-free cached recovery. No model or loop implementation is needed.

use std::{cell::RefCell, path::Path, rc::Rc, time::Duration};

use futures::FutureExt;
use serde_json::json;
use swarmie_agent_contracts::{PreparedContext, TurnContext, prepare_context};
use swarmie_composition::{FacetIdentity, FacetTarget, PluginKind, Release, Selection};
use swarmie_runtime::{Application, Configuration, Runtime, TransitionOutcome};
use tembra::{NativeFacet, core};
use tembra_hooks::{HookManager, HookManagerOptions};
use tembra_native::TokioDriver;

fn application(root: &Path, version: &str) -> Application {
    let mut application = Application::default();
    for bundle in application.composition.bundles.values_mut() {
        for selection in &mut bundle.plugins {
            if selection.plugin == swarmie_memory_store::PLUGIN_ID {
                selection.config = json!({"database":root.join("memory.sqlite")});
            }
        }
    }
    application.composition.profile.plugins.push(Selection {
        id: "context".into(),
        plugin: "agent-context".into(),
        version: version.into(),
        provider: Some("local".into()),
        config: json!({}),
    });
    application.releases.push(Release {
        plugin: "agent-context".into(),
        version: version.into(),
        kind: PluginKind::Dynamic,
        facets: vec![FacetIdentity {
            id: "fixture.agent-context".into(),
            target: FacetTarget::Server,
        }],
    });
    application
}

fn copy_packages(source: &Path) {
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/fixtures/agent-context");
    for version in ["1.0.0", "1.1.0"] {
        let from = fixtures.join(version);
        assert!(
            from.join("manifest.json").is_file(),
            "build real TypeScript fixtures with just tembra-plugins"
        );
        let to = source.join(version);
        std::fs::create_dir_all(&to).unwrap();
        for filename in ["manifest.json", "artifact.json"] {
            std::fs::copy(from.join(filename), to.join(filename)).unwrap();
        }
    }
}

async fn runner(runtime: &Runtime, driver: Rc<TokioDriver>) -> HookManager {
    let captured = Rc::new(RefCell::new(None));
    let selected = captured.clone();
    runtime
        .host
        .install(
            NativeFacet::new("fixture.runtime-context-runner", vec![], move |scope, _| {
                *selected.borrow_mut() = Some(scope.clone());
                Ok(())
            })
            .unwrap(),
            core::LoadOptions::default(),
        )
        .unwrap()
        .await
        .unwrap();
    HookManager::new(
        captured.borrow_mut().take().unwrap(),
        driver,
        HookManagerOptions::default(),
    )
    .unwrap()
}

async fn invoke(runtime: &Runtime, hooks: HookManager) -> String {
    let result = runtime
        .host
        .core()
        .root(move |context| {
            async move {
                hooks
                    .run_waterfall(
                        &prepare_context()?,
                        PreparedContext {
                            turn: TurnContext {
                                turn_id: "runtime-turn".into(),
                                session_id: "runtime-session".into(),
                                conversation_id: "runtime-conversation".into(),
                                task_id: "runtime-task".into(),
                                step: 0,
                            },
                            messages: vec![],
                        },
                        &context,
                    )
                    .await
            }
            .boxed_local()
        })
        .await
        .unwrap();
    serde_json::to_string(&result.messages).unwrap()
}

#[test]
fn runtime_owned_agent_adapter_supports_typescript_reload_and_cached_recovery() {
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(async {
            tokio::time::timeout(Duration::from_secs(20), async {
                let directory = tempfile::tempdir().unwrap();
                let source = directory.path().join("source");
                copy_packages(&source);
                let configuration = |application| Configuration {
                    database: directory.path().join("admin.sqlite"),
                    source: source.clone(),
                    application,
                };
                let mut runtime = swarmie_runtime::start(
                    driver.clone(),
                    configuration(Some(application(directory.path(), "1.0.0"))),
                )
                .await
                .unwrap();
                assert!(
                    runtime.deployment_errors.is_empty(),
                    "{:?}",
                    runtime.deployment_errors
                );
                assert!(runtime.recovery_error.is_none());
                let hooks = runner(&runtime, driver.clone()).await;
                assert!(
                    invoke(&runtime, hooks.clone())
                        .await
                        .contains("dynamic-context-v1")
                );
                let previous_epoch = runtime.host.core().code_version();
                let report = runtime
                    .apply_application(application(directory.path(), "1.1.0"))
                    .await
                    .unwrap();
                assert_eq!(
                    report.results["context"].outcome,
                    TransitionOutcome::Applied,
                    "{report:?}"
                );
                assert!(runtime.host.core().code_version() > previous_epoch);
                let rendered = invoke(&runtime, hooks).await;
                assert!(rendered.contains("dynamic-context-v2"));
                assert!(!rendered.contains("dynamic-context-v1"));
                runtime.host.close().await.unwrap();
                drop(runtime);
                std::fs::remove_dir_all(&source).unwrap();
                let recovered = swarmie_runtime::start(driver.clone(), configuration(None))
                    .await
                    .unwrap();
                assert!(
                    recovered.deployment_errors.is_empty(),
                    "{:?}",
                    recovered.deployment_errors
                );
                assert!(recovered.recovery_error.is_none());
                let hooks = runner(&recovered, driver.clone()).await;
                let rendered = invoke(&recovered, hooks).await;
                assert!(rendered.contains("dynamic-context-v2"));
                assert!(!rendered.contains("dynamic-context-v1"));
                recovered.host.close().await.unwrap();
            })
            .await
            .expect("runtime TypeScript hook lifecycle did not settle");
        }));
}
