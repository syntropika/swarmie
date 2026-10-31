use serde_json::json;
use swarmie_model_catalog::{
    Capability, Evidence, Layer, ModelFeature as F, ModelFeatureStatus, ModelIdentity, ModelsDev,
};

fn evidence(layer: Layer, time: u64) -> Evidence {
    Evidence {
        source: "fixture".into(),
        revision: time.to_string(),
        layer,
        observed_at_ms: time,
        expires_at_ms: Some(time + 100),
    }
}

fn source(row: serde_json::Value) -> ModelsDev {
    ModelsDev::from_json(
        &serde_json::to_vec(&json!({"fixture":{"name":"Fixture","models":{"model":row}}})).unwrap(),
        evidence(Layer::Catalog, 10),
    )
    .unwrap()
}

fn identity() -> ModelIdentity {
    ModelIdentity {
        provider: "fixture".into(),
        adapter: "chat".into(),
        endpoint: "https://provider.example/v1/".into(),
        deployment: None,
        model: "model".into(),
        revision: None,
    }
}

#[test]
fn descriptive_facts_preserve_unknown_false_empty_zero_and_future_fields() {
    let source = source(
        json!({"id":"model","tool_call":false,"modalities":{"input":["text","image","future"],"output":[]},"cost":{"input":0,"tiers":[{"limit":200000,"output":3}]},"lifecycle":{"future":true}}),
    );
    let row = source.model("fixture", "model").unwrap();
    assert_eq!(row.spec.supports(F::Tools), Some(false));
    assert_eq!(row.spec.supports(F::StructuredOutput), None);
    assert_eq!(row.spec.supports(F::Input("future")), Some(true));
    assert_eq!(row.spec.supports(F::Input("audio")), Some(false));
    assert_eq!(row.spec.supports(F::Output("text")), Some(false));
    assert_eq!(row.spec.cost.as_ref().unwrap().input, Some(0.0));
    assert_eq!(row.spec.cost.as_ref().unwrap().output, None);
    assert_eq!(
        row.spec.cost.as_ref().unwrap().extra["tiers"][0]["output"],
        3
    );
    assert_eq!(row.spec.extra["lifecycle"]["future"], true);
    assert_eq!(source.provider("fixture").unwrap()["name"], "Fixture");
    assert_eq!(
        row.spec.require(&[F::Tools]).unwrap_err().status,
        ModelFeatureStatus::Unsupported
    );
    assert_eq!(
        row.spec.require(&[F::Reasoning]).unwrap_err().status,
        ModelFeatureStatus::Unknown
    );
    assert!(source.model("fixture", "model-2026-01-01").is_none());
}

#[test]
fn reasoning_and_cache_checks_use_reported_controls_without_guessed_bounds() {
    let source = source(
        json!({"id":"model","reasoning":true,"limit":{"output":1000},"reasoning_options":[{"type":"effort","values":["low","high","none"]},{"type":"budget_tokens","min":10,"max":100}],"cost":{"cache_read":1},"compatibility":{"prefill":false}}),
    );
    let row = source.model("fixture", "model").unwrap();
    for (feature, result) in [
        (F::ReasoningEffort("high"), Some(true)),
        (F::ReasoningEffort("medium"), Some(false)),
        (F::ReasoningBudget(50), Some(true)),
        (F::ReasoningBudget(101), Some(false)),
        (F::DisableReasoning, Some(true)),
        (F::CacheRetention("5m"), None),
        (F::Prefill, Some(false)),
    ] {
        assert_eq!(row.spec.supports(feature), result, "{feature:?}");
    }
    for min in [json!(null), json!(-1)] {
        let source = self::source(
            json!({"id":"model","reasoning":true,"limit":{"output":1000},"reasoning_options":[{"type":"budget_tokens","min":min,"max":100}]}),
        );
        assert_eq!(
            source
                .model("fixture", "model")
                .unwrap()
                .spec
                .supports(F::ReasoningBudget(50)),
            None
        );
    }
}

