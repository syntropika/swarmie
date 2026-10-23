use std::{
    cell::{Cell as Flag, RefCell},
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    rc::Rc,
};

use swarmie_plugin_contracts::*;
use tembra::{core, prelude::*};
use tembra_executor_js::{Executor, ExecutorOptions};

use crate::{
    ManagerError, Result,
    store::{CachedPlugin, Store},
};

/// Native administrative owner for one fresh Tembra Host session.
///
/// Administration is a local Tembra service for trusted in-Host callers. The transport
/// embedding owns remote connection authority; this service exposes no remote administration.
/// Administrative domain access follows the actual object lifecycle; a shared physical SQLite
/// owner can outlive this domain and is never part of ordinary plugin invocation.
pub struct PluginManager {
    pub(crate) host: Host,
    pub(crate) database: DatabaseSource,
    pub(crate) store: RefCell<Option<core::WeakResource<Store>>>,
    pub(crate) executor: Executor,
    pub(crate) busy: Flag<bool>,
    pub(crate) fresh_session: Flag<bool>,
    pub(crate) restart_required: Flag<bool>,
    pub(crate) reconciling: Flag<bool>,
    pub(crate) composition_selection: RefCell<Option<BTreeSet<String>>>,
    pub(crate) composition_staged: RefCell<BTreeSet<String>>,
    // Real Tembra claims retained only for availability observation, never invocation dispatch.
    pub(crate) references: RefCell<BTreeMap<String, Vec<core::Reference>>>,
    pub(crate) reference_failures: RefCell<BTreeMap<String, BTreeMap<String, String>>>,
    pub(crate) core_facets: RefCell<BTreeMap<String, BTreeSet<String>>>,
    pub(crate) trusted_sources: RefCell<BTreeSet<String>>,
    pub(crate) cores: RefCell<Vec<PluginRecord>>,
    pub(crate) uncertain: RefCell<BTreeMap<String, PluginRecord>>,
}

#[derive(Clone)]
pub(crate) enum DatabaseSource {
    Standalone(PathBuf),
    Shared(swarmie_sqlite_local::DatabaseHandle),
}

pub(crate) struct OperationGuard<'a>(&'a Flag<bool>);
impl Drop for OperationGuard<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

impl PluginManager {
    /// Observes session uncertainty without changing admission or inferring object readiness.
    pub fn requires_recovery(&self) -> bool {
        self.restart_required.get()
            || !self.uncertain.borrow().is_empty()
            || self
                .storage()
                .and_then(|store| store.operations())
                .map_or(true, |operations| {
                    operations.iter().any(|operation| {
                        matches!(
                            operation.status,
                            OperationStatus::Pending | OperationStatus::Running
                        )
                    })
                })
    }

    /// Installs the compiled administrative Core Plugin and opens its SQLite Resource.
    ///
    /// The Host must be a fresh session whose dynamic facets have not been installed by a
    /// different supervisor. Restart recovery is explicit through [`Self::recover`]. The
    /// retained Driver must be the Driver that owns this Host.
    ///
    /// # Errors
    /// Refuses database failures, executor admission or Tembra startup
    /// failures. Opening an existing database preserves cached package versions and evidence.
    pub async fn start(
        host: Host,
        driver: Rc<dyn core::Driver>,
        database: impl Into<PathBuf>,
    ) -> Result<Rc<Self>> {
        let executor = Executor::new(driver, ExecutorOptions::default())?;
        Self::start_with_executor(host, executor, database).await
    }

    /// Starts administration with the embedding's configured JavaScript executor.
    ///
    /// The embedding registers capability-owned language adapters before activation. This
    /// manager knows neither their Contracts nor the Registries they use. The executor's
    /// retained Driver must own this fresh Host, as required by [`Self::start`].
    ///
    /// # Errors
    /// Refuses database and Tembra startup failures.
    pub async fn start_with_executor(
        host: Host,
        executor: Executor,
        database: impl Into<PathBuf>,
    ) -> Result<Rc<Self>> {
        Self::start_with_source(host, executor, DatabaseSource::Standalone(database.into())).await
    }

    /// Starts administration over an already declared physical SQLite owner.
    ///
    /// Register [`crate::sqlite_schema`] with that owner and await its startup first. The
    /// administrative Resource depends on its actual Tembra Resource; closing this domain
    /// revokes administration without closing other domains or erasing retained evidence.
    /// This constructor installs no application plugins and adds no manager invocation to
    /// their ordinary capability calls.
    ///
    /// # Errors
    /// Missing owner readiness, incompatible domain schema and Tembra startup errors refuse
    /// initialization. A failed administrative recovery can follow earlier durable changes.
    pub async fn start_with_database(
        host: Host,
        executor: Executor,
        database: swarmie_sqlite_local::DatabaseHandle,
    ) -> Result<Rc<Self>> {
        database.database()?;
        Self::start_with_source(host, executor, DatabaseSource::Shared(database)).await
    }

