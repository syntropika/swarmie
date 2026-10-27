use super::{
    DefinitionStore, SaveDefinition, ServerAction,
    administration::{Administration, MANAGED_SERVICE},
};
use crate::{ServerBinding, invalid, mounted::Mounted};
use futures::FutureExt;
use std::rc::Rc;
use swarmie_tools::{Catalog, ToolProvider};
use tembra::{Host, Orb, Service};

pub const SETTINGS_SERVICE: &str = "swarmie.mcp.settings";
pub const SETTINGS_KEY: &str = "main";

/// Compose host-owned definitions and configured MCP providers. The selector must
/// return the same principal catalog used by the Tools Orb. Configuration never
/// connects by itself. Only the enable action or an admitted Connect opens I/O.
pub fn settings_orb(
    host: Host,
    store: Rc<dyn DefinitionStore>,
    catalog: impl Fn(&str) -> tembra::Result<Catalog> + 'static,
) -> Orb {
    let configured_store = store.clone();
    let configured = Service::new(MANAGED_SERVICE, move |declaration| {
        let address = declaration.address()?;
        let [id, revision] = address.key.as_slice() else {
            return Err(invalid("configured MCP key needs an identity and revision"));
        };
        let definition = configured_store
            .load(&address.principal)?
            .into_iter()
            .find(|entry| entry.id.as_str() == id && entry.revision.to_string() == *revision)
            .ok_or_else(|| invalid("MCP definition revision is unavailable"))?;
        definition.validate()?;
        let binding = ServerBinding {
            configuration: definition.configuration(),
            principal: address.principal.clone(),
            catalog: catalog(&address.principal)?,
        };
        let provider = ToolProvider::new(MANAGED_SERVICE, definition.key())?;
        let guard_store = configured_store.clone();
        let principal = address.principal.clone();
        let guard_id = id.clone();
        let guard_revision = definition.revision;
        let guard = Rc::new(move || {
            if guard_store.load(&principal)?.iter().any(|entry| {
                entry.id.as_str() == guard_id && entry.revision == guard_revision && entry.enabled
            }) {
                Ok(())
            } else {
                Err(invalid(
                    "MCP connection is disabled or its definition changed",
                ))
            }
        });
        declaration.resource(
            "mcp.connection",
            1,
            move || Ok(Mounted::bound(binding, provider, Some(guard))),
            |mounted, _| async move { mounted.close().await }.boxed_local(),
        )
    });
    let administration =
        Service::new(SETTINGS_SERVICE, move |declaration| {
            let address = declaration.address()?;
            if address.key != [SETTINGS_KEY] {
                return Err(invalid("MCP settings uses the main key"));
            }
            Ok(Administration::new(
                host.clone(),
                store.clone(),
                address.principal,
            ))
        })
        .method("list", |admin, context, (): ()| async move {
            admin.list(&context).await
        })
        .method(
            "save",
            |admin, context, (input,): (SaveDefinition,)| async move {
                admin.save(&context, input).await
            },
        )
        .method(
            "enable",
            |admin, context, (input,): (ServerAction,)| async move {
                admin.set_enabled(&context, input, true).await
            },
        )
        .method(
            "disable",
            |admin, context, (input,): (ServerAction,)| async move {
                admin.set_enabled(&context, input, false).await
            },
        )
        .method(
            "remove",
            |admin, context, (input,): (ServerAction,)| async move {
                admin.remove(&context, input).await
            },
        );
    Orb::new(SETTINGS_SERVICE, 1)
        .service(administration)
        .service(crate::orb::connection_methods(configured))
}
