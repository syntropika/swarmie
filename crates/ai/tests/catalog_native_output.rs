use std::sync::Arc;
use swarmie_ai::catalog::{
    Capability, Catalog, Evidence, Layer, LimitClaim, Limits, ModelIdentity, NativeOutputMaximum,
    OutputAccounting, Precision, Preparation, Requirements, ResolutionError, SourceRecord,
    UnknownReason,
};

fn evidence(layer: Layer) -> Evidence {
    Evidence {
        source: "native-output-fixture".into(),
        revision: "1".into(),
        layer,
        observed_at_ms: 10,
        expires_at_ms: None,
    }
}

fn preparation() -> Preparation {
    Preparation {
        identity: ModelIdentity {
            provider: "fixture".into(),
            adapter: "fixture-profile".into(),
            endpoint: "https://provider.example/v1/".into(),
            deployment: None,
            model: "fixture-model".into(),
            revision: None,
        },
        configuration_revision: "exact-profile-1".into(),
        requirements: Requirements::default(),
        allow_stale_for_ms: 0,
    }
}

fn maximum() -> NativeOutputMaximum {
    NativeOutputMaximum {
        tokens: 100.try_into().unwrap(),
        configuration_revision: preparation().configuration_revision,
        fact: evidence(Layer::Provider),
        accounting: OutputAccounting::TotalGeneratedTokens,
    }
}

fn record() -> SourceRecord {
    let claim = |tokens: u64| LimitClaim {
        tokens: tokens.try_into().unwrap(),
        precision: Precision::Reported,
        evidence: evidence(Layer::Provider),
    };
    SourceRecord {
        identity: preparation().identity,
        limits: Limits {
            context: vec![claim(1000)],
            output: vec![claim(100)],
            ..Limits::default()
        },
        tools: Capability::default(),
        streaming: Capability::default(),
        native_compaction: Capability::default(),
        native_output_maximum: Capability::Supported {
            evidence: evidence(Layer::Adapter),
            value: maximum(),
        },
    }
}

fn catalog(records: Vec<SourceRecord>) -> Catalog {
    let mut catalog = Catalog::new(8);
    catalog.replace_sources(records).unwrap();
    catalog
}

#[test]
fn effective_output_policy_and_context_never_create_or_reduce_a_native_maximum() {
    let mut source = record();
    source.limits.output.push(LimitClaim {
        tokens: 20.try_into().unwrap(),
        precision: Precision::Conservative,
        evidence: evidence(Layer::Policy),
    });
    let snapshot = catalog(vec![source.clone()])
        .prepare(preparation(), 10)
        .unwrap();
    assert_eq!(snapshot.output().known().unwrap().tokens.get(), 20);
    assert!(matches!(
        snapshot.native_output_maximum(),
        Capability::Supported { value, .. } if value.tokens.get() == 100
    ));
    source.native_output_maximum = Capability::default();
    let absent = catalog(vec![source]).prepare(preparation(), 10).unwrap();
    assert_eq!(absent.output().known().unwrap().tokens.get(), 20);
    assert!(matches!(
        absent.native_output_maximum(),
        Capability::Unknown { .. }
    ));
}

#[test]
fn native_maximum_requires_separate_provider_fact_and_adapter_qualification() {
    for layer in [
        Layer::Catalog,
        Layer::Configuration,
        Layer::Policy,
        Layer::Adapter,
    ] {
        let mut source = record();
        let Capability::Supported { value, .. } = &mut source.native_output_maximum else {
            unreachable!()
        };
        value.fact.layer = layer;
        assert_eq!(
            Catalog::new(1).replace_sources(vec![source]),
            Err(ResolutionError::InvalidEvidence)
        );
    }
    for layer in [
        Layer::Catalog,
        Layer::Provider,
        Layer::Deployment,
        Layer::Configuration,
        Layer::Policy,
    ] {
        let mut source = record();
        let Capability::Supported { evidence, .. } = &mut source.native_output_maximum else {
            unreachable!()
        };
        evidence.layer = layer;
        assert_eq!(
            Catalog::new(1).replace_sources(vec![source]),
            Err(ResolutionError::InvalidEvidence)
        );
    }
    let mut deployment = record();
    let Capability::Supported { value, .. } = &mut deployment.native_output_maximum else {
        unreachable!()
    };
    value.fact.layer = Layer::Deployment;
    assert!(matches!(
        catalog(vec![deployment])
            .prepare(preparation(), 10)
            .unwrap()
            .native_output_maximum(),
        Capability::Supported { .. }
    ));
}

#[test]
fn exact_route_and_configuration_are_required_even_with_equal_numeric_limits() {
    for change in 0..6 {
        let mut selection = preparation();
        selection.requirements.context = false;
        selection.requirements.input = false;
        selection.requirements.output = false;
        match change {
            0 => selection.identity.provider = "other".into(),
            1 => selection.identity.adapter = "other-profile".into(),
            2 => selection.identity.endpoint = "https://other.example/v1/".into(),
            3 => selection.identity.deployment = Some("other".into()),
            4 => selection.identity.model = "other".into(),
            _ => selection.identity.revision = Some("other".into()),
        }
        assert!(matches!(
            catalog(vec![record()])
                .prepare(selection, 10)
                .unwrap()
                .native_output_maximum(),
            Capability::Unknown { .. }
        ));
    }
    let mut selection = preparation();
    selection.configuration_revision = "changed-auth-profile".into();
    assert!(matches!(
        catalog(vec![record()]).prepare(selection, 10),
        Err(ResolutionError::ConfigurationMismatch(
            "native_output_maximum"
        ))
    ));
}

