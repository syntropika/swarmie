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
        requires: vec![],
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
        provides: vec![],
        requires: vec![],
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

fn required(instance: &str) -> Requirement {
    Requirement {
        instance: instance.into(),
        optional: false,
    }
}

fn counter() -> Capability {
    Capability {
        contract: "example.counter".into(),
        version: 1,
    }
}

fn counter_requirement() -> CapabilityRequirement {
    CapabilityRequirement {
        contract: "example.counter".into(),
        version: 1,
        optional: false,
    }
}

fn diagnostics(result: &EffectiveComposition, id: &str) -> Vec<Diagnostic> {
    match &result.instances[id].state {
        InstanceState::Blocked { diagnostics } => diagnostics.clone(),
        other => panic!("expected blocked state, got {other:?}"),
    }
}

#[test]
fn replaces_bundled_core_with_dynamic_selection_and_preserves_consumer() {
    let mut provider = release("example.core");
    provider.provides.push(counter());
    let mut alternative = release("example.dynamic");
    alternative.kind = PluginKind::Dynamic;
    alternative.provides.push(counter());
    let mut consumer = release("example.consumer");
    consumer.requires.push(counter_requirement());
    let mut input = composition(vec![selection("consumer", "example.consumer")]);
    input.bundles.insert(
        "foundation".into(),
        Bundle {
            plugins: vec![selection("counter", "example.core")],
            ..Default::default()
        },
    );
    input.profile.bundles.push("foundation".into());
    let catalog = vec![provider, alternative, consumer];
    let before = resolve(&input, &catalog).unwrap();
    let consumer_before = before.instances["consumer"].selection.clone();
    let mut replacement = selection("counter", "example.dynamic");
    replacement.provider = Some("community".into());
    replacement.config = json!({ "step": 10 });
    input.profile.overrides.push(replacement.clone());
    let snapshot = input.clone();
    let after = resolve(&input, &catalog).unwrap();
    assert_eq!(after.instances["counter"].selection, replacement);
    assert_eq!(after.instances["counter"].origin, Origin::Override);
    assert_eq!(after.instances["consumer"].selection, consumer_before);
    assert_eq!(after.activation_order, ["counter", "consumer"]);
    assert_eq!(input, snapshot);
    assert_eq!(
        after,
        resolve(&input, &catalog.into_iter().rev().collect::<Vec<_>>()).unwrap()
    );
}

#[test]
fn override_replaces_config_instead_of_merging_and_nested_origins_survive() {
    let mut original = selection("counter", "example.counter");
    original.config = json!({ "initial": 1, "step": 2 });
    let mut input = composition(vec![]);
    input.bundles.insert(
        "outer".into(),
        Bundle {
            bundles: vec!["inner".into()],
            ..Default::default()
        },
    );
    input.bundles.insert(
        "inner".into(),
        Bundle {
            plugins: vec![original],
            ..Default::default()
        },
    );
    input.profile.bundles.push("outer".into());
    let catalog = vec![release("example.counter")];
    let bundled = resolve(&input, &catalog).unwrap();
    assert_eq!(
        bundled.instances["counter"].origin,
        Origin::Bundle { id: "inner".into() }
    );
    input
        .profile
        .overrides
        .push(selection("counter", "example.counter"));
    assert_eq!(
        resolve(&input, &catalog).unwrap().instances["counter"]
            .selection
            .config,
        json!({})
    );
}

#[test]
fn required_instance_blocking_propagates_and_restoration_resolves() {
    let mut consumer = selection("a-consumer", "consumer");
    consumer.requires.push(required("provider"));
    let mut downstream = selection("downstream", "downstream");
    downstream.requires.push(required("a-consumer"));
    let mut input = composition(vec![consumer, downstream]);
    let catalog = vec![
        release("consumer"),
        release("downstream"),
        release("provider"),
    ];
    let missing = resolve(&input, &catalog).unwrap();
    assert_eq!(
        diagnostics(&missing, "a-consumer"),
        [Diagnostic::MissingInstance {
            instance: "provider".into()
        }]
    );
    assert_eq!(
        diagnostics(&missing, "downstream"),
        [Diagnostic::UnavailableInstance {
            instance: "a-consumer".into()
        }]
    );
    assert!(missing.activation_order.is_empty());
    input
        .profile
        .plugins
        .push(selection("provider", "provider"));
    assert_eq!(
        resolve(&input, &catalog).unwrap().activation_order,
        ["provider", "a-consumer", "downstream"]
    );
    input.profile.disable.push("provider".into());
    let disabled = resolve(&input, &catalog).unwrap();
    assert_eq!(
        disabled.instances["provider"].state,
        InstanceState::Disabled
    );
    assert_eq!(disabled.instances["provider"].origin, Origin::Profile);
    assert!(disabled.activation_order.is_empty());
    input.profile.disable.clear();
    assert_eq!(
        resolve(&input, &catalog).unwrap().instances["downstream"].state,
        InstanceState::Ready
    );
}

