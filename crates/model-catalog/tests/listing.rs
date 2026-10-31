use swarmie_model_catalog::{ListingProtocol as P, ModelListing};

#[test]
fn parses_protocol_pages_without_inventing_capabilities_or_aliases() {
    for (protocol, raw, id, input, output) in [
        (
            P::OpenAi,
            r#"{"data":[{"id":"model","owned_by":"fixture","future":true}]}"#,
            "model",
            None,
            None,
        ),
        (
            P::Anthropic,
            r#"{"data":[{"id":"model","display_name":"Model","max_input_tokens":1000,"max_tokens":100,"capabilities":{"thinking":{"supported":true}}}],"has_more":true,"last_id":"model"}"#,
            "model",
            Some(1000),
            Some(100),
        ),
        (
            P::Gemini,
            r#"{"models":[{"name":"models/model-v1","displayName":"Model","inputTokenLimit":1000,"outputTokenLimit":100,"supportedGenerationMethods":["generateContent"]}],"nextPageToken":"next"}"#,
            "models/model-v1",
            Some(1000),
            Some(100),
        ),
        (
            P::Ollama,
            r#"{"models":[{"model":"model:latest","name":"Model","digest":"fixture","details":{"family":"fixture"}}]}"#,
            "model:latest",
            None,
            None,
        ),
    ] {
        let listing = ModelListing::from_json(protocol, raw.as_bytes()).unwrap();
        assert_eq!(listing.models[0].id, id);
        assert_eq!(listing.models[0].limits.input, input);
        assert_eq!(listing.models[0].limits.output, output);
        assert_eq!(listing.models[0].limits.context, None);
        assert_eq!(
            listing.raw,
            serde_json::from_str::<serde_json::Value>(raw).unwrap()
        );
        if protocol == P::Gemini {
            assert_eq!(listing.next_page_token.as_deref(), Some("next"));
        }
        if protocol == P::Anthropic {
            assert_eq!(listing.has_more, Some(true));
            assert_eq!(listing.last_id.as_deref(), Some("model"));
        }
    }
}

#[test]
fn refuses_ambiguous_and_malformed_pages() {
    for (protocol, raw) in [
        (P::OpenAi, r#"{"data":[{"id":"model"},{"id":"model"}]}"#),
        (P::OpenAi, r#"{"data":[{"id":""}]}"#),
        (
            P::OpenAi,
            r#"{"data":[{"id":"model","extra":{"x":1,"x":2}}]}"#,
        ),
        (P::Anthropic, r#"{"data":[],"has_more":"yes"}"#),
        (
            P::Gemini,
            r#"{"models":[{"name":"models/model","inputTokenLimit":-1}]}"#,
        ),
        (P::Gemini, r#"{"models":[],"nextPageToken":true}"#),
        (P::Ollama, r#"{"models":[{"name":"model"}]}"#),
    ] {
        assert!(
            ModelListing::from_json(protocol, raw.as_bytes()).is_err(),
            "{raw}"
        );
    }
    assert!(
        ModelListing::from_json(P::OpenAi, br#"{"data":[]}"#)
            .unwrap()
            .models
            .is_empty()
    );
    let zero = ModelListing::from_json(
        P::Gemini,
        br#"{"models":[{"name":"models/model","outputTokenLimit":0}]}"#,
    )
    .unwrap();
    assert_eq!(zero.models[0].limits.output, None);
    assert_eq!(zero.models[0].raw["outputTokenLimit"], 0);
}
