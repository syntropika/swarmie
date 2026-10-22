//! Compiled capabilities own language adaptation before genuine Dynamic activation and recovery.

use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    future::Future,
    path::Path,
    rc::Rc,
    time::Duration,
};

use futures::FutureExt;
use serde_json::json;
use sha2::{Digest, Sha256};
use swarmie_composition::{Composition, FacetIdentity, PluginKind, Profile, Release, Selection};
use swarmie_plugin_contracts::{
    Compatibility, Distribution, FacetFormat, FacetTarget, ManifestFile, ObservedState,
    PluginFacet, PluginManifest, TEMBRA_REVISION,
};
use swarmie_plugin_manager::LocalSource;
use swarmie_runtime::{Application, CompiledPlugin, Configuration, Runtime, start_with_catalog};
use tembra::{NativeFacet, Result, RuntimeError, core};
use tembra_executor_js::{BundledJsArtifact, NativeValue, ScopedCodec};
use tembra_hooks::{
    HookDescriptor, HookManager, HookManagerOptions, HookType, RustHookDefinition, Strategy,
};
use tembra_native::TokioDriver;

const OBSERVER: &str = "fixture.language-observer";
const CAPABILITY: &str = "fixture.language-capability";
const RUNNER: &str = "fixture.language-runner";
const SOURCE: &str = "swarmie.local-plugin-source";

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
        .block_on(driver.run_until(async {
            tokio::time::timeout(Duration::from_secs(20), test(driver.clone()))
                .await
                .expect("language adapter acceptance timed out");
        }));
}

fn hook() -> Result<RustHookDefinition<u32, u32>> {
    let schema = json!({"type":"integer", "minimum":0, "maximum":4_294_967_295_u64});
    let value = HookType::serde(schema)?;
    let descriptor = HookDescriptor::new(
        "fixture.prepare-number",
        1,
        Strategy::Waterfall,
        value.profile().clone(),
        value.profile().clone(),
        None,
    )?;
    RustHookDefinition::from_descriptor(descriptor, value.clone(), value)
}

struct UnsignedInteger;

impl ScopedCodec<u32> for UnsignedInteger {
    fn encode(&self, value: u32) -> Result<NativeValue> {
        Ok(NativeValue::portable(json!(value)))
    }

    fn decode(&self, value: NativeValue) -> Result<u32> {
        serde_json::from_value(value.into_portable()?)
            .map_err(|error| RuntimeError::invalid("fixture_integer", error.to_string()))
    }
}

