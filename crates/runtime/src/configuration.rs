use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use swarmie_composition::{EffectiveComposition, EffectiveInstance, SelectionState};
use swarmie_plugin_contracts::{
    ConfiguredValue, PluginConfiguration, PluginConfigurationInput, PluginConfigurationProvider,
    PluginConfigurationRef,
};
use tembra::{Context, Host, NativeFacet, Result, RuntimeError};

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

    /// Makes both executable identities read the same acknowledged input during a
    /// code-only provider replacement. Refusal restores the old bindings; adoption
    /// publishes the new bindings. Cancellation retains both while recovery is required.
    pub(crate) fn allow_replacement_facets(
        &self,
        before: &EffectiveInstance,
        after: &EffectiveInstance,
    ) {
        self.publish(before);
        if let Some((facets, _)) = self.0.borrow_mut().get_mut(&before.selection.id) {
            for facet in after.release.iter().flat_map(|release| &release.facets) {
                if !facets.contains(&facet.id) {
                    facets.push(facet.id.clone());
                }
            }
        }
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

/// Retains ordinary live configuration objects so candidate initializers can call them.
/// Host acquisition and default declaration acquisition use the same Host principal.
pub(crate) async fn hold_inputs(
    host: &Host,
    instance: &EffectiveInstance,
) -> Result<Vec<PluginConfigurationRef>> {
    let mut references = Vec::new();
    for facet in instance.release.iter().flat_map(|release| &release.facets) {
        if facet.target != swarmie_composition::FacetTarget::Server {
            continue;
        }
        let reference = host
            .get(
                PluginConfiguration,
                PluginConfigurationInput {
                    facet_id: facet.id.clone(),
                },
            )
            .await?;
        let input = reference.read().await?;
        if input.instance_id != instance.selection.id
            || input.configuration != instance.selection.config
        {
            return Err(RuntimeError::invalid(
                "plugin_configuration",
                "replacement input differs from the acknowledged configured instance",
            ));
        }
        references.push(reference);
    }
    Ok(references)
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
