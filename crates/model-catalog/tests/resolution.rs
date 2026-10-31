use std::sync::Arc;
use swarmie_model_catalog::{
    Capability, Catalog, Evidence, Layer, LimitClaim, Limits, Metadata, ModelIdentity,
    NativeCompaction, Precision, Preparation, Requirements, ResolutionError, SourceRecord,
    sources::ModelsDev,
};

fn identity() -> ModelIdentity {
    ModelIdentity {
        provider: "fixture".into(),
        adapter: "chat".into(),
        endpoint: "https://provider.example/v1/".into(),
        deployment: None,
        model: "same-name".into(),
        revision: None,
    }
}

fn evidence(layer: Layer) -> Evidence {
    Evidence {
        source: "fixture-source".into(),
        revision: "1".into(),
        layer,
        observed_at_ms: 10,
        expires_at_ms: None,
    }
}

fn claim(tokens: u64, layer: Layer) -> LimitClaim {
    LimitClaim {
        tokens: tokens.try_into().unwrap(),
        precision: Precision::Reported,
        evidence: evidence(layer),
    }
}

fn record() -> SourceRecord {
    SourceRecord {
        identity: identity(),
        limits: Limits {
            context: vec![claim(1000, Layer::Catalog)],
            input: vec![],
            output: vec![claim(100, Layer::Catalog)],
        },
        tools: Capability::default(),
        streaming: Capability::default(),
        native_output_maximum: Capability::default(),
        native_compaction: Capability::default(),
    }
}

fn preparation() -> Preparation {
    Preparation {
        identity: identity(),
        configuration_revision: "config-1".into(),
        requirements: Requirements::default(),
        allow_stale_for_ms: 0,
    }
}

fn catalog(records: Vec<SourceRecord>) -> Catalog {
    let mut catalog = Catalog::new(8);
    catalog.replace_sources(records).unwrap();
    catalog
}

#[test]
fn effective_limits_retain_restrictions_and_policy_cannot_raise_a_ceiling() {
    let mut source = record();
    source
        .limits
        .context
        .extend([claim(800, Layer::Deployment), claim(2000, Layer::Policy)]);
    source.limits.input.push(claim(700, Layer::Configuration));
    let mut catalog = catalog(vec![source]);
    let snapshot = catalog.prepare(preparation(), 10).unwrap();
    assert_eq!(snapshot.context().known().unwrap().tokens.get(), 800);
    assert_eq!(snapshot.context().known().unwrap().claims.len(), 3);
    assert_eq!(snapshot.input().known().unwrap().tokens.get(), 700);
    assert_eq!(snapshot.output().known().unwrap().tokens.get(), 100);
}

#[test]
fn input_can_use_a_conservative_context_ceiling_but_output_remains_unknown() {
    let mut source = record();
    source.limits.output.clear();
    let mut catalog = catalog(vec![source]);
    assert!(matches!(
        catalog.prepare(preparation(), 10),
        Err(ResolutionError::MissingLimit {
            field: "output",
            ..
        })
    ));
    let mut selection = preparation();
    selection.requirements.output = false;
    let snapshot = catalog.prepare(selection, 10).unwrap();
    assert_eq!(snapshot.input().known().unwrap().tokens.get(), 1000);
    assert_eq!(
        snapshot.input().known().unwrap().precision,
        Precision::Conservative
    );
    assert!(matches!(snapshot.output(), Metadata::Unknown { .. }));
}

#[test]
fn no_source_estimates_and_policy_only_values_cannot_establish_limits() {
    let mut source = record();
    source.limits.context = vec![claim(1000, Layer::Policy)];
    let mut estimated = claim(5000, Layer::Catalog);
    estimated.precision = Precision::Estimated;
    source.limits.context.push(estimated);
    let mut catalog = catalog(vec![source]);
    assert!(matches!(
        catalog.prepare(preparation(), 10),
        Err(ResolutionError::MissingLimit {
            field: "context",
            ..
        })
    ));
    catalog.replace_sources(vec![]).unwrap();
    assert!(catalog.prepare(preparation(), 10).is_err());
}

#[test]
fn exact_route_identity_prevents_metadata_and_capability_collisions() {
    for change in 0..6 {
        let mut selection = preparation();
        match change {
            0 => selection.identity.provider = "other".into(),
            1 => selection.identity.adapter = "responses".into(),
            2 => selection.identity.endpoint = "https://other.example/v1/".into(),
            3 => selection.identity.deployment = Some("deployment".into()),
            4 => selection.identity.model = "other".into(),
            _ => selection.identity.revision = Some("revision".into()),
        }
        assert!(catalog(vec![record()]).prepare(selection, 10).is_err());
    }
    let mut bad = preparation();
    bad.identity.endpoint = "https://secret@provider.example/v1".into();
    assert!(matches!(
        catalog(vec![record()]).prepare(bad, 10),
        Err(ResolutionError::InvalidIdentity)
    ));
}

