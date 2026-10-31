use swarmie_ai::catalog::{Evidence, Layer, ModelIdentity, sources::ModelsDev};

fn evidence() -> Evidence {
    Evidence {
        source: "fixture".into(),
        revision: "1".into(),
        layer: Layer::Catalog,
        observed_at_ms: 10,
        expires_at_ms: None,
    }
}

#[test]
fn bundled_snapshot_is_integrity_checked_and_uses_real_provider_keys() {
    let source = ModelsDev::bundled().unwrap();
    let identity = ModelIdentity {
        provider: "openai".into(),
        adapter: "responses".into(),
        endpoint: "https://api.openai.com/v1/".into(),
        deployment: None,
        model: "gpt-5".into(),
        revision: None,
    };
    let record = source.bind(identity).unwrap().unwrap();
    assert!(!record.limits.context.is_empty());
    assert!(!record.limits.output.is_empty());
    assert!(record.limits.context[0].evidence.revision.contains(':'));
    let mut document: serde_json::Value =
        serde_json::from_slice(include_bytes!(concat!(env!("OUT_DIR"), "/models-dev.json")))
            .unwrap();
    document["content_sha256"] = "0".repeat(64).into();
    assert!(ModelsDev::from_snapshot(&serde_json::to_vec(&document).unwrap()).is_err());
}

#[test]
fn external_duplicate_keys_and_ambiguous_model_id_are_rejected() {
    for input in [
        br#"{"fixture":{"models":{}},"fixture":{"models":{}}}"#.as_slice(),
        br#"{"fixture":{"models":{"model":{"id":"model"},"model":{"id":"model"}}}}"#,
    ] {
        assert!(ModelsDev::from_json(input, evidence()).is_err());
    }
    assert!(
        ModelsDev::from_json(
            br#"{"fixture":{"models":{"model":{"id":"other"}}}}"#,
            evidence(),
        )
        .is_err()
    );
}

#[test]
fn api_compatible_routes_require_explicit_source_binding() {
    let source = ModelsDev::from_json(br#"{"publisher":{"models":{"fixture-model":{"id":"fixture-model","limit":{"context":1000,"output":100}}}}}"#, evidence()).unwrap();
    let identity = ModelIdentity {
        provider: "openai".into(),
        adapter: "fixture:chat".into(),
        endpoint: "https://publisher.example/v1/".into(),
        deployment: None,
        model: "fixture-model".into(),
        revision: None,
    };
    assert!(source.bind(identity.clone()).unwrap().is_none());
    let record = source
        .bind_provider(identity.clone(), "publisher")
        .unwrap()
        .unwrap();
    assert_eq!(record.identity, identity);
    assert_eq!(record.limits.context[0].tokens.get(), 1000);
}
