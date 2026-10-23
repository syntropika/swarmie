use std::collections::BTreeMap;

use serde_json::json;
use swarmie_composition::*;

fn selection(id: &str, plugin: &str) -> Selection {
    Selection {
        id: id.into(),
        plugin: plugin.into(),
        version: "1.0.0".into(),
        provider: None,
        config: json!({}),
    }
}

fn release(plugin: &str) -> Release {
    Release {
        plugin: plugin.into(),
        version: "1.0.0".into(),
        kind: PluginKind::Core,
        facets: vec![FacetIdentity {
            id: plugin.into(),
            target: FacetTarget::Server,
        }],
    }
}

fn composition(plugins: Vec<Selection>) -> Composition {
    Composition {
        schema_version: 1,
        profile: Profile {
            id: "test".into(),
            bundles: vec![],
            plugins,
            overrides: vec![],
            disable: vec![],
        },
        bundles: BTreeMap::new(),
    }
}

#[test]
fn substitution_replaces_the_selection_without_service_contract_metadata() {
    let mut input = composition(vec![selection("consumer", "consumer")]);
    input.profile.bundles.push("foundation".into());
    input.bundles.insert(
        "foundation".into(),
        Bundle {
            plugins: vec![selection("counter", "core-counter")],
            ..Default::default()
        },
    );
    let mut alternative = release("dynamic-counter");
    alternative.kind = PluginKind::Dynamic;
    let catalog = [release("consumer"), release("core-counter"), alternative];
    let before = resolve(&input, &catalog).unwrap();
    let mut replacement = selection("counter", "dynamic-counter");
    replacement.provider = Some("local".into());
    replacement.config = json!({"initial":42});
    input.profile.overrides.push(replacement.clone());
    let snapshot = input.clone();
    let after = resolve(&input, &catalog).unwrap();
    assert_eq!(after.instances["counter"].selection, replacement);
    assert_eq!(after.instances["counter"].origin, Origin::Override);
    assert_eq!(before.instances["consumer"], after.instances["consumer"]);
    assert_eq!(after.deployment_order, ["counter", "consumer"]);
    assert_eq!(input, snapshot);
    assert_eq!(
        after,
        resolve(&input, &catalog.into_iter().rev().collect::<Vec<_>>()).unwrap()
    );
}

#[test]
fn nested_bundles_keep_declaration_order_and_whole_config_override() {
    let mut first = selection("z-first", "first");
    first.config = json!({"initial":1,"step":2});
    let mut input = composition(vec![selection("a-last", "last")]);
    input.profile.bundles.push("outer".into());
    input.bundles.insert(
        "outer".into(),
        Bundle {
            bundles: vec!["inner".into()],
            plugins: vec![selection("m-middle", "middle")],
        },
    );
    input.bundles.insert(
        "inner".into(),
        Bundle {
            plugins: vec![first],
            ..Default::default()
        },
    );
    input.profile.overrides.push(selection("z-first", "first"));
    let output = resolve(
        &input,
        &[release("first"), release("middle"), release("last")],
    )
    .unwrap();
    assert_eq!(output.deployment_order, ["z-first", "m-middle", "a-last"]);
    assert_eq!(output.instances["z-first"].selection.config, json!({}));
    assert_eq!(
        output.instances["m-middle"].origin,
        Origin::Bundle { id: "outer".into() }
    );
}

#[test]
fn unavailable_release_is_selection_data_and_does_not_block_other_plugins() {
    let input = composition(vec![
        selection("absent", "missing"),
        selection("present", "present"),
    ]);
    let output = resolve(&input, &[release("present")]).unwrap();
    assert_eq!(
        output.instances["absent"].state,
        SelectionState::UnavailableRelease {
            plugin: "missing".into(),
            version: "1.0.0".into()
        }
    );
    assert_eq!(output.deployment_order, ["present"]);
    assert_eq!(output.instances["present"].state, SelectionState::Enabled);
}

#[test]
fn disabled_selection_preserves_its_origin_without_reserving_active_facets() {
    let mut input = composition(vec![selection("old", "old"), selection("new", "new")]);
    input.profile.disable.push("old".into());
    let old = release("old");
    let mut new = release("new");
    new.facets = old.facets.clone();
    let output = resolve(&input, &[old, new]).unwrap();
    assert_eq!(output.instances["old"].state, SelectionState::Disabled);
    assert_eq!(output.instances["old"].origin, Origin::Profile);
    assert_eq!(output.deployment_order, ["new"]);
}

#[test]
fn fixed_package_and_target_facet_collisions_are_deterministic() {
    let same = composition(vec![selection("one", "same"), selection("two", "same")]);
    assert!(matches!(
        resolve(&same, &[release("same")]),
        Err(CompositionError::PackageCollision { .. })
    ));
    let input = composition(vec![selection("a", "a"), selection("b", "b")]);
    let a = release("a");
    let mut b = release("b");
    b.facets = a.facets.clone();
    assert!(matches!(
        resolve(&input, &[a.clone(), b.clone()]),
        Err(CompositionError::FacetCollision { .. })
    ));
    b.facets[0].target = FacetTarget::Browser;
    assert!(resolve(&input, &[a, b]).is_ok());
}

#[test]
fn bundle_cycles_and_unknown_edits_are_refused_without_mutation() {
    let mut input = composition(vec![]);
    input.profile.bundles.push("a".into());
    assert_eq!(
        resolve(&input, &[]),
        Err(CompositionError::UnknownBundle("a".into()))
    );
    input.bundles.insert(
        "a".into(),
        Bundle {
            bundles: vec!["b".into()],
            ..Default::default()
        },
    );
    input.bundles.insert(
        "b".into(),
        Bundle {
            bundles: vec!["a".into()],
            ..Default::default()
        },
    );
    let snapshot = input.clone();
    assert_eq!(
        resolve(&input, &[]),
        Err(CompositionError::BundleCycle(vec![
            "a".into(),
            "b".into(),
            "a".into()
        ]))
    );
    assert_eq!(input, snapshot);
    let mut input = composition(vec![selection("one", "one")]);
    input.profile.overrides.push(selection("absent", "one"));
    assert_eq!(
        resolve(&input, &[]),
        Err(CompositionError::UnknownOverride("absent".into()))
    );
    input.profile.overrides.clear();
    input.profile.disable.push("absent".into());
    assert_eq!(
        resolve(&input, &[]),
        Err(CompositionError::UnknownDisable("absent".into()))
    );
}

#[test]
fn input_validation_rejects_unknown_graph_fields_and_malformed_release_metadata() {
    let unknown_fields = json!({"id":"one","plugin":"one","version":"1.0.0","requires":[]});
    assert!(serde_json::from_value::<Selection>(unknown_fields).is_err());
    let mut input = composition(vec![selection("one", "one")]);
    input.profile.plugins[0].version = "^1.0".into();
    assert!(matches!(
        resolve(&input, &[]),
        Err(CompositionError::Invalid { .. })
    ));
    input.profile.plugins[0].version = "1.0.0".into();
    input.profile.plugins[0].config = json!([]);
    assert!(matches!(
        resolve(&input, &[]),
        Err(CompositionError::Invalid { .. })
    ));
    let mut invalid = release("one");
    invalid.facets.clear();
    assert!(matches!(
        resolve(&composition(vec![]), &[invalid]),
        Err(CompositionError::Invalid { .. })
    ));
    assert!(matches!(
        resolve(&composition(vec![]), &[release("one"), release("one")]),
        Err(CompositionError::DuplicateRelease { .. })
    ));
}