#[test]
fn prepared_values_are_reused_and_replacement_does_not_mutate_existing_snapshots() {
    let mut catalog = catalog(vec![record()]);
    let first = catalog.prepare(preparation(), 10).unwrap();
    for now in 11..1010 {
        let next = catalog.prepare(preparation(), now).unwrap();
        assert!(Arc::ptr_eq(&first, &next));
    }
    let mut changed = preparation();
    changed.configuration_revision = "config-2".into();
    let second = catalog.prepare(changed, 20).unwrap();
    assert_ne!(first.generation(), second.generation());
    let mut source = record();
    source.limits.context = vec![claim(2000, Layer::Configuration)];
    catalog.replace_sources(vec![source]).unwrap();
    let third = catalog.prepare(preparation(), 30).unwrap();
    assert_eq!(first.context().known().unwrap().tokens.get(), 1000);
    assert_eq!(third.context().known().unwrap().tokens.get(), 2000);
    assert_ne!(first.generation(), third.generation());
    catalog.invalidate(&identity());
    assert!(!Arc::ptr_eq(
        &third,
        &catalog.prepare(preparation(), 30).unwrap()
    ));
}

#[test]
fn expiry_and_explicit_stale_policy_change_snapshot_and_fail_after_grace() {
    let mut source = record();
    for claim in source
        .limits
        .context
        .iter_mut()
        .chain(&mut source.limits.output)
    {
        claim.evidence.expires_at_ms = Some(100);
    }
    let mut catalog = catalog(vec![source]);
    let mut selection = preparation();
    selection.allow_stale_for_ms = 50;
    let fresh = catalog.prepare(selection.clone(), 20).unwrap();
    assert_eq!(fresh.valid_until_ms(), Some(100));
    let stale = catalog.prepare(selection.clone(), 100).unwrap();
    assert!(!stale.stale_evidence().is_empty());
    assert_ne!(fresh.generation(), stale.generation());
    assert_eq!(stale.valid_until_ms(), Some(150));
    assert!(catalog.prepare(selection, 150).is_err());
    assert!(catalog.prepare(preparation(), 100).is_err());
}

fn native() -> Capability<NativeCompaction> {
    Capability::Supported {
        evidence: evidence(Layer::Adapter),
        value: NativeCompaction {
            method: "compact".into(),
            input_format: "fixture-request-v1".into(),
            output_format: "opaque-v1".into(),
            continuation_format: "opaque-v1".into(),
            max_input_tokens: 1000.try_into().unwrap(),
            max_output_tokens: 100.try_into().unwrap(),
        },
    }
}

#[test]
fn native_compaction_requires_fresh_adapter_evidence_and_exact_binding() {
    let mut source = record();
    source.native_compaction = native();
    let mut selection = preparation();
    selection.requirements.native_compaction = true;
    let mut catalog = catalog(vec![source.clone()]);
    assert!(catalog.prepare(selection.clone(), 10).is_ok());
    if let Capability::Supported { evidence, .. } = &mut source.native_compaction {
        evidence.layer = Layer::Catalog;
    }
    assert!(matches!(
        catalog.replace_sources(vec![source.clone()]),
        Err(ResolutionError::InvalidEvidence)
    ));
    // Failed replacement is atomic.
    assert!(catalog.prepare(selection.clone(), 10).is_ok());
    if let Capability::Supported { evidence, .. } = &mut source.native_compaction {
        evidence.layer = Layer::Adapter;
        evidence.expires_at_ms = Some(20);
    }
    catalog.replace_sources(vec![source]).unwrap();
    selection.allow_stale_for_ms = 100;
    assert!(matches!(
        catalog.prepare(selection, 20),
        Err(ResolutionError::MissingCapability("native_compaction"))
    ));
}

#[test]
fn duplicate_sources_and_contradictory_adapter_support_are_explicit_failures() {
    let source = record();
    assert!(matches!(
        Catalog::new(1).replace_sources(vec![source.clone(), source.clone()]),
        Err(ResolutionError::DuplicateSource)
    ));
    let mut yes = source.clone();
    yes.native_compaction = native();
    let mut no = source;
    no.native_compaction = Capability::Unsupported {
        evidence: evidence(Layer::Adapter),
        reason: "route does not preserve opaque input".into(),
    };
    assert!(matches!(
        catalog(vec![yes, no]).prepare(preparation(), 10),
        Err(ResolutionError::ConflictingCapability("native_compaction"))
    ));
}