#[test]
fn incompatible_and_ambiguous_capabilities_are_diagnostic() {
    let mut consumer = release("consumer");
    consumer.requires.push(counter_requirement());
    let mut provider = release("provider");
    provider.provides.push(Capability {
        version: 2,
        ..counter()
    });
    let mut input = composition(vec![
        selection("consumer", "consumer"),
        selection("provider", "provider"),
    ]);
    let mismatch = resolve(&input, &[consumer.clone(), provider.clone()]).unwrap();
    assert_eq!(
        diagnostics(&mismatch, "consumer"),
        [Diagnostic::IncompatibleCapability {
            contract: "example.counter".into(),
            version: 1,
            available: vec![2],
        }]
    );
    provider.provides[0].version = 1;
    let mut alternative = release("alternative");
    alternative.provides.push(counter());
    input
        .profile
        .plugins
        .push(selection("alternative", "alternative"));
    let ambiguous = resolve(&input, &[consumer, provider, alternative]).unwrap();
    assert_eq!(
        diagnostics(&ambiguous, "consumer"),
        [Diagnostic::AmbiguousCapability {
            contract: "example.counter".into(),
            version: 1,
            providers: vec!["alternative".into(), "provider".into()],
        }]
    );
}

#[test]
fn disabled_provider_does_not_compete_and_cannot_satisfy_a_requirement() {
    let mut consumer = release("consumer");
    consumer.requires.push(counter_requirement());
    let mut first = release("first");
    first.provides.push(counter());
    let mut second = release("second");
    second.provides.push(counter());
    let catalog = [consumer, first, second];
    let mut input = composition(vec![
        selection("consumer", "consumer"),
        selection("first", "first"),
        selection("second", "second"),
    ]);
    input.profile.disable.push("first".into());
    let output = resolve(&input, &catalog).unwrap();
    assert_eq!(output.instances["consumer"].state, InstanceState::Ready);
    assert_eq!(output.activation_order, ["second", "consumer"]);
    input.profile.disable.push("second".into());
    let output = resolve(&input, &catalog).unwrap();
    assert_eq!(
        diagnostics(&output, "consumer"),
        [Diagnostic::MissingCapability {
            contract: "example.counter".into(),
            version: 1,
        }]
    );
    assert!(output.activation_order.is_empty());
}

#[test]
fn optional_capabilities_and_instance_cycles_do_not_block() {
    let mut a = selection("a", "a");
    a.requires.push(Requirement {
        instance: "b".into(),
        optional: true,
    });
    let mut b = selection("b", "b");
    b.requires.push(Requirement {
        instance: "a".into(),
        optional: true,
    });
    b.requires.push(Requirement {
        instance: "absent".into(),
        optional: true,
    });
    let mut a_release = release("a");
    a_release.requires.push(CapabilityRequirement {
        optional: true,
        ..counter_requirement()
    });
    let output = resolve(&composition(vec![a, b]), &[a_release, release("b")]).unwrap();
    assert_eq!(output.activation_order, ["a", "b"]);
}

#[test]
fn intra_plugin_capability_does_not_create_a_second_object_dependency_graph() {
    let mut bundled = release("bundled");
    bundled.provides.push(counter());
    bundled.requires.push(counter_requirement());
    let output = resolve(
        &composition(vec![selection("bundled", "bundled")]),
        &[bundled],
    )
    .unwrap();
    assert_eq!(output.activation_order, ["bundled"]);
    assert_eq!(output.instances["bundled"].state, InstanceState::Ready);
}

#[test]
fn required_cycles_report_a_reproducible_path() {
    let mut a = selection("a", "a");
    a.requires.push(required("b"));
    let mut b = selection("b", "b");
    b.requires.push(required("a"));
    let input = composition(vec![b, a]);
    assert_eq!(
        resolve(&input, &[release("a"), release("b")]),
        Err(CompositionError::DependencyCycle(vec![
            "a".into(),
            "b".into(),
            "a".into()
        ]))
    );
}

#[test]
fn capability_cycles_are_deployment_cycles() {
    let mut a = release("a");
    a.provides.push(counter());
    a.requires.push(CapabilityRequirement {
        contract: "other".into(),
        version: 1,
        optional: false,
    });
    let mut b = release("b");
    b.provides.push(Capability {
        contract: "other".into(),
        version: 1,
    });
    b.requires.push(counter_requirement());
    assert!(matches!(
        resolve(
            &composition(vec![selection("a", "a"), selection("b", "b")]),
            &[a, b]
        ),
        Err(CompositionError::DependencyCycle(_))
    ));
}

