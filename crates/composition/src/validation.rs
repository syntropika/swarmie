use std::collections::{BTreeMap, BTreeSet};

use crate::{CompositionError, EffectiveInstance, FacetTarget, InstanceState, Release, Selection};

pub(crate) fn invalid(field: impl Into<String>, reason: impl Into<String>) -> CompositionError {
    CompositionError::Invalid {
        field: field.into(),
        reason: reason.into(),
    }
}

pub(crate) fn identifier(field: &str, value: &str) -> Result<(), CompositionError> {
    if value.is_empty()
        || value.chars().any(char::is_whitespace)
        || value.chars().any(char::is_control)
    {
        return Err(invalid(
            field,
            "identifier must be nonempty without whitespace or control characters",
        ));
    }
    Ok(())
}

pub(crate) fn version(field: &str, value: &str) -> Result<(), CompositionError> {
    semver::Version::parse(value).map_err(|error| invalid(field, error.to_string()))?;
    Ok(())
}

pub(crate) fn validate_selection(selection: &Selection) -> Result<(), CompositionError> {
    identifier("selection.id", &selection.id)?;
    identifier("selection.plugin", &selection.plugin)?;
    version("selection.version", &selection.version)?;
    if let Some(provider) = &selection.provider {
        identifier("selection.provider", provider)?;
    }
    if !selection.config.is_object() {
        return Err(invalid(
            format!("{}.config", selection.id),
            "configuration must be an object",
        ));
    }
    let mut required = BTreeSet::new();
    for dependency in &selection.requires {
        identifier("requirement.instance", &dependency.instance)?;
        if !required.insert(&dependency.instance) {
            return Err(invalid(
                format!("{}.requires", selection.id),
                "duplicate instance requirement",
            ));
        }
    }
    Ok(())
}

pub(crate) fn validate_catalog(
    catalog: &[Release],
) -> Result<BTreeMap<(String, String), &Release>, CompositionError> {
    let mut index = BTreeMap::new();
    for release in catalog {
        identifier("release.plugin", &release.plugin)?;
        version("release.version", &release.version)?;
        if release.facets.is_empty() {
            return Err(invalid(
                "release.facets",
                "at least one executable Facet is required",
            ));
        }
        let mut facets = BTreeSet::new();
        for facet in &release.facets {
            identifier("facet.id", &facet.id)?;
            if !facets.insert((target(facet.target), &facet.id)) {
                return Err(invalid(
                    "release.facets",
                    "duplicate Facet identity for target",
                ));
            }
        }
        for (field, capabilities) in [
            (
                "release.provides",
                release
                    .provides
                    .iter()
                    .map(|c| (&c.contract, c.version))
                    .collect::<Vec<_>>(),
            ),
            (
                "release.requires",
                release
                    .requires
                    .iter()
                    .map(|c| (&c.contract, c.version))
                    .collect::<Vec<_>>(),
            ),
        ] {
            let mut contracts = BTreeSet::new();
            for (contract, version) in capabilities {
                identifier(field, contract)?;
                if version == 0 || !contracts.insert((contract, version)) {
                    return Err(invalid(
                        field,
                        "capability versions must be positive and declarations unique",
                    ));
                }
            }
        }
        let key = (release.plugin.clone(), release.version.clone());
        if index.insert(key, release).is_some() {
            return Err(CompositionError::DuplicateRelease {
                plugin: release.plugin.clone(),
                version: release.version.clone(),
            });
        }
    }
    Ok(index)
}

fn target(target: FacetTarget) -> &'static str {
    match target {
        FacetTarget::Server => "server",
        FacetTarget::Browser => "browser",
    }
}

pub(crate) fn check_collisions(
    instances: &BTreeMap<String, EffectiveInstance>,
) -> Result<(), CompositionError> {
    let mut packages = BTreeMap::new();
    let mut facets = BTreeMap::new();
    for (id, instance) in instances {
        if instance.state == InstanceState::Disabled {
            continue;
        }
        if let Some(first) = packages.insert(&instance.selection.plugin, id) {
            return Err(CompositionError::PackageCollision {
                plugin: instance.selection.plugin.clone(),
                first: first.clone(),
                second: id.clone(),
            });
        }
        if let Some(release) = &instance.release {
            for facet in &release.facets {
                if let Some(first) = facets.insert((target(facet.target), &facet.id), id) {
                    return Err(CompositionError::FacetCollision {
                        target: target(facet.target).into(),
                        facet: facet.id.clone(),
                        first: first.clone(),
                        second: id.clone(),
                    });
                }
            }
        }
    }
    Ok(())
}