#[test]
fn generations_are_independent_of_source_order_and_cache_eviction() {
    let first = record();
    let mut second = record();
    second.limits.context = vec![claim(500, Layer::Deployment)];
    let a = catalog(vec![first.clone(), second.clone()])
        .prepare(preparation(), 10)
        .unwrap();
    let b = catalog(vec![second, first])
        .prepare(preparation(), 10)
        .unwrap();
    assert_eq!(a.generation(), b.generation());
}

#[test]
fn models_dev_is_loaded_once_and_never_promotes_external_flags_to_adapter_support() {
    let document = br#"{"fixture":{"models":{"same-name":{"id":"same-name","limit":{"context":2000,"output":300},"tool_call":true,"reasoning":true}}}}"#;
    let source = ModelsDev::from_json(document, evidence(Layer::Catalog)).unwrap();
    let bound = source.bind(identity()).unwrap().unwrap();
    assert!(matches!(bound.tools, Capability::Unknown { .. }));
    assert!(matches!(
        bound.native_output_maximum,
        Capability::Unknown { .. }
    ));
    assert!(matches!(
        bound.native_compaction,
        Capability::Unknown { .. }
    ));
    let snapshot = catalog(vec![bound]).prepare(preparation(), 10).unwrap();
    assert_eq!(snapshot.context().known().unwrap().tokens.get(), 2000);
    let mut other = identity();
    other.model = "SAME-NAME".into();
    assert!(source.bind(other).unwrap().is_none());
    let mut revised = identity();
    revised.revision = Some("deployment-revision".into());
    assert!(source.bind(revised).unwrap().is_none());
}

#[test]
fn future_evidence_is_unknown_and_cache_reprepares_at_its_start_time() {
    let mut source = record();
    for claim in source
        .limits
        .context
        .iter_mut()
        .chain(&mut source.limits.output)
    {
        claim.evidence.observed_at_ms = 100;
    }
    let mut selection = preparation();
    selection.requirements.context = false;
    selection.requirements.input = false;
    selection.requirements.output = false;
    let mut catalog = catalog(vec![source]);
    let unknown = catalog.prepare(selection.clone(), 99).unwrap();
    assert!(unknown.context().known().is_none());
    assert_eq!(unknown.valid_until_ms(), Some(100));
    let known = catalog.prepare(selection, 100).unwrap();
    assert!(known.context().known().is_some());
    assert_ne!(unknown.generation(), known.generation());
}

#[test]
fn cache_is_bounded_and_claim_order_does_not_change_generation() {
    let mut source = record();
    source.limits.context.push(claim(500, Layer::Deployment));
    let a = catalog(vec![source.clone()])
        .prepare(preparation(), 10)
        .unwrap();
    source.limits.context.reverse();
    let b = catalog(vec![source.clone()])
        .prepare(preparation(), 10)
        .unwrap();
    assert_eq!(a.generation(), b.generation());
    let mut bounded = Catalog::new(1);
    bounded.replace_sources(vec![source]).unwrap();
    let first = bounded.prepare(preparation(), 10).unwrap();
    let mut second_selection = preparation();
    second_selection.configuration_revision = "other-config".into();
    let second = bounded.prepare(second_selection, 10).unwrap();
    assert_ne!(first.generation(), second.generation());
    let first_again = bounded.prepare(preparation(), 10).unwrap();
    assert!(!Arc::ptr_eq(&first, &first_again));
    assert_eq!(first.generation(), first_again.generation());
}

#[test]
fn route_replacement_is_atomic_idempotent_and_preserves_other_routes() {
    let source = record();
    let mut catalog = catalog(vec![source.clone()]);
    let first = catalog.prepare(preparation(), 10).unwrap();
    catalog
        .replace_route(&identity(), vec![source.clone()])
        .unwrap();
    assert!(Arc::ptr_eq(
        &first,
        &catalog.prepare(preparation(), 10).unwrap()
    ));
    let mut other = source.clone();
    other.identity.deployment = Some("another-deployment".into());
    assert!(
        catalog
            .replace_route(&identity(), vec![other.clone()])
            .is_err()
    );
    catalog
        .replace_route(&other.identity.clone(), vec![other])
        .unwrap();
    assert!(Arc::ptr_eq(
        &first,
        &catalog.prepare(preparation(), 10).unwrap()
    ));
    catalog.replace_route(&identity(), vec![]).unwrap();
    assert!(catalog.prepare(preparation(), 10).is_err());
    assert_eq!(first.context().known().unwrap().tokens.get(), 1000);
}
