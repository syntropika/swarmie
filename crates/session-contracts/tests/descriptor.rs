use swarmie_session_contracts::*;
use tembra::{ContractToken, contracts::Contract};

#[test]
fn canonical_descriptor_round_trips_and_preserves_portable_operations()
-> Result<(), Box<dyn std::error::Error>> {
    let descriptor = SessionStore.descriptor()?;
    let encoded = serde_json::to_string(descriptor.descriptor())?;
    let parsed = Contract::from_json(&encoded)?;
    assert_eq!(descriptor.fingerprint(), parsed.fingerprint());
    assert!(encoded.contains("swarmie.session-store"));
    for method in [
        "identity",
        "publication_status",
        "create",
        "open",
        "append",
        "events",
        "project",
        "committed",
    ] {
        assert!(encoded.contains(method));
    }
    Ok(())
}

#[test]
fn validation_refuses_unsafe_envelopes_and_preserves_future_payloads() {
    let base = NewEvent {
        event_id: "future".into(),
        kind: MESSAGE.into(),
        version: 2,
        correlation: Correlation::default(),
        occurred_at_ms: None,
        payload: serde_json::json!({"future":true}),
    };
    assert!(validate_event(&base).is_ok());
    let mut current = base.clone();
    current.version = 1;
    assert!(validate_event(&current).is_err());
    let mut invalid = base;
    invalid.event_id = " invalid ".into();
    assert!(validate_event(&invalid).is_err());
}