    async fn start_with_source(
        host: Host,
        executor: Executor,
        database: DatabaseSource,
    ) -> Result<Rc<Self>> {
        let manager = Rc::new(Self {
            host: host.clone(),

            database,
            store: RefCell::new(None),
            executor,
            busy: Flag::new(false),
            fresh_session: Flag::new(true),
            restart_required: Flag::new(false),
            reconciling: Flag::new(false),
            composition_selection: RefCell::new(None),
            composition_staged: RefCell::new(BTreeSet::new()),
            references: RefCell::new(BTreeMap::new()),
            reference_failures: RefCell::new(BTreeMap::new()),
            core_facets: RefCell::new(BTreeMap::from([(
                "swarmie.plugin-manager".into(),
                BTreeSet::from(["swarmie.plugin-manager".into()]),
            )])),
            trusted_sources: RefCell::new(BTreeSet::new()),
            uncertain: RefCell::new(BTreeMap::new()),
            cores: RefCell::new(vec![PluginRecord {
                plugin_id: "swarmie.plugin-manager".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                provider_id: "compiled".into(),
                kind: PluginKind::Core,
                desired: DesiredState::Active,
                observed: ObservedState::Active,
                errors: vec![],
            }]),
        });
        host.install(
            crate::service::facet(Rc::downgrade(&manager))?,
            core::LoadOptions::default(),
        )?
        .await?;
        for mut operation in manager.storage()?.operations()? {
            if matches!(
                operation.status,
                OperationStatus::Pending | OperationStatus::Running
            ) {
                operation.status = OperationStatus::Interrupted;
                operation.error = "previous supervisor session ended before settlement was recorded; external effects are unknown".into();
                manager.storage()?.operation(&operation)?;
            }
        }
        for mut plugin in manager.storage()?.all()? {
            if plugin.desired == DesiredState::Active {
                plugin.observed = ObservedState::Unavailable;
                plugin.activation_id.clear();
                plugin.error =
                    "runtime session restarted; activation has not been recovered".into();
            } else {
                // A fresh Host cannot retain executable work from an earlier supervisor.
                plugin.observed = ObservedState::Installed;
                plugin.activation_id.clear();
            }
            manager.storage()?.save(&plugin)?;
        }
        for mut session in manager.storage()?.browser_sessions()? {
            session.observed = BrowserObservation::Unknown;
            session.error =
                "supervisor session restarted; browser availability has not been observed".into();
            manager.storage()?.browser_session(&session)?;
        }
        Ok(manager)
    }

