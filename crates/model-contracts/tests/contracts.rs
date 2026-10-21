use swarmie_model_contracts::*;
use tembra::{ContractToken, contracts::Contract};

#[test]
fn independent_provider_identities_share_canonical_shapes() -> tembra::Result<()> {
    let canonical = ModelProviderSchema.descriptor()?;
    for name in ["fixture.provider-one", "fixture.provider-two"] {
        let endpoint = ModelEndpoint::new(name)?.descriptor()?;
        assert_eq!(endpoint.members(""), canonical.members(""));
        assert_ne!(endpoint.fingerprint(), canonical.fingerprint());
        let roundtrip =
            Contract::from_json(&serde_json::to_string(endpoint.descriptor()).unwrap()).unwrap();
        assert_eq!(endpoint.fingerprint(), roundtrip.fingerprint());
    }
    let canonical = ModelGenerationSchema.descriptor()?;
    let endpoint = GenerationEndpoint::new("fixture.generations")?.descriptor()?;
    assert_eq!(canonical.members(""), endpoint.members(""));
    assert_eq!(canonical.input(), endpoint.input());
    Ok(())
}

#[test]
fn portable_requests_and_all_state_branches_validate() -> tembra::Result<()> {
    use tembra::contracts::{schema_for, to_portable_value, validate};
    let request = ModelRequest {
        request_id: "attempt-1".into(),
        model: ModelIdentity {
            connection: "local".into(),
            model: "small".into(),
        },
        messages: vec![Message::text(Role::User, "hello")],
        tools: vec![],
        options: Default::default(),
    };
    let schema = schema_for::<ModelRequest>().unwrap();
    validate(&schema, &to_portable_value(&request).unwrap()).unwrap();
    let schema = schema_for::<GenerationState>().unwrap();
    let state = GenerationState {
        request_id: request.request_id,
        phase: GenerationPhase::Prepared,
        acknowledged: 0,
        last_sequence: 1,
        events: vec![SequencedEvent {
            sequence: 1,
            event: ModelEvent::ToolCallSnapshot {
                id: "call_1".into(),
                name: "read".into(),
                arguments_json: "{\"path\":".into(),
            },
        }],
        response: None,
        error: Some(ProviderError {
            kind: ProviderErrorKind::Timeout,
            submission: Submission::MayHaveBeenSent,
            http_status: None,
            retry_after_ms: None,
        }),
        physically_settled: true,
    };
    validate(&schema, &to_portable_value(&state).unwrap()).unwrap();
    let mut bad = to_portable_value(&state).unwrap();
    bad["events"][0]["event"]["type"] = "invalid".into();
    assert!(validate(&schema, &bad).is_err());
    Ok(())
}
