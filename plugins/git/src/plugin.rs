//! Optional administrative metadata for the compiled Git Orb.

use std::num::NonZeroU32;

use swarmie_plugin_manager::{CorePlugin, PluginId};

use crate::GitService;

pub const PLUGIN_ID: &str = "core.git";

/// Make Git available to PluginManager without putting it in the invocation path.
///
/// The embedding supplies the same authorized service used by direct composition.
/// When Tools support is enabled, its metadata follows active plugin membership;
/// actual execution still calls the ordinary Git service and rechecks authority.
pub fn core_plugin(service: GitService) -> tembra::Result<CorePlugin> {
    let id = PluginId::new(PLUGIN_ID)
        .map_err(|error| tembra::Error::invalid("swarmie.git.plugin", error.to_string()))?;
    let plugin = CorePlugin::new(id, NonZeroU32::MIN, move |_| {
        Ok(vec![crate::orb(service.clone())])
    });
    #[cfg(feature = "tools")]
    let plugin = crate::tool_definitions()?
        .into_iter()
        .fold(plugin, CorePlugin::contributes);
    Ok(plugin)
}
