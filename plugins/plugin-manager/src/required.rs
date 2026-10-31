//! Host-owned startup requirements over the existing atomic plugin inventory.

use crate::manager::storage_error;
use crate::{Catalog, CommitOutcome, InstalledPlugin, Inventory, PluginChange, PluginStorage};
use std::rc::Rc;
use tembra::{Code, Error, ResourceSignal};

pub(crate) fn unavailable(message: &str) -> Error {
    Error::new(
        Code::Unavailable,
        "swarmie.plugin_manager.required",
        message,
    )
}

pub(crate) fn validate(catalog: &Catalog, inventory: &Inventory) -> tembra::Result<()> {
    for id in &catalog.2 {
        if !inventory.plugins.iter().any(|record| {
            &record.id == id && record.enabled && record.version == catalog.0[id].version
        }) {
            return Err(unavailable(
                "required plugin inventory is missing, disabled or incompatible",
            ));
        }
    }
    Ok(())
}

pub(crate) async fn initialize(
    catalog: &Catalog,
    storage: &Rc<dyn PluginStorage>,
    inventory: &mut Inventory,
    signal: &ResourceSignal,
) -> tembra::Result<()> {
    if inventory.revision == 0 && inventory.plugins.is_empty() && !catalog.2.is_empty() {
        signal.check()?;
        let plugins: Vec<_> = catalog
            .2
            .iter()
            .map(|id| InstalledPlugin {
                id: id.clone(),
                version: catalog.0[id].version,
                enabled: true,
            })
            .collect();
        match storage
            .commit(0, plugins.iter().cloned().map(PluginChange::Put).collect())
            .await
            .map_err(storage_error)?
        {
            CommitOutcome::Applied { revision } => {
                inventory.revision = revision;
                inventory.plugins = plugins;
            }
            CommitOutcome::Rejected { .. } => {
                return Err(unavailable(
                    "plugin inventory changed during initial startup",
                ));
            }
            CommitOutcome::Unknown { .. } => {
                storage.owner().require_recovery();
                return Err(unavailable("initial plugin inventory requires recovery"));
            }
        }
    }
    validate(catalog, inventory)
}
