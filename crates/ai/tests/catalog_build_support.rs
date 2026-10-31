#[path = "../catalog/build_support.rs"]
mod build_support;

use build_support::{MAX_BYTES, SOURCE_URL, snapshot};
use serde_json::{Value, json};
use swarmie_ai::catalog::sources::ModelsDev;

fn input() -> Value {
    json!({"fixture": {"models": {"model": {
        "id": "model", "limit": {"context": 1000, "output": 100}, "tool_call": true
    }}}})
}

fn make(input: &Value) -> Value {
    serde_json::from_slice(&snapshot(&serde_json::to_vec(input).unwrap(), 1234).unwrap()).unwrap()
}

#[test]
fn snapshot_retains_model_facts_and_provenance() {
    let result = make(&input());
    assert_eq!(result["source_url"], SOURCE_URL);
    assert_eq!(result["fetched_at_ms"], 1234);
    assert_eq!(result["expires_at_ms"], 1234 + 7 * 24 * 60 * 60 * 1000);
    assert_eq!(
        result["providers"]["fixture"]["models"]["model"],
        input()["fixture"]["models"]["model"]
    );
    assert_eq!(result["upstream_sha256"].as_str().unwrap().len(), 64);
    assert!(ModelsDev::from_snapshot(&serde_json::to_vec(&result).unwrap()).is_ok());
}

#[test]
fn malformed_limits_identities_and_duplicate_keys_fail_validation() {
    for value in [json!(-1), json!(true), json!(1.5), json!("100")] {
        let mut data = input();
        data["fixture"]["models"]["model"]["limit"]["context"] = value;
        assert!(snapshot(&serde_json::to_vec(&data).unwrap(), 1234).is_err());
    }
    for raw in [
        r#"{}"#,
        r#"{"fixture":{"models":{}},"fixture":{"models":{}}}"#,
        r#"{"fixture":{"models":{"model":{"id":"model"},"model":{"id":"model"}}}}"#,
        r#"{"fixture":{"models":{"model":{"id":"other"}}}}"#,
        r#"{"fixture":{"models":{"model":{"id":"model","limit":{"context":18446744073709551616}}}}}"#,
        r#"{"fixture":{"models":{"model":{"id":"model","limit":{"context":10,"context":20}}}}}"#,
        r#"{"\u0000":{"models":{}}}"#,
    ] {
        assert!(snapshot(raw.as_bytes(), 1234).is_err(), "{raw}");
    }
}

#[test]
fn zero_and_missing_limits_remain_unknown() {
    let mut data = input();
    data["fixture"]["models"]["model"]["limit"] = json!({"context": 0});
    assert_eq!(
        make(&data)["providers"]["fixture"]["models"]["model"]["limit"],
        json!({"context": 0})
    );
}

#[test]
fn normalization_is_independent_of_source_field_order() {
    let first =
        br#"{"fixture":{"models":{"model":{"id":"model","limit":{"output":100,"context":1000}}}}}"#;
    let second =
        br#"{"fixture":{"models":{"model":{"limit":{"context":1000,"output":100},"id":"model"}}}}"#;
    let parse =
        |raw: &[u8]| -> Value { serde_json::from_slice(&snapshot(raw, 1234).unwrap()).unwrap() };
    assert_eq!(
        parse(first)["content_sha256"],
        parse(second)["content_sha256"]
    );
    assert_ne!(
        parse(first)["upstream_sha256"],
        parse(second)["upstream_sha256"]
    );
}

#[test]
fn bounded_input_and_timestamp_are_required() {
    assert!(snapshot(&vec![b' '; MAX_BYTES as usize + 1], 1234).is_err());
    let raw = serde_json::to_vec(&input()).unwrap();
    assert!(snapshot(&raw, u64::MAX).is_err());
}

#[test]
fn rich_facts_survive_build_normalization_and_integrity_covers_extra_fields() {
    let data = json!({"fixture":{"name":"Fixture","models":{"model":{
        "id":"model","name":"Model","tool_call":true,"reasoning":true,
        "modalities":{"input":["text","image"],"output":["text"],"future":true},
        "reasoning_options":[{"type":"budget_tokens","min":-1,"max":100}],
        "interleaved":{"field":"reasoning_content","future":true},
        "cost":{"input":0,"output":2,"tiers":[{"future":true}]},
        "future":{"nested":[1,2]}
    }}}});
    let mut snapshot = make(&data);
    let source = ModelsDev::from_snapshot(&serde_json::to_vec(&snapshot).unwrap()).unwrap();
    let model = source.model("fixture", "model").unwrap();
    assert_eq!(
        model.spec.modalities.as_ref().unwrap().extra["future"],
        true
    );
    assert_eq!(model.spec.extra["future"]["nested"], json!([1, 2]));
    assert_eq!(model.spec.cost.as_ref().unwrap().input, Some(0.0));
    assert_eq!(
        model.spec.reasoning_options.as_ref().unwrap()[0].min,
        Some(-1)
    );
    snapshot["providers"]["fixture"]["models"]["model"]["future"]["nested"] = json!([3]);
    assert!(ModelsDev::from_snapshot(&serde_json::to_vec(&snapshot).unwrap()).is_err());
}