#[test]
fn reviewed_fields_survive_refresh_and_binding_retains_each_fields_evidence() {
    let original = source(
        json!({"id":"model","tool_call":true,"limit":{"context":1000,"output":100},"cost":{"input":1,"output":2}}),
    );
    let overridden = original.with_overrides(br#"{"fixture":{"models":{"model":{"tool_call":false,"limit":{"output":50},"cost":null}}}}"#,evidence(Layer::Configuration,20)).unwrap();
    let refreshed = overridden.with_models_dev(br#"{"fixture":{"models":{"model":{"id":"model","tool_call":true,"limit":{"context":2000,"output":200},"cost":{"input":3}}}}}"#,evidence(Layer::Catalog,30)).unwrap();
    let row = refreshed.model("fixture", "model").unwrap();
    assert_eq!(row.spec.tools, Some(false));
    assert!(row.spec.cost.is_none());
    assert_eq!(row.evidence("/tool_call").unwrap().observed_at_ms, 20);
    let bound = refreshed.bind(identity()).unwrap().unwrap();
    assert_eq!(bound.limits.context[0].tokens.get(), 2000);
    assert_eq!(bound.limits.context[0].evidence.observed_at_ms, 30);
    assert_eq!(bound.limits.output[0].tokens.get(), 50);
    assert_eq!(bound.limits.output[0].evidence.layer, Layer::Configuration);
    assert!(matches!(bound.tools, Capability::Unknown { .. }));
    assert_eq!(
        original
            .model("fixture", "model")
            .unwrap()
            .spec
            .limit
            .output,
        Some(100)
    );
    assert!(
        overridden
            .with_models_dev(br#"{}"#, evidence(Layer::Catalog, 10))
            .is_err()
    );
    let mut revised = identity();
    revised.revision = Some("v1".into());
    assert!(refreshed.bind(revised).unwrap().is_none());
}

#[test]
fn refresh_does_not_freshen_missing_fields_and_invalid_overrides_are_atomic() {
    let source = source(json!({"id":"model","limit":{"context":1000,"output":100}}));
    let refreshed = source
        .with_models_dev(
            br#"{"fixture":{"models":{"model":{"id":"model","limit":{"context":2000}}}}}"#,
            evidence(Layer::Catalog, 20),
        )
        .unwrap();
    assert_eq!(
        refreshed
            .model("fixture", "model")
            .unwrap()
            .evidence("/limit/output")
            .unwrap()
            .observed_at_ms,
        10
    );
    for patch in [
        r#"{"fixture":{"models":{"model":{"limit":{"output":-1}}}}}"#,
        r#"{"fixture":{"models":{"model":{"id":"other"}}}}"#,
        r#"{"fixture":{"models":{"model":{"cost":{"input":-1}}}}}"#,
        r#"{"fixture":{"models":{"model":{"extra":{"a":1,"a":2}}}}}"#,
        r#"{"fixture":{"models":{"model":{"compatibility":{"typo":true}}}}}"#,
        r#"{"fixture":{"models":{"model":{"modalities":{"input":true}}}}}"#,
    ] {
        assert!(
            source
                .with_overrides(patch.as_bytes(), evidence(Layer::Configuration, 20))
                .is_err(),
            "{patch}"
        );
    }
    assert_eq!(
        source.model("fixture", "model").unwrap().spec.limit.context,
        Some(1000)
    );
    assert!(
        source
            .with_overrides(br#"{}"#, evidence(Layer::Adapter, 20))
            .is_err()
    );
}

#[test]
fn bundled_index_is_shared_and_order_is_stable() {
    let first = ModelsDev::bundled().unwrap();
    let second = ModelsDev::bundled().unwrap();
    assert!(std::ptr::eq(
        first.model("openai", "gpt-5").unwrap().spec,
        second.model("openai", "gpt-5").unwrap().spec
    ));
    let identities: Vec<_> = first
        .models()
        .map(|row| (row.provider.to_owned(), row.spec.id.clone()))
        .collect();
    assert!(identities.windows(2).all(|pair| pair[0] < pair[1]));
}
