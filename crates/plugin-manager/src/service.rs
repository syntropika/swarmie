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

    async fn inventory(&self, _context: Context) -> Result<Inventory> {
        self.manager()?
            .inventory()
            .map_err(ManagerError::into_runtime)
    }
    async fn active_package(
        &self,
        request: ActivePackageRequest,
        _context: Context,
    ) -> Result<ActivePackage> {
        self.manager()?
            .active_package(&request.plugin_id)
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

struct BrowserAdministration {
    manager: Weak<PluginManager>,
}
impl BrowserAdministration {
    fn manager(&self) -> Result<Rc<PluginManager>> {
        self.manager
            .upgrade()
            .ok_or_else(|| RuntimeError::invalid("supervisor", "supervisor closed"))
    }
}

impl BrowserAccessProvider for BrowserAdministration {
    async fn inventory(
        &self,
        request: BrowserInventoryRequest,
        _context: Context,
    ) -> Result<BrowserInventory> {
        self.manager()?
            .browser_inventory(&request.session_id)
            .map_err(ManagerError::into_runtime)
    }
    async fn active_package(
        &self,
        request: ActivePackageRequest,
        _context: Context,
    ) -> Result<ActivePackage> {
        self.manager()?
            .active_browser_package(&request.plugin_id)
            .map_err(ManagerError::into_runtime)
    }
    async fn read_package_file(
        &self,
        request: ReadPackageFileRequest,
        _context: Context,
    ) -> Result<ReadPackageFileResult> {
        self.manager()?
            .read_browser_package_file(request)
            .map_err(ManagerError::into_runtime)
    }
    async fn report_browser(
        &self,
        request: ReportBrowserRequest,
        _context: Context,
    ) -> Result<BrowserInventory> {
        self.manager()?
            .browser_report(request)
            .map_err(ManagerError::into_runtime)
    }
}

impl RemoteBrowserAccessProvider for BrowserAdministration {
    async fn inventory(&self, _context: Context) -> Result<Vec<ActivePackage>> {
        self.manager()?
            .active_browser_packages()
            .map_err(ManagerError::into_runtime)
    }
    async fn active_package(
        &self,
        request: ActivePackageRequest,
        _context: Context,
    ) -> Result<ActivePackage> {
        self.manager()?
            .active_browser_package(&request.plugin_id)
            .map_err(ManagerError::into_runtime)
    }
    async fn read_package_file(
        &self,
        request: ReadPackageFileRequest,
        _context: Context,
    ) -> Result<ReadPackageFileResult> {
        self.manager()?
            .read_browser_package_file(request)
            .map_err(ManagerError::into_runtime)
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
                let mut domain = match manager.database.clone() {
                    crate::manager::DatabaseSource::Standalone(database) => ResourceDeclaration::new(move |_| {
                        let database = database.clone();
                        async move { Store::open(&database).map_err(ManagerError::into_runtime) }.boxed_local()
                    }),
                    crate::manager::DatabaseSource::Shared(handle) => {
                        let database = handle.declare(declaration)?;
                        let opening = database.clone();
                        let mut domain = ResourceDeclaration::new(move |_| {
                            let database = opening.value().map(|database| (*database).clone());
                            async move { Store::from_database(database?).map_err(ManagerError::into_runtime) }.boxed_local()
                        });
                        domain.deps.push(database.dependency());
                        domain
                    }
                };
                domain = domain.with_close(|store| {
                    async move { store.close().map_err(ManagerError::into_runtime) }.boxed_local()
                });
                let resource = declaration.core_mut().resource("administration-sqlite", domain)?;
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
        let browser_owner = manager.clone();
        setup.serve(BrowserAccess, move |_, _, _| {
            Ok(BrowserAdministration {
                manager: browser_owner.clone(),
            })
        })?;
        let remote_owner = manager.clone();
        setup.serve(RemoteBrowserAccess, move |_, _, _| {
            Ok(BrowserAdministration {
                manager: remote_owner.clone(),
            })
        })
    })
}