#[test]
fn missing_release_is_blocked_and_retains_exact_configuration() {
    let input = composition(vec![selection("counter", "unavailable")]);
    let output = resolve(&input, &[]).unwrap();
    assert_eq!(
        diagnostics(&output, "counter"),
        [Diagnostic::MissingRelease {
            plugin: "unavailable".into(),
            version: "1.0.0".into()
        }]
    );
    assert_eq!(
        output.instances["counter"].selection,
        input.profile.plugins[0]
    );
    assert!(output.instances["counter"].release.is_none());
}

#[test]
fn unavailable_capability_provider_blocks_its_consumer_transitively() {
    let mut provider = release("provider");
    provider.provides.push(counter());
    let mut provider_selection = selection("provider", "provider");
    provider_selection.requires.push(required("missing"));
    let mut consumer = release("consumer");
    consumer.requires.push(counter_requirement());
    let output = resolve(
        &composition(vec![provider_selection, selection("consumer", "consumer")]),
        &[provider, consumer],
    )
    .unwrap();
    assert_eq!(
        diagnostics(&output, "consumer"),
        [Diagnostic::UnavailableInstance {
            instance: "provider".into()
        }]
    );
}

#[test]
fn fixed_package_and_facet_collisions_are_refused_but_disabled_rows_do_not_collide() {
    let mut input = composition(vec![
        selection("first", "provider"),
        selection("second", "provider"),
    ]);
    assert!(matches!(
        resolve(&input, &[release("provider")]),
        Err(CompositionError::PackageCollision { .. })
    ));
    input.profile.disable.push("first".into());
    assert_eq!(
        resolve(&input, &[release("provider")])
            .unwrap()
            .activation_order,
        ["second"]
    );
    let a = release("a");
    let mut b = release("b");
    b.facets = a.facets.clone();
    let input = composition(vec![selection("a", "a"), selection("b", "b")]);
    assert!(matches!(
        resolve(&input, &[a.clone(), b.clone()]),
        Err(CompositionError::FacetCollision { .. })
    ));
    b.facets[0].target = FacetTarget::Browser;
    assert!(resolve(&input, &[a, b]).is_ok());
}

#[test]
fn duplicate_selection_and_unknown_override_or_disable_are_refused() {
    let mut input = composition(vec![
        selection("counter", "counter"),
        selection("counter", "counter"),
    ]);
    assert_eq!(
        resolve(&input, &[]),
        Err(CompositionError::DuplicateInstance("counter".into()))
    );
    input.profile.plugins.pop();
    input
        .profile
        .overrides
        .push(selection("missing", "counter"));
    assert_eq!(
        resolve(&input, &[]),
        Err(CompositionError::UnknownOverride("missing".into()))
    );
    input.profile.overrides.clear();
    input.profile.disable.push("missing".into());
    assert_eq!(
        resolve(&input, &[]),
        Err(CompositionError::UnknownDisable("missing".into()))
    );
}

#[test]
fn bundle_cycles_and_missing_bundles_are_refused() {
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
    assert_eq!(
        resolve(&input, &[]),
        Err(CompositionError::BundleCycle(vec![
            "a".into(),
            "b".into(),
            "a".into()
        ]))
    );
}

#[test]
fn serde_and_semantic_boundaries_reject_unchecked_values() {
    let invalid_input = json!({ "schema_version":1, "profile":{"id":"test", "surprise":true} });
    assert!(serde_json::from_value::<Composition>(invalid_input).is_err());
    let mut input = composition(vec![selection("counter", "counter")]);
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
    input.profile.plugins[0].config = json!({});
    input.profile.plugins[0].provider = Some("source".into());
    assert!(matches!(
        resolve(&input, &[release("counter")]),
        Err(CompositionError::Invalid { .. })
    ));
}

#[test]
fn catalog_validation_precedes_resolution_even_for_unselected_releases() {
    let empty = composition(vec![]);
    let mut invalid = release("counter");
    invalid.facets.clear();
    assert!(matches!(
        resolve(&empty, &[invalid]),
        Err(CompositionError::Invalid { .. })
    ));
    let mut invalid = release("counter");
    invalid.provides.push(Capability {
        version: 0,
        ..counter()
    });
    assert!(matches!(
        resolve(&empty, &[invalid]),
        Err(CompositionError::Invalid { .. })
    ));
    assert!(matches!(
        resolve(&empty, &[release("counter"), release("counter")]),
        Err(CompositionError::DuplicateRelease { .. })
    ));
}
