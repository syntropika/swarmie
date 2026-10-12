use mpai::mock::{MockHarness, MockScript, MockStep};
use mpai::*;
use serde_json::json;

#[test]
fn capability_states_and_provider_are_independent_of_execution_readiness() {
    let unknown = Capability::unknown("Session negotiation is required");
    let unsupported = Capability::default();
    let unavailable = Capability::unavailable("This session is ephemeral");
    for capability in [&unknown, &unsupported, &unavailable] {
        assert!(!capability.is_available());
        assert!(capability.reason.is_some());
        assert!(capability.require("resume").is_err());
    }
    assert_ne!(unknown.availability, unsupported.availability);
    assert_ne!(unavailable.availability, unsupported.availability);
    let capabilities = Capabilities::default();
    assert!(capabilities.queue.is_available());
    assert_eq!(capabilities.queue.source, CapabilitySource::Runtime);
    assert_eq!(
        Capability::negotiated(true).evidence,
        CapabilityEvidence::Negotiated
    );
    assert_eq!(json!(unknown)["availability"], "unknown");
    assert!(serde_json::from_value::<Capability>(json!(true)).is_err());
}

#[tokio::test]
async fn configuration_scopes_reject_creation_and_next_turn_changes_while_busy() {
    let runtime = Runtime::builder().harness(MockHarness::new(MockScript {
        configuration: vec![
            json!({"id":"creation", "label":"Creation", "type":"string", "value":"original", "application":"creation_only"}),
            json!({"id":"future", "label":"Future", "type":"boolean", "value":false, "application":"next_turn"}),
            json!({"id":"current", "label":"Current", "type":"boolean", "value":false, "application":"current_turn"}),
        ],
        ..Default::default()
    }.turn(vec![MockStep::Sleep { milliseconds: 2000 }]))).build();
    let agent = runtime
        .spawn(SpawnOptions::new("mock", std::env::current_dir().unwrap()))
        .await
        .unwrap();
    assert!(matches!(
        agent.configure(json!({"creation":"changed"})).await,
        Err(Error::Unsupported(_))
    ));
    assert!(matches!(
        agent.configure(json!({"future":"invalid"})).await,
        Err(Error::Invalid(_))
    ));
    agent.prompt("running").await.unwrap();
    assert!(matches!(
        agent.configure(json!({"future":true})).await,
        Err(Error::Busy)
    ));
    let receipt = agent.configure(json!({"current":true})).await.unwrap();
    assert_eq!(receipt.values["current"], true);
    assert_eq!(receipt.values["future"], false);
    assert_eq!(receipt.values["creation"], "original");
    assert!(agent.snapshot().await.turn.is_some());
    agent.close().await.unwrap();
}

#[test]
fn absent_setting_values_are_distinct_from_confirmed_null_resets() {
    let descriptor =
        json!({"id":"model", "label":"Model", "type":"string", "application":"next_turn"});
    let unknown: SettingDescriptor = serde_json::from_value(descriptor.clone()).unwrap();
    assert!(unknown.value.is_none());
    assert!(json!(unknown).get("value").is_none());
    let mut acknowledged = descriptor;
    acknowledged["value"] = serde_json::Value::Null;
    let reset: SettingDescriptor = serde_json::from_value(acknowledged).unwrap();
    assert_eq!(reset.value, Some(serde_json::Value::Null));
    assert!(json!(reset).as_object().unwrap().contains_key("value"));
}
