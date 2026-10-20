use std::rc::{Rc, Weak};

use futures::FutureExt;
use swarmie_plugin_contracts::*;
use tembra::{core, prelude::*};

use crate::{ManagerError, PluginManager, store::Store};

struct Administration {
    manager: Weak<PluginManager>,
    _database: Resource<Store>,
}

impl Administration {
    fn manager(&self) -> Result<Rc<PluginManager>> {
        self.manager.upgrade().ok_or_else(|| {
            RuntimeError::new(
                core::ErrorCode::Gone,
                "supervisor",
                "administrative supervisor is no longer retained",
            )
        })
    }
}

impl PluginAdministrationProvider for Administration {
    async fn install(&self, request: InstallRequest, _context: Context) -> Result<Inventory> {
        self.manager()?
            .install(request)
            .await
            .map_err(ManagerError::into_runtime)
    }

    async fn activate(&self, request: ActivateRequest, _context: Context) -> Result<Inventory> {
        self.manager()?
            .activate(request)
            .await
            .map_err(ManagerError::into_runtime)
    }

    async fn deactivate(&self, request: DeactivateRequest, _context: Context) -> Result<Inventory> {
        self.manager()?
            .deactivate(request)
            .await
            .map_err(ManagerError::into_runtime)
    }

    async fn update(&self, request: UpdateRequest, _context: Context) -> Result<Inventory> {
        self.manager()?
            .update(request)
            .await
            .map_err(ManagerError::into_runtime)
    }

    async fn uninstall(&self, request: UninstallRequest, _context: Context) -> Result<Inventory> {
        self.manager()?
            .uninstall(request)
            .await
            .map_err(ManagerError::into_runtime)
    }

    async fn configure_provider(
        &self,
        request: ConfigureProviderRequest,
        _context: Context,
    ) -> Result<Inventory> {
        self.manager()?
            .configure_provider(request)
            .await
            .map_err(ManagerError::into_runtime)
    }

    async fn inventory(&self, request: InventoryRequest, _context: Context) -> Result<Inventory> {
        self.manager()?
            .inventory(&request.proof)
            .map_err(ManagerError::into_runtime)
    }
    async fn active_package(
        &self,
        request: ActivePackageRequest,
        _context: Context,
    ) -> Result<ActivePackage> {
        self.manager()?
            .active_package(&request.proof, &request.plugin_id)
            .map_err(ManagerError::into_runtime)
    }

    async fn read_package_file(
        &self,
        request: ReadPackageFileRequest,
        _context: Context,
    ) -> Result<ReadPackageFileResult> {
        self.manager()?
            .read_package_file(request)
            .map_err(ManagerError::into_runtime)
    }
    async fn report_browser(
        &self,
        request: ReportBrowserRequest,
        _context: Context,
    ) -> Result<Inventory> {
        self.manager()?
            .report_browser(request)
            .map_err(ManagerError::into_runtime)
    }
}

struct RemoteAdministration {
    local: PluginAdministrationRef,
}

impl RemotePluginAdministrationProvider for RemoteAdministration {
    async fn install(&self, request: InstallRequest, context: Context) -> Result<Inventory> {
        self.local.with_context(&context).install(request).await
    }

    async fn activate(&self, request: ActivateRequest, context: Context) -> Result<Inventory> {
        self.local.with_context(&context).activate(request).await
    }

    async fn deactivate(&self, request: DeactivateRequest, context: Context) -> Result<Inventory> {
        self.local.with_context(&context).deactivate(request).await
    }

    async fn update(&self, request: UpdateRequest, context: Context) -> Result<Inventory> {
        self.local.with_context(&context).update(request).await
    }

    async fn uninstall(&self, request: UninstallRequest, context: Context) -> Result<Inventory> {
        self.local.with_context(&context).uninstall(request).await
    }

    async fn configure_provider(
        &self,
        request: ConfigureProviderRequest,
        context: Context,
    ) -> Result<Inventory> {
        self.local
            .with_context(&context)
            .configure_provider(request)
            .await
    }

    async fn inventory(&self, request: InventoryRequest, context: Context) -> Result<Inventory> {
        self.local.with_context(&context).inventory(request).await
    }
    async fn active_package(
        &self,
        request: ActivePackageRequest,
        context: Context,
    ) -> Result<ActivePackage> {
        self.local
            .with_context(&context)
            .active_package(request)
            .await
    }

    async fn read_package_file(
        &self,
        request: ReadPackageFileRequest,
        context: Context,
    ) -> Result<ReadPackageFileResult> {
        self.local
            .with_context(&context)
            .read_package_file(request)
            .await
    }
    async fn report_browser(
        &self,
        request: ReportBrowserRequest,
        context: Context,
    ) -> Result<Inventory> {
        self.local
            .with_context(&context)
            .report_browser(request)
            .await
    }
}

pub(crate) fn facet(manager: Weak<PluginManager>) -> Result<NativeFacet> {
    NativeFacet::new("swarmie.plugin-manager", vec![], move |_, setup| {
        let owner = manager.clone();
        setup.serve_with_options(
            PluginAdministration,
            move |_, _, declaration| {
                let manager = owner.upgrade().ok_or_else(|| {
                    RuntimeError::new(
                        core::ErrorCode::Gone,
                        "supervisor",
                        "administrative supervisor is no longer retained",
                    )
                })?;
                let database = manager.database.clone();
                let resource = declaration.core_mut().resource(
                    "administration-sqlite",
                    ResourceDeclaration::new(move |_| {
                        let database = database.clone();
                        async move { Store::open(&database).map_err(ManagerError::into_runtime) }
                            .boxed_local()
                    })
                    .with_close(|store| {
                        async move { store.close().map_err(ManagerError::into_runtime) }
                            .boxed_local()
                    }),
                )?;
                *manager.store.borrow_mut() = Some(resource.downgrade());
                Ok(Administration {
                    manager: owner.clone(),
                    _database: resource,
                })
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )?;
        setup.serve(RemotePluginAdministration, |_, _, declaration| {
            Ok(RemoteAdministration {
                local: declaration.get(PluginAdministration, PluginAdministrationInput {})?,
            })
        })
    })
}
