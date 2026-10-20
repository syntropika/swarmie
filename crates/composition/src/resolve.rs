use std::collections::{BTreeMap, BTreeSet};

use crate::{
    Composition, CompositionError, Diagnostic, EffectiveComposition, EffectiveInstance,
    InstanceState, Origin, PluginKind, Release, Selection, dependencies,
    validation::{check_collisions, identifier, invalid, validate_catalog, validate_selection},
};

/// Resolves a profile against immutable, trusted catalog metadata without executing plugins.
///
/// # Guarantees
/// Inputs remain unchanged on success and failure. Bundle expansion follows declared order;
/// duplicate instance declarations refuse resolution unless replaced by a profile override.
/// An override replaces the entire selection. Disabled instances retain their origin.
/// Missing releases or requirements produce inspectable blocked instances. Only ready
/// instances enter the stable activation order; readiness here means metadata eligibility.
///
/// # Errors
/// Returns [`CompositionError`] for invalid metadata, unknown layers, duplicate identities,
/// fixed artifact collisions or required deployment cycles. Runtime configuration validation,
/// installation, live capability readiness and lifecycle remain the caller's responsibility.
///
/// # Examples
/// ```
/// use swarmie_composition::{Composition, resolve};
/// let input: Composition = serde_json::from_value(serde_json::json!({
///     "schema_version": 1, "profile": {"id": "empty"}
/// })).unwrap();
/// let before = input.clone();
/// let output = resolve(&input, &[]).unwrap();
/// assert!(output.activation_order.is_empty());
/// assert_eq!(input, before);
/// let mut invalid = input.clone();
/// invalid.schema_version = 2;
/// assert!(resolve(&invalid, &[]).is_err());
/// assert_eq!(input, before);
/// ```
pub fn resolve(
    input: &Composition,
    catalog: &[Release],
) -> Result<EffectiveComposition, CompositionError> {
    if input.schema_version != 1 {
        return Err(CompositionError::SchemaVersion(input.schema_version));
    }
    identifier("profile.id", &input.profile.id)?;
    let catalog = validate_catalog(catalog)?;
    let mut selections = BTreeMap::new();
    for bundle in &input.profile.bundles {
        expand(input, bundle, &mut Vec::new(), &mut selections)?;
    }
    for selection in &input.profile.plugins {
        insert(&mut selections, selection, Origin::Profile)?;
    }
    let mut overridden = BTreeSet::new();
    for selection in &input.profile.overrides {
        validate_selection(selection)?;
        if !overridden.insert(&selection.id) {
            return Err(CompositionError::DuplicateOverride(selection.id.clone()));
        }
        let entry = selections
            .get_mut(&selection.id)
            .ok_or_else(|| CompositionError::UnknownOverride(selection.id.clone()))?;
        *entry = (selection.clone(), Origin::Override);
    }
    let mut disabled = BTreeSet::new();
    for id in &input.profile.disable {
        if !selections.contains_key(id) {
            return Err(CompositionError::UnknownDisable(id.clone()));
        }
        if !disabled.insert(id.clone()) {
            return Err(CompositionError::DuplicateDisable(id.clone()));
        }
    }
    let mut instances = BTreeMap::new();
    for (id, (selection, origin)) in selections {
        let release = catalog
            .get(&(selection.plugin.clone(), selection.version.clone()))
            .copied();
        if let Some(release) = release {
            let dynamic = release.kind == PluginKind::Dynamic;
            if dynamic != selection.provider.is_some() {
                return Err(invalid(
                    format!("{id}.provider"),
                    "source provider must be present only for Dynamic selections",
                ));
            }
        }
        let state = if disabled.contains(&id) {
            InstanceState::Disabled
        } else if release.is_none() {
            InstanceState::Blocked {
                diagnostics: vec![Diagnostic::MissingRelease {
                    plugin: selection.plugin.clone(),
                    version: selection.version.clone(),
                }],
            }
        } else {
            InstanceState::Ready
        };
        instances.insert(
            id,
            EffectiveInstance {
                selection,
                origin,
                release: release.cloned(),
                state,
            },
        );
    }
    check_collisions(&instances)?;
    let activation_order = dependencies::resolve(&mut instances)?;
    Ok(EffectiveComposition {
        profile: input.profile.id.clone(),
        instances,
        activation_order,
    })
}

type Selections = BTreeMap<String, (Selection, Origin)>;

fn expand(
    input: &Composition,
    id: &str,
    path: &mut Vec<String>,
    selections: &mut Selections,
) -> Result<(), CompositionError> {
    identifier("bundle.id", id)?;
    if let Some(index) = path.iter().position(|part| part == id) {
        let mut cycle = path[index..].to_vec();
        cycle.push(id.into());
        return Err(CompositionError::BundleCycle(cycle));
    }
    let bundle = input
        .bundles
        .get(id)
        .ok_or_else(|| CompositionError::UnknownBundle(id.into()))?;
    path.push(id.into());
    for child in &bundle.bundles {
        expand(input, child, path, selections)?;
    }
    for selection in &bundle.plugins {
        insert(selections, selection, Origin::Bundle { id: id.into() })?;
    }
    path.pop();
    Ok(())
}

fn insert(
    selections: &mut Selections,
    selection: &Selection,
    origin: Origin,
) -> Result<(), CompositionError> {
    validate_selection(selection)?;
    if selections.contains_key(&selection.id) {
        return Err(CompositionError::DuplicateInstance(selection.id.clone()));
    }
    selections.insert(selection.id.clone(), (selection.clone(), origin));
    Ok(())
}
