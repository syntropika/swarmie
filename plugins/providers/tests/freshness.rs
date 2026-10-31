use swarmie_model_catalog::{
    Capability, Catalog, Evidence, Layer, LimitClaim, Limits, ModelIdentity, Precision,
    Preparation, Requirements, SourceRecord,
};
use swarmie_providers::OperationalModel;

#[test]
fn safe_freshness_tracks_the_same_cached_snapshot_and_its_stale_margin() {
    let identity = ModelIdentity {
        provider: "fixture".into(),
        adapter: "chat".into(),
        endpoint: "https://provider.example/".into(),
        deployment: None,
        model: "one".into(),
        revision: None,
    };
    let claim = |tokens: u64| LimitClaim {
        tokens: tokens.try_into().unwrap(),
        precision: Precision::Reported,
        evidence: Evidence {
            source: "fixture-source".into(),
            revision: "one".into(),
            layer: Layer::Catalog,
            observed_at_ms: 10,
            expires_at_ms: Some(100),
        },
    };
    let mut catalog = Catalog::new(1);
    catalog
        .replace_sources(vec![SourceRecord {
            identity: identity.clone(),
            limits: Limits {
                context: vec![claim(1000)],
                input: vec![],
                output: vec![claim(100)],
            },
            tools: Capability::default(),
            streaming: Capability::default(),
            native_output_maximum: Capability::default(),
            native_compaction: Capability::default(),
        }])
        .unwrap();
    let preparation = Preparation {
        identity,
        configuration_revision: "one".into(),
        requirements: Requirements::default(),
        allow_stale_for_ms: 100,
    };
    let fresh = catalog.prepare(preparation.clone(), 50).unwrap();
    let descriptor = OperationalModel::from_snapshot(&fresh).unwrap();
    assert_eq!(descriptor.generation, fresh.generation());
    assert_eq!(
        descriptor.metadata_freshness.prepared_at_ms.milliseconds(),
        50
    );
    assert_eq!(
        descriptor
            .metadata_freshness
            .valid_until_ms
            .unwrap()
            .milliseconds(),
        100
    );
    assert_eq!(descriptor.metadata_freshness.stale_limit_evidence, 0);
    for until in [49_u64, 50] {
        let mut invalid = descriptor.metadata_freshness.clone();
        invalid.valid_until_ms = Some(until.into());
        assert!(invalid.validate().is_err());
    }
    let cached = catalog.prepare(preparation.clone(), 70).unwrap();
    assert!(std::sync::Arc::ptr_eq(&fresh, &cached));
    assert_eq!(
        OperationalModel::from_snapshot(&cached).unwrap(),
        descriptor
    );

    let stale = catalog.prepare(preparation.clone(), 100).unwrap();
    let stale_descriptor = OperationalModel::from_snapshot(&stale).unwrap();
    assert_eq!(stale_descriptor.generation, stale.generation());
    assert_eq!(
        stale_descriptor
            .metadata_freshness
            .prepared_at_ms
            .milliseconds(),
        100
    );
    assert_eq!(
        stale_descriptor
            .metadata_freshness
            .valid_until_ms
            .unwrap()
            .milliseconds(),
        200
    );
    assert_eq!(stale_descriptor.metadata_freshness.stale_limit_evidence, 1);
    assert!(stale.is_valid_at(199));
    assert!(!stale.is_valid_at(200));
    assert!(catalog.prepare(preparation, 200).is_err());
    // A later preparation does not mutate the earlier inspection's timing.
    assert_eq!(descriptor.metadata_freshness.stale_limit_evidence, 0);
    let encoded = serde_json::to_value(&stale_descriptor).unwrap();
    assert_eq!(encoded["metadata_freshness"]["prepared_at_ms"], "100");
    assert_eq!(encoded["metadata_freshness"]["valid_until_ms"], "200");
    assert!(
        serde_json::from_value::<OperationalModel>(encoded)
            .unwrap()
            .metadata_freshness
            .validate()
            .is_ok()
    );
}
