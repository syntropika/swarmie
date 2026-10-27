use futures::{FutureExt, task::LocalSpawn};
use std::rc::Rc;
use swarmie_plugin_manager::{
    ADMIN_PRINCIPAL, Catalog, INSTANCE_KEY, InstalledPlugin, ManagerMethod, PluginSnapshot,
    SERVICE, StorageFactory,
};
use tembra::{Host, Reference};

/// A host whose only bootstrap orb is PluginManager.
///
/// Callers own the local executor and must keep it running through [`Self::close`].
/// Plugin methods use Tembra references directly; this type supplies no second
/// object registry, task scheduler, event loop or transport engine.
pub struct Runtime {
    host: Host,
    manager: Rc<Reference>,
}

impl Runtime {
    /// Opens storage and recovers plugins through the manager's managed startup.
    ///
    /// # Errors
    /// Storage/startup failures close the partial host and return the startup error.
    /// Individual plugin mount failures remain inspectable through [`Self::plugins`].
    /// No plugin configuration is synthesized when storage cannot be read.
    ///
    /// # Examples
    ///
    /// ```
    /// use futures::executor::LocalPool;
    /// use swarmie_plugin_manager::Catalog;
    /// use swarmie_runtime::Runtime;
    /// use swarmie_storage::sqlite;
    ///
    /// let directory = tempfile::tempdir().unwrap();
    /// let mut executor = LocalPool::new();
    /// let spawner = executor.spawner();
    /// executor.run_until(async {
    ///     let runtime = Runtime::open(
    ///         spawner,
    ///         sqlite::factory(directory.path().join("runtime.sqlite")),
    ///         Catalog::default(),
    ///     ).await.unwrap();
    ///     assert!(runtime.plugins().await.unwrap().plugins.is_empty());
    ///     runtime.close().await.unwrap();
    /// });
    /// ```
    pub async fn open(
        executor: impl LocalSpawn + 'static,
        storage: StorageFactory,
        catalog: Catalog,
    ) -> tembra::Result<Self> {
        let host = Host::new(executor);
        host.install(vec![swarmie_plugin_manager::orb(
            host.clone(),
            storage,
            catalog,
        )])?;
        let acquired = host.get(SERVICE, vec![INSTANCE_KEY.into()], ADMIN_PRINCIPAL);
        let manager = match acquired {
            Ok(manager) => manager,
            Err(error) => {
                let _cleanup = host.close().await;
                return Err(error);
            }
        };
        if let Err(error) = manager.ready().await {
            drop(manager);
            let _cleanup = host.close().await;
            return Err(error);
        }
        Ok(Self {
            host,
            manager: Rc::new(manager),
        })
    }

    /// Gives compositions access to Tembra's normal managed references and calls.
    pub fn host(&self) -> &Host {
        &self.host
    }

    pub async fn plugins(&self) -> tembra::Result<PluginSnapshot> {
        self.call(ManagerMethod::List, ()).await
    }

    /// Persists desired state before installing any newly enabled compiled orbs.
    /// Disabling mounted native orbs requires a host restart, visible in the snapshot.
    pub async fn configure(&self, plugin: InstalledPlugin) -> tembra::Result<PluginSnapshot> {
        self.call(ManagerMethod::Configure, (plugin,)).await
    }

    /// Reloads known inventory after a competing writer; uncertain writes require recovery.
    pub async fn refresh(&self) -> tembra::Result<PluginSnapshot> {
        self.call(ManagerMethod::Refresh, ()).await
    }

    /// Signals shutdown and awaits physical invocations and resource cleanup.
    /// Dropping this future is not proof of shutdown; keep driving the executor.
    pub async fn close(self) -> tembra::Result<()> {
        let released = self.manager.release();
        let closed = self.host.close().await;
        released.and(closed)
    }

    async fn call<A: 'static, R: 'static>(
        &self,
        method: ManagerMethod,
        arguments: A,
    ) -> tembra::Result<R> {
        let manager = self.manager.clone();
        self.host
            .root(ADMIN_PRINCIPAL, move |context| {
                async move { manager.call(&context, method.as_str(), arguments).await }
                    .boxed_local()
            })
            .await
    }
}
