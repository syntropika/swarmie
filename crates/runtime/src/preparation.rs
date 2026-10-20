use std::{
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

use swarmie_composition::{EffectiveComposition, InstanceState, PluginKind};
use tembra::{NativeFacet, contracts::Contract};

use crate::{Application, CompiledPlugin};

type Error = Box<dyn std::error::Error>;

pub(crate) struct Prepared {
    pub effective: EffectiveComposition,
    pub native: BTreeMap<String, Vec<NativeFacet>>,
    pub trusted_sources: BTreeMap<String, Vec<String>>,
}

pub(crate) fn prepare(
    application: &Application,
    compiled: &[CompiledPlugin],
    contracts: &[Rc<Contract>],
) -> Result<Prepared, Error> {
    if application
        .releases
        .iter()
        .any(|release| release.kind != PluginKind::Dynamic)
    {
        return Err("imported release facts must describe Dynamic Plugins".into());
    }
    let catalog = compiled
        .iter()
        .map(|plugin| plugin.release.clone())
        .chain(application.releases.clone())
        .collect::<Vec<_>>();
    let mut effective = swarmie_composition::resolve(&application.composition, &catalog)?;
    let mut native = BTreeMap::new();
    let mut trusted_sources = BTreeMap::new();
    for id in &effective.activation_order {
        let instance = &effective.instances[id];
        let release = instance
            .release
            .as_ref()
            .ok_or("eligible release metadata is absent")?;
        if ["swarmie.plugin-manager", crate::configuration::PLUGIN_ID]
            .contains(&release.plugin.as_str())
            || release.facets.iter().any(|facet| {
                ["swarmie.plugin-manager", crate::configuration::FACET_ID]
                    .contains(&facet.id.as_str())
            })
        {
            return Err("application selection conflicts with indispensable administration".into());
        }
        for capability in &release.provides {
            if !contracts.iter().any(|contract| {
                contract.name() == capability.contract
                    && contract.version() == u64::from(capability.version)
                    && contract.kind() == tembra::contracts::ContractKind::Local
            }) {
                return Err(format!(
                    "unqualified singleton capability: {}@{}",
                    capability.contract, capability.version
                )
                .into());
            }
        }
        match release.kind {
            PluginKind::Core => {
                let adapter = compiled
                    .iter()
                    .find(|adapter| adapter.release == *release)
                    .ok_or("compiled adapter is absent")?;
                let facets = adapter.prepare(&instance.selection.config)?;
                let actual = facets
                    .iter()
                    .map(|facet| facet.core().name().to_owned())
                    .collect::<BTreeSet<_>>();
                let expected = release
                    .facets
                    .iter()
                    .map(|facet| facet.id.clone())
                    .collect::<BTreeSet<_>>();
                if facets.len() != actual.len()
                    || actual != expected
                    || release
                        .facets
                        .iter()
                        .any(|facet| facet.target != swarmie_composition::FacetTarget::Server)
                {
                    return Err(format!(
                        "compiled adapter Facets differ from catalog: {}",
                        release.plugin
                    )
                    .into());
                }
                native.insert(id.clone(), facets);
                trusted_sources.insert(id.clone(), adapter.trusted_sources.clone());
            }
            PluginKind::Dynamic => {}
        }
    }
    // Current compiled factories provide acquisition and foundational capabilities. Refuse an
    // unsupported eager Core-to-Dynamic edge instead of hanging initial native installation.
    for id in native.keys() {
        let instance = &effective.instances[id];
        for requirement in &instance.selection.requires {
            if !requirement.optional
                && effective
                    .instances
                    .get(&requirement.instance)
                    .and_then(|other| other.release.as_ref())
                    .is_some_and(|other| other.kind == PluginKind::Dynamic)
            {
                return Err(
                    "compiled startup dependencies cannot require a later Dynamic deployment"
                        .into(),
                );
            }
        }
        for requirement in &instance
            .release
            .as_ref()
            .ok_or("compiled release absent")?
            .requires
        {
            if !requirement.optional
                && effective.instances.values().any(|other| {
                    other.state == InstanceState::Ready
                        && other.release.as_ref().is_some_and(|release| {
                            release.kind == PluginKind::Dynamic
                                && release.provides.iter().any(|capability| {
                                    capability.contract == requirement.contract
                                        && capability.version == requirement.version
                                })
                        })
                })
            {
                return Err(
                    "compiled startup capabilities cannot require a later Dynamic deployment"
                        .into(),
                );
            }
        }
    }
    // The qualified adapter admits acquisition Core Plugins before staged JavaScript. The
    // refused Core-to-Dynamic edges above make this stable partition preserve dependencies.
    let core_ids = native.keys().cloned().collect::<BTreeSet<_>>();
    effective
        .activation_order
        .sort_by_key(|id| !core_ids.contains(id));
    Ok(Prepared {
        effective,
        native,
        trusted_sources,
    })
}
