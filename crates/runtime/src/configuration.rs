use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use swarmie_composition::{EffectiveComposition, EffectiveInstance, SelectionState};
use swarmie_plugin_contracts::{ConfiguredValue, PluginConfiguration, PluginConfigurationProvider};
use tembra::{Context, NativeFacet, Result, RuntimeError};

pub(crate) const PLUGIN_ID: &str = "swarmie.application-configuration";
pub(crate) const FACET_ID: &str = "swarmie.application-configuration.runtime";

/// Initialization or acknowledged input, keyed by configured instance. Facet bindings are explicit;
/// this delivery object does not validate application schemas or drive plugin lifecycle.
type DeliveredValues = BTreeMap<String, (Vec<String>, ConfiguredValue)>;

#[derive(Clone, Default)]
pub(crate) struct ConfigurationValues(Rc<RefCell<DeliveredValues>>);

impl ConfigurationValues {
    pub(crate) fn new(effective: &EffectiveComposition) -> Self {
        let values = Self::default();
        for instance in effective.instances.values() {
            if instance.state == SelectionState::Enabled {
                values.publish(instance);
            }
        }
        values
    }

    pub(crate) fn publish(&self, instance: &EffectiveInstance) {
        if let Some(release) = &instance.release {
            self.0.borrow_mut().insert(
                instance.selection.id.clone(),
                (
                    release
                        .facets
                        .iter()
                        .map(|facet| facet.id.clone())
                        .collect(),
                    ConfiguredValue {
                        instance_id: instance.selection.id.clone(),
                        configuration: instance.selection.config.clone(),
                    },
                ),
            );
        }
    }

    pub(crate) fn remove(&self, instance: &str) {
        self.0.borrow_mut().remove(instance);
    }

    pub(crate) fn facet(&self) -> Result<NativeFacet> {
        let values = self.clone();
        NativeFacet::new(FACET_ID, vec![], move |_, setup| {
            let values = values.clone();
            setup.serve(PluginConfiguration, move |input, _, _| {
                Ok(Configuration {
                    values: values.clone(),
                    facet_id: input.facet_id,
                })
            })
        })
    }
}

struct Configuration {
    values: ConfigurationValues,
    facet_id: String,
}

impl PluginConfigurationProvider for Configuration {
    async fn read(&self, _context: Context) -> Result<ConfiguredValue> {
        self.values
            .0
            .borrow()
            .values()
            .find(|(facets, _)| facets.contains(&self.facet_id))
            .map(|(_, value)| value.clone())
            .ok_or_else(|| {
                RuntimeError::invalid(
                    "plugin_configuration",
                    "Facet has no selected configuration input",
                )
            })
    }
}
