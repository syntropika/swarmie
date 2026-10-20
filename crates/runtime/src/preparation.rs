use std::collections::{BTreeMap, BTreeSet};

use swarmie_composition::{EffectiveComposition, PluginKind};
use tembra::NativeFacet;

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
) -> Result<Prepared, Error> {
    if application
        .releases
        .iter()
        .any(|release| release.kind != PluginKind::Dynamic)
        || compiled
            .iter()
            .any(|adapter| adapter.release.kind != PluginKind::Core)
    {
        return Err(
            "compiled and imported release origins do not match their execution adapters".into(),
        );
    }
    let catalog = compiled
        .iter()
        .map(|adapter| adapter.release.clone())
        .chain(application.releases.clone())
        .collect::<Vec<_>>();
    let effective = swarmie_composition::resolve(&application.composition, &catalog)?;
    let mut native = BTreeMap::new();
    let mut trusted_sources = BTreeMap::new();
    for id in &effective.deployment_order {
        let instance = &effective.instances[id];
        let release = instance
            .release
            .as_ref()
            .ok_or("selected release metadata is absent")?;
        if ["swarmie.plugin-manager", crate::configuration::PLUGIN_ID]
            .contains(&release.plugin.as_str())
            || release.facets.iter().any(|facet| {
                ["swarmie.plugin-manager", crate::configuration::FACET_ID]
                    .contains(&facet.id.as_str())
            })
        {
            return Err("application selection conflicts with indispensable administration".into());
        }
        if release.kind != PluginKind::Core {
            continue;
        }
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
                "compiled adapter Facets differ from release: {}",
                release.plugin
            )
            .into());
        }
        native.insert(id.clone(), facets);
        trusted_sources.insert(id.clone(), adapter.trusted_sources.clone());
    }
    Ok(Prepared {
        effective,
        native,
        trusted_sources,
    })
}