#[test]
fn both_fact_and_qualification_expire_without_stale_limit_grace() {
    for inner in [true, false] {
        let mut source = record();
        let Capability::Supported { evidence, value } = &mut source.native_output_maximum else {
            unreachable!()
        };
        evidence.expires_at_ms = Some(if inner { 30 } else { 20 });
        value.fact.expires_at_ms = Some(if inner { 20 } else { 30 });
        let mut selection = preparation();
        selection.allow_stale_for_ms = 1000;
        let mut catalog = catalog(vec![source]);
        let fresh = catalog.prepare(selection.clone(), 10).unwrap();
        assert_eq!(fresh.valid_until_ms(), Some(20));
        assert!(matches!(
            fresh.native_output_maximum(),
            Capability::Supported { .. }
        ));
        let expired = catalog.prepare(selection, 20).unwrap();
        assert!(matches!(
            expired.native_output_maximum(),
            Capability::Unknown {
                reason: UnknownReason::Expired
            }
        ));
        assert!(expired.stale_evidence().is_empty());
        assert_ne!(fresh.generation(), expired.generation());
    }
}

#[test]
fn future_inner_or_outer_evidence_reprepares_at_its_start_time() {
    for inner in [true, false] {
        let mut source = record();
        let Capability::Supported { evidence, value } = &mut source.native_output_maximum else {
            unreachable!()
        };
        if inner {
            value.fact.observed_at_ms = 50;
        } else {
            evidence.observed_at_ms = 50;
        }
        let mut catalog = catalog(vec![source]);
        let future = catalog.prepare(preparation(), 10).unwrap();
        assert_eq!(future.valid_until_ms(), Some(50));
        assert!(matches!(
            future.native_output_maximum(),
            Capability::Unknown {
                reason: UnknownReason::NotYetValid
            }
        ));
        let ready = catalog.prepare(preparation(), 50).unwrap();
        assert!(matches!(
            ready.native_output_maximum(),
            Capability::Supported { .. }
        ));
        assert_ne!(future.generation(), ready.generation());
    }
}

#[test]
fn conflicting_positive_or_negative_claims_fail_in_both_source_orders() {
    let yes = record();
    let mut different = record();
    let Capability::Supported { value, .. } = &mut different.native_output_maximum else {
        unreachable!()
    };
    value.tokens = 90.try_into().unwrap();
    let mut no = record();
    no.native_output_maximum = Capability::Unsupported {
        evidence: evidence(Layer::Adapter),
        reason: "this exact profile has no qualified uncapped maximum".into(),
    };
    for other in [different, no] {
        for records in [
            vec![yes.clone(), other.clone()],
            vec![other.clone(), yes.clone()],
        ] {
            assert!(matches!(
                catalog(records).prepare(preparation(), 10),
                Err(ResolutionError::ConflictingCapability(
                    "native_output_maximum"
                ))
            ));
        }
    }
}

#[test]
fn existing_cache_reuses_one_snapshot_and_includes_nested_facts_in_generation() {
    let mut source = record();
    let mut catalog = catalog(vec![source.clone()]);
    let first = catalog.prepare(preparation(), 10).unwrap();
    for now in 11..1011 {
        assert!(Arc::ptr_eq(
            &first,
            &catalog.prepare(preparation(), now).unwrap()
        ));
    }
    let Capability::Supported { value, .. } = &mut source.native_output_maximum else {
        unreachable!()
    };
    value.fact.revision = "new-authoritative-fact".into();
    catalog.replace_sources(vec![source]).unwrap();
    let next = catalog.prepare(preparation(), 10).unwrap();
    assert_ne!(first.generation(), next.generation());
    assert!(
        matches!(first.native_output_maximum(), Capability::Supported { value, .. } if value.fact.revision == "1")
    );
}

#[test]
fn malformed_facts_or_configuration_do_not_replace_a_valid_source() {
    let mut catalog = catalog(vec![record()]);
    let first = catalog.prepare(preparation(), 10).unwrap();
    for malformed in 0..4 {
        let mut source = record();
        let Capability::Supported { value, .. } = &mut source.native_output_maximum else {
            unreachable!()
        };
        match malformed {
            0 => value.configuration_revision.clear(),
            1 => value.configuration_revision = "bad\nrevision".into(),
            2 => value.fact.source.clear(),
            _ => value.fact.expires_at_ms = Some(10),
        }
        assert!(catalog.replace_sources(vec![source]).is_err());
        assert!(Arc::ptr_eq(
            &first,
            &catalog.prepare(preparation(), 10).unwrap()
        ));
    }
}

#[test]
fn zero_overflow_and_partial_accounting_are_not_native_maximum_proofs() {
    let encoded = serde_json::to_value(maximum()).unwrap();
    let mut zero = encoded.clone();
    zero["tokens"] = serde_json::json!(0);
    assert!(serde_json::from_value::<NativeOutputMaximum>(zero).is_err());
    let overflow = serde_json::to_string(&encoded)
        .unwrap()
        .replace("\"tokens\":100", "\"tokens\":18446744073709551616");
    assert!(serde_json::from_str::<NativeOutputMaximum>(&overflow).is_err());
    let mut partial = encoded;
    partial["accounting"] = serde_json::json!("visible_text_only");
    assert!(serde_json::from_value::<NativeOutputMaximum>(partial).is_err());
}
