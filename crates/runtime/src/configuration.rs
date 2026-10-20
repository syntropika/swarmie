use std::{collections::BTreeMap, rc::Rc};

use serde_json::Value;
use swarmie_composition::{EffectiveComposition, InstanceState};
use swarmie_plugin_contracts::{PluginConfiguration, PluginConfigurationProvider};
use tembra::{Context, NativeFacet, Result, RuntimeError};

pub(crate) const PLUGIN_ID: &str = "swarmie.application-configuration";
pub(crate) const FACET_ID: &str = "swarmie.application-configuration.runtime";

/// Immutable input delivery through an ordinary Tembra object. Persistence remains in the
/// desired application document; this object neither writes State nor manages plugin lifetime.
pub(crate) fn facet(effective: &EffectiveComposition) -> Result<NativeFacet> {
    let values = Rc::new(
        effective
            .instances
            .values()
            .filter(|instance| instance.state == InstanceState::Ready)
            .map(|instance| {
                (
                    instance.selection.plugin.clone(),
                    instance.selection.config.clone(),
                )
            })
            .collect::<BTreeMap<_, _>>(),
    );
    NativeFacet::new(FACET_ID, vec![], move |_, setup| {
        let values = values.clone();
        setup.serve(PluginConfiguration, move |input, _, _| {
            let value = values.get(&input.plugin_id).ok_or_else(|| {
                RuntimeError::invalid(
                    "plugin_configuration",
                    "plugin is not selected by the current application",
                )
            })?;
            Ok(Configuration {
                value: value.clone(),
            })
        })
    })
}

struct Configuration {
    value: Value,
}

impl PluginConfigurationProvider for Configuration {
    async fn read(&self, _context: Context) -> Result<Value> {
        Ok(self.value.clone())
    }
}