    pub(crate) fn guard(&self) -> Result<OperationGuard<'_>> {
        if self.busy.get() {
            return Err(ManagerError::Busy);
        }
        if self.storage()?.operations()?.iter().any(|operation| {
            matches!(
                operation.status,
                OperationStatus::Pending | OperationStatus::Running
            )
        }) {
            return Err(ManagerError::RecoveryRequired);
        }
        self.exclusive_guard()
    }

    pub(crate) fn recovery_guard(&self) -> Result<OperationGuard<'_>> {
        self.require_composition_reconciliation()?;
        if !self.fresh_session.replace(false) {
            return Err(ManagerError::RecoveryRequired);
        }
        self.guard()
    }

    fn exclusive_guard(&self) -> Result<OperationGuard<'_>> {
        if self.restart_required.get() || !self.uncertain.borrow().is_empty() {
            return Err(ManagerError::RecoveryRequired);
        }
        if self.busy.replace(true) {
            return Err(ManagerError::Busy);
        }
        Ok(OperationGuard(&self.busy))
    }

    pub(crate) fn storage(&self) -> Result<Rc<Store>> {
        Ok(self
            .store
            .borrow()
            .as_ref()
            .ok_or(ManagerError::Closed)?
            .upgrade()?
            .value()?)
    }

    pub(crate) fn publication_unrecorded(
        &self,
        package: &AcquiredPackage,
        desired: DesiredState,
        cause: ManagerError,
    ) -> ManagerError {
        let error = ManagerError::PublicationUnrecorded {
            plugin_id: package.manifest.id.clone(),
            version: package.manifest.version.clone(),
            cause: Box::new(cause),
        };
        self.uncertain.borrow_mut().insert(
            package.manifest.id.clone(),
            PluginRecord {
                plugin_id: package.manifest.id.clone(),
                version: package.manifest.version.clone(),
                provider_id: package.provenance.provider_id.clone(),
                kind: PluginKind::Dynamic,
                desired,
                observed: ObservedState::Unavailable,
                errors: vec![error.to_string()],
            },
        );
        error
    }

    pub(crate) fn failure_unrecorded(
        &self,
        failure: ManagerError,
        recording: ManagerError,
    ) -> ManagerError {
        self.restart_required.set(true);
        ManagerError::FailureUnrecorded {
            failure: Box::new(failure),
            recording: Box::new(recording),
        }
    }

    pub(crate) fn dynamic(&self, id: &str) -> Result<CachedPlugin> {
        if self.cores.borrow().iter().any(|core| core.plugin_id == id) {
            return Err(ManagerError::CorePlugin(id.into()));
        }
        self.storage()?
            .load(id)?
            .ok_or_else(|| ManagerError::NotInstalled(id.into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use futures::FutureExt;
    use serde_json::json;
    use tembra_executor_js::{BundledJsArtifact, NativeValue, ScopedCodec};
    use tembra_hooks::{
        HookDescriptor, HookManager, HookManagerOptions, HookType, RustHookDefinition, Strategy,
    };

    struct Integer;

    impl ScopedCodec<u32> for Integer {
        fn encode(&self, value: u32) -> tembra::Result<NativeValue> {
            Ok(NativeValue::portable(json!(value)))
        }

        fn decode(&self, value: NativeValue) -> tembra::Result<u32> {
            serde_json::from_value(value.into_portable()?)
                .map_err(|error| RuntimeError::invalid("embedding_integer", error.to_string()))
        }
    }

    #[test]
    fn injected_executor_preserves_unrelated_typed_hook_adapters() {
        let driver = tembra::native::TokioDriver::new();
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(driver.run_until(async {
                let directory = tempfile::tempdir().unwrap();
                let host = Host::with_driver(RUNTIME_HOST, "test", driver.clone()).unwrap();
                let profile = core::contracts::ValueSchema::Portable(
                    json!({"type": "integer", "minimum": 0}),
                );
                let descriptor = HookDescriptor::new(
                    "embedding.prepare",
                    1,
                    Strategy::Waterfall,
                    Some(profile.clone()),
                    Some(profile),
                    None,
                )
                .unwrap();
                let input =
                    HookType::<u32>::serde(json!({"type": "integer", "minimum": 0})).unwrap();
                let definition =
                    RustHookDefinition::from_descriptor(descriptor.clone(), input.clone(), input)
                        .unwrap();
                let executor = Executor::new(driver.clone(), ExecutorOptions::default()).unwrap();
                executor
                    .register_hook(&descriptor, Rc::new(Integer), Rc::new(Integer))
                    .unwrap();
                let manager = PluginManager::start_with_executor(
                    host.clone(),
                    executor,
                    directory.path().join("admin.sqlite"),
                )
                .await
                .unwrap();

                let token = core::contracts::Contract::local(
                    "embedding.prepare-contributor",
                    1,
                    json!({}),
                    BTreeMap::from([("prepare".into(), descriptor.contribution_member())]),
                )
                .unwrap();
                let source = format!(
                    r#"
                    const token = {token}, registry = {registry}, metadata = {metadata};
                    export default {{ name: 'embedding.adapter', uses: [], setup(_scope, d) {{
                        d.serve(token, (_input, self) => {{
                            self.registerMethod(registry, 'prepare', metadata, 'prepare');
                            return {{ state: {{}}, methods: {{ async prepare(_ctx, args) {{
                                return args.input + 7;
                            }} }} }};
                        }}, {{ lifetime: 'host' }});
                    }} }};
                "#,
                    token = serde_json::to_string(token.descriptor()).unwrap(),
                    registry = json!({"name": descriptor.registry().name(), "conflict": "error"}),
                    metadata =
                        json!({"hook": descriptor.fingerprint(), "priority": 0, "profile": 1}),
                );
                let executable = manager
                    .executor
                    .prepare(
                        host.core().id(),
                        BundledJsArtifact::new("embedding.adapter", source, vec!["default".into()])
                            .unwrap(),
                        "default".into(),
                    )
                    .await
                    .unwrap();
                host.core()
                    .install_executable(executable, core::LoadOptions::default())
                    .unwrap()
                    .await
                    .unwrap();
                let captured = Rc::new(RefCell::new(None));
                let selected = captured.clone();
                host.core()
                    .install(
                        core::NativeFacet::new("embedding.runner", vec![], move |scope, _| {
                            *selected.borrow_mut() = Some(scope.clone());
                            Ok(())
                        })
                        .unwrap(),
                        core::LoadOptions::default(),
                    )
                    .unwrap()
                    .await
                    .unwrap();
                let hooks = HookManager::new(
                    captured.borrow().clone().unwrap(),
                    driver.clone(),
                    HookManagerOptions::default(),
                )
                .unwrap();
                let output = host
                    .core()
                    .root(move |context| {
                        async move { hooks.run_dynamic_waterfall(&definition, 5, &context).await }
                            .boxed_local()
                    })
                    .await
                    .unwrap();
                assert_eq!(output, 12);
                host.close().await.unwrap();
            }));
    }
}
