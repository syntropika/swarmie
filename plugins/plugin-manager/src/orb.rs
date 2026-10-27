use crate::{
    Catalog, InstalledPlugin, StorageFactory,
    manager::{PluginManager, Reason, storage_error},
};
use futures::FutureExt;
use tembra::{Code, Context, Error, Host, Orb, Service};

pub const SERVICE: &str = "swarmie.plugin_manager";

/// Tembra's reserved principal and key for host-lifetime services.
pub const ADMIN_PRINCIPAL: &str = "host";
pub const INSTANCE_KEY: &str = "main";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagerMethod {
    List,
    Configure,
    Refresh,
}

impl ManagerMethod {
    /// Returns the public method name used by Tembra.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Configure => "configure",
            Self::Refresh => "refresh",
        }
    }
}

#[derive(Clone, Copy)]
enum ResourceName {
    Storage,
    Startup,
}

impl ResourceName {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Storage => "storage",
            Self::Startup => "plugins.startup",
        }
    }
}

/// Declares the manager and its storage-dependent startup as managed resources.
///
/// The synchronous factory performs no I/O. Storage opens before startup and closes
/// after manager operations settle. Plugin installation failures are reported in the
/// index rather than aborting unrelated installations. Failure to read inventory is
/// a startup error; it must not be treated as an empty installation.
///
/// Native orbs install once per host. Disabling or replacing one persists intent
/// and sets `restart_required`; it never claims a live native orb was unloaded.
pub fn orb(host: Host, storage: StorageFactory, catalog: Catalog) -> Orb {
    let service = Service::new(SERVICE, move |declaration| {
        let address = declaration.address()?;
        if address.principal != ADMIN_PRINCIPAL {
            return Err(Error::new(
                Code::Forbidden,
                Reason::Forbidden.as_str(),
                "plugin administration requires the runtime principal",
            ));
        }
        if !address.key.iter().map(String::as_str).eq([INSTANCE_KEY]) {
            return Err(Error::new(
                Code::Invalid,
                Reason::Address.as_str(),
                "the manager has one fixed host instance key",
            ));
        }
        let storage = storage.clone();
        let stored = declaration.resource_async(
            ResourceName::Storage.as_str(),
            1,
            &[],
            move |_| async move { storage().await.map_err(storage_error) }.boxed_local(),
            |storage, _| async move { storage.close().await.map_err(storage_error) }.boxed_local(),
        )?;
        let (host, catalog) = (host.clone(), catalog.clone());
        declaration.resource_async(
            ResourceName::Startup.as_str(),
            1,
            &[stored.key()],
            move |signal| {
                async move {
                    PluginManager::open(host, catalog, stored.value()?.as_ref().clone(), signal)
                        .await
                }
                .boxed_local()
            },
            |manager, _| {
                async move {
                    manager.close();
                    Ok(())
                }
                .boxed_local()
            },
        )
    })
    .lifetime(tembra::core::Lifetime::Host, 0)
    .method(
        ManagerMethod::List.as_str(),
        |manager, context, (): ()| async move {
            authorize(&context)?;
            Ok(manager.value()?.snapshot())
        },
    )
    .method(
        ManagerMethod::Configure.as_str(),
        |manager, context, (plugin,): (InstalledPlugin,)| async move {
            authorize(&context)?;
            manager.value()?.configure(plugin).await
        },
    )
    .method(
        ManagerMethod::Refresh.as_str(),
        |manager, context, (): ()| async move {
            authorize(&context)?;
            manager.value()?.refresh().await
        },
    );
    Orb::new(SERVICE, 1).service(service)
}

fn authorize(context: &Context) -> tembra::Result<()> {
    context.check()?;
    if context.principal()? != ADMIN_PRINCIPAL {
        return Err(Error::new(
            Code::Forbidden,
            Reason::Forbidden.as_str(),
            "plugin administration requires the runtime principal",
        ));
    }
    Ok(())
}