fn package(source: &Path) -> Vec<u8> {
    let definition = hook().unwrap();
    let descriptor = definition.descriptor();
    let token = core::contracts::Contract::local(
        "fixture.number-contributor",
        1,
        json!({}),
        BTreeMap::from([("prepare".into(), descriptor.contribution_member())]),
    )
    .unwrap();
    let code = format!(
        r#"
        const token = {token}, registry = {registry}, metadata = {metadata};
        export default {{ name: '{OBSERVER}', uses: [], setup(_scope, d) {{
            d.serve(token, (_input, self) => {{
                self.registerMethod(registry, 'number', metadata, 'prepare');
                return {{ state: {{}}, methods: {{ async prepare(_context, args) {{
                    return args.input + 9;
                }} }} }};
            }}, {{ lifetime: 'host' }});
        }} }};
        "#,
        token = serde_json::to_string(token.descriptor()).unwrap(),
        registry = json!({"name":descriptor.registry().name(), "conflict":"error"}),
        metadata = json!({"hook":descriptor.fingerprint(), "priority":0, "profile":1}),
    );
    let artifact = BundledJsArtifact::new(OBSERVER, code.clone(), vec!["default".into()]).unwrap();
    let bytes = serde_json::to_vec(&json!({
        "format":"tembra-esm-v1", "identity":artifact.identity(), "source":code,
        "sha256":artifact.sha256(), "exports":["default"],
    }))
    .unwrap();
    let sha256 = hex::encode(Sha256::digest(&bytes));
    let manifest = PluginManifest {
        schema_version: 1,
        id: OBSERVER.into(),
        version: "1.0.0".into(),
        compatibility: Compatibility {
            swarmie: "^0.1.0".into(),
            tembra_revision: TEMBRA_REVISION.into(),
        },
        dependencies: vec![],
        facets: vec![PluginFacet {
            id: OBSERVER.into(),
            target: FacetTarget::Server,
            format: FacetFormat::Bundled,
            artifact_path: "artifact.json".into(),
            sha256: sha256.clone(),
            selected_export: "default".into(),
        }],
        files: vec![ManifestFile {
            path: "artifact.json".into(),
            sha256,
        }],
        distribution: Distribution {
            provider: "local".into(),
            description: "Language adapter acceptance".into(),
        },
        ui: vec![],
    };
    std::fs::create_dir_all(source).unwrap();
    std::fs::write(source.join("artifact.json"), &bytes).unwrap();
    std::fs::write(
        source.join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    bytes
}

fn release(id: &str, kind: PluginKind) -> Release {
    Release {
        plugin: id.into(),
        version: "1.0.0".into(),
        kind,
        facets: vec![FacetIdentity {
            id: id.into(),
            target: FacetTarget::Server,
        }],
    }
}

fn selection(id: &str, dynamic: bool) -> Selection {
    Selection {
        id: id.into(),
        plugin: id.into(),
        version: "1.0.0".into(),
        provider: dynamic.then(|| "local".into()),
        config: json!({}),
    }
}

fn application() -> Application {
    Application {
        composition: Composition {
            schema_version: 1,
            profile: Profile {
                id: "language-adapter-acceptance".into(),
                bundles: vec![],
                plugins: vec![
                    selection(SOURCE, false),
                    selection(RUNNER, false),
                    selection(OBSERVER, true),
                ],
                overrides: vec![],
                disable: vec![],
            },
            bundles: BTreeMap::new(),
        },
        releases: vec![release(OBSERVER, PluginKind::Dynamic)],
    }
}

#[derive(Default)]
struct Observations {
    initialized: Cell<u32>,
    source_setups: Cell<u32>,
    runner_setups: Cell<u32>,
    capability_setups: Cell<u32>,
    scope: RefCell<Option<core::FacetScope>>,
}

fn catalog(source: &Path, observations: Rc<Observations>, refuse: bool) -> Vec<CompiledPlugin> {
    let directory = source.to_path_buf();
    let runner = observations.clone();
    let source_observations = observations.clone();
    let capability = observations.clone();
    let adapter = observations;
    let mut source_release = release(SOURCE, PluginKind::Core);
    source_release.facets.push(FacetIdentity {
        id: "fixture.source-adapter-marker".into(),
        target: FacetTarget::Server,
    });
    vec![
        CompiledPlugin::new(source_release, move |_| {
            let observations = source_observations.clone();
            Ok(vec![
                LocalSource::new("local", directory.clone()).facet()?,
                NativeFacet::new("fixture.source-adapter-marker", vec![], move |_, _| {
                    assert!(observations.initialized.get() > 0);
                    observations
                        .source_setups
                        .set(observations.source_setups.get() + 1);
                    Ok(())
                })?,
            ])
        })
        .with_trusted_source("local".into()),
        CompiledPlugin::new(release(RUNNER, PluginKind::Core), move |_| {
            let observations = runner.clone();
            Ok(vec![NativeFacet::new(RUNNER, vec![], move |scope, _| {
                assert!(
                    observations.initialized.get() > 0,
                    "adapter must initialize before Facet setup"
                );
                observations
                    .runner_setups
                    .set(observations.runner_setups.get() + 1);
                *observations.scope.borrow_mut() = Some(scope.clone());
                Ok(())
            })?])
        }),
        CompiledPlugin::new(release(CAPABILITY, PluginKind::Core), move |_| {
            let observations = capability.clone();
            Ok(vec![NativeFacet::new(CAPABILITY, vec![], move |_, _| {
                assert!(observations.initialized.get() > 0);
                observations
                    .capability_setups
                    .set(observations.capability_setups.get() + 1);
                Ok(())
            })?])
        })
        .with_language_adapter(move |executor| {
            adapter.initialized.set(adapter.initialized.get() + 1);
            if refuse {
                return Err(RuntimeError::invalid(
                    "fixture_adapter_refused",
                    "language initialization refused",
                ));
            }
            executor.register_hook(
                hook()?.descriptor(),
                Rc::new(UnsignedInteger),
                Rc::new(UnsignedInteger),
            )
        }),
    ]
}

async fn invoke(
    runtime: &Runtime,
    driver: Rc<TokioDriver>,
    observations: &Observations,
    input: u32,
) -> u32 {
    let scope = observations.scope.borrow().clone().unwrap();
    let manager = HookManager::new(scope, driver, HookManagerOptions::default()).unwrap();
    let definition = hook().unwrap();
    runtime
        .host
        .core()
        .root(move |context| {
            async move {
                manager
                    .run_dynamic_waterfall(&definition, input, &context)
                    .await
            }
            .boxed_local()
        })
        .await
        .unwrap()
}

#[test]
fn catalog_owned_adapter_precedes_dynamic_activation_and_source_free_cached_recovery() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        package(&source);
        let observations = Rc::new(Observations::default());
        let configuration = |application| Configuration {
            database: directory.path().join("admin.sqlite"),
            source: source.clone(),
            application,
        };
        let mut runtime = start_with_catalog(
            driver.clone(),
            configuration(Some(application())),
            catalog(&source, observations.clone(), false),
        )
        .await
        .unwrap();
        assert!(
            runtime.deployment_errors.is_empty(),
            "{:?}",
            runtime.deployment_errors
        );
        assert!(runtime.recovery_error.is_none());
        assert_eq!(observations.initialized.get(), 1);
        assert_eq!(observations.runner_setups.get(), 1);
        assert_eq!(observations.source_setups.get(), 1);
        assert_eq!(
            observations.capability_setups.get(),
            0,
            "unselected capability must only register language types"
        );
        assert_eq!(invoke(&runtime, driver.clone(), &observations, 5).await, 14);
        let mut enabled = runtime.desired_application().clone();
        enabled
            .composition
            .profile
            .plugins
            .push(selection(CAPABILITY, false));
        let report = runtime.apply_application(enabled).await.unwrap();
        assert_eq!(
            report.results[CAPABILITY].outcome,
            swarmie_runtime::TransitionOutcome::RestartRequired
        );
        assert_eq!(
            observations.initialized.get(),
            1,
            "Profile changes must not register codecs again"
        );
        assert_eq!(observations.capability_setups.get(), 0);
        assert_eq!(invoke(&runtime, driver.clone(), &observations, 2).await, 11);
        runtime.host.close().await.unwrap();
        drop(runtime);
        std::fs::remove_dir_all(&source).unwrap();
        let recovered = start_with_catalog(
            driver.clone(),
            configuration(None),
            catalog(&source, observations.clone(), false),
        )
        .await
        .unwrap();
        assert!(
            recovered.deployment_errors.is_empty(),
            "{:?}",
            recovered.deployment_errors
        );
        assert!(recovered.recovery_error.is_none());
        assert_eq!(
            observations.initialized.get(),
            2,
            "each fresh Host must initialize its executor exactly once"
        );
        assert_eq!(observations.capability_setups.get(), 1);
        assert_eq!(observations.source_setups.get(), 2);
        assert!(
            recovered
                .manager
                .inventory()
                .unwrap()
                .plugins
                .iter()
                .any(|plugin| plugin.plugin_id == OBSERVER
                    && plugin.observed == ObservedState::Active)
        );
        assert_eq!(invoke(&recovered, driver, &observations, 13).await, 22);
        recovered.host.close().await.unwrap();
    });
}

#[test]
fn adapter_rejection_prevents_application_setup_and_preserves_source_artifacts() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        let artifact = package(&source);
        let manifest = std::fs::read(source.join("manifest.json")).unwrap();
        let observations = Rc::new(Observations::default());
        let error = start_with_catalog(
            driver,
            Configuration {
                database: directory.path().join("admin.sqlite"),
                source: source.clone(),
                application: Some(application()),
            },
            catalog(&source, observations.clone(), true),
        )
        .await
        .err()
        .expect("adapter refusal must abort startup");
        assert!(
            error.to_string().contains("fixture_adapter_refused"),
            "{error}"
        );
        assert_eq!(observations.initialized.get(), 1);
        assert_eq!(observations.runner_setups.get(), 0);
        assert_eq!(observations.source_setups.get(), 0);
        assert_eq!(observations.capability_setups.get(), 0);
        assert_eq!(
            std::fs::read(source.join("artifact.json")).unwrap(),
            artifact
        );
        assert_eq!(
            std::fs::read(source.join("manifest.json")).unwrap(),
            manifest
        );
    });
}
