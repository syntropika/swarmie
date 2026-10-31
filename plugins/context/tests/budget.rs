use std::num::NonZeroU64;
use swarmie_context::{Budget, BudgetDecision, BudgetPolicy};
use swarmie_model_catalog::{Capability, Limit, ModelIdentity, Precision};
use swarmie_providers::PreparationPolicy;
use swarmie_providers::{
    Component, ComponentKind, CountPrecision, Measurement, MetadataFreshness, OperationId,
    OperationalModel, OutputReservation, PreparedDescription,
};

fn description(tokens: u64) -> PreparedDescription {
    let limit = |tokens| Limit {
        tokens: NonZeroU64::new(tokens).unwrap(),
        precision: Precision::Reported,
        claims: vec![],
        bounded_by_context: false,
    };
    PreparedDescription {
        operation: OperationId::new(),
        fingerprint: [1; 32],
        streaming: false,
        output_reserve: 20,
        output_reservation: OutputReservation::SentCap,
        model: OperationalModel {
            identity: ModelIdentity {
                provider: "fixture".into(),
                adapter: "fixture-v1".into(),
                endpoint: "https://provider.example/".into(),
                deployment: None,
                model: "one".into(),
                revision: None,
            },
            generation: "a".repeat(64),
            metadata_freshness: MetadataFreshness {
                prepared_at_ms: 0.into(),
                valid_until_ms: None,
                stale_limit_evidence: 0,
            },
            context: limit(100),
            input: limit(90),
            output: limit(20),
            native_compaction: Capability::default(),
            native_output_maximum: Capability::default(),
        },
        measurement: Measurement {
            input_tokens: tokens,
            final_payload_bytes: 400,
            precision: CountPrecision::Estimated,
            method: "fixture-estimate".into(),
            components: vec![Component {
                kind: ComponentKind::Conversation,
                tokens,
                bytes: 400,
            }],
        },
    }
}

#[test]
fn checkpoint_source_is_not_a_real_user_and_no_user_overflow_requires_an_owner_floor() {
    use swarmie_context::{ArtifactRef, Configuration, Plan, Projection, Strategy, textual};
    use swarmie_conversations::{ConversationId, EntryId, Head, HistoryPoint};
    use swarmie_providers::{AgentContext, Message, ModelRequest, SubmissionProtection, Usage};
    let conversation = ConversationId::new();
    let entry = EntryId::new();
    let projection = Projection {
        operation: OperationId::new(),
        plan: Plan {
            through: HistoryPoint {
                conversation,
                entry,
            },
            previous: None,
            configuration: Configuration([1; 32]),
            strategy: Strategy::Textual {
                model: Configuration([2; 32]),
            },
        },
        artifact: ArtifactRef::new(
            Head {
                conversation,
                entry: Some(EntryId::new()),
            },
            Some(entry),
        )
        .unwrap(),
    };
    let source = textual::project(
        &projection,
        &textual::Summary {
            content: "Large but compactable checkpoint".repeat(1000),
            usage: Usage::default(),
            protection: SubmissionProtection::Clean,
        },
    )
    .unwrap();
    let mut request = ModelRequest {
        context: AgentContext {
            sources: None,
            instructions: "Current instructions stay complete".into(),
            messages: vec![
                source.clone(),
                Message::User {
                    content: "The complete pending user input".into(),
                },
            ],
        },
        tools: vec![],
    };
    let policy = BudgetPolicy::new(10);
    let fixed = policy
        .fixed_request(&request, &description(85))
        .unwrap()
        .unwrap();
    assert_eq!(fixed.context.instructions, request.context.instructions);
    assert_eq!(fixed.tools, request.tools);
    assert_eq!(fixed.context.messages, request.context.messages[1..]);
    assert!(matches!(
        policy
            .decision(&description(85), Some(&description(50)))
            .unwrap(),
        BudgetDecision::CompactionRequired { .. }
    ));
    assert!(matches!(
        policy
            .decision(&description(85), Some(&description(85)))
            .unwrap(),
        BudgetDecision::InputTooLarge { .. }
    ));
    request.context.messages = vec![source];
    let original = request.clone();
    let unavailable = policy
        .fixed_request(&request, &description(85))
        .unwrap_err();
    assert_eq!(unavailable.code, tembra::Code::Unavailable);
    assert_eq!(unavailable.reason, "swarmie.context.fixed_budget");
    assert_eq!(request, original);
}

#[test]
fn effective_input_reserves_actual_output_and_headroom_and_keeps_precision() {
    let prepared = description(50);
    let budget = Budget::calculate(&prepared, 10).unwrap();
    assert_eq!(
        (
            budget.input_limit,
            budget.output_reserve,
            budget.headroom,
            budget.available,
            budget.excess
        ),
        (70, 20, 10, 20, 0)
    );
    assert_eq!(budget.precision, CountPrecision::Estimated);
    assert!(!budget.requires_compaction());
    let budget = Budget::calculate(&description(85), 10).unwrap();
    assert_eq!(budget.excess, 15);
    assert!(budget.requires_compaction());
}

#[test]
fn malformed_or_overflowing_reservations_and_attribution_never_admit() {
    assert!(Budget::calculate(&description(50), u64::MAX).is_err());
    assert!(Budget::calculate(&description(50), 90).is_err());
    let mut prepared = description(50);
    prepared.output_reserve = 21;
    assert!(Budget::calculate(&prepared, 0).is_err());
    prepared = description(50);
    prepared
        .measurement
        .components
        .push(prepared.measurement.components[0].clone());
    assert!(Budget::calculate(&prepared, 0).is_err());
    prepared = description(50);
    prepared.measurement.components[0].tokens = 49;
    assert!(Budget::calculate(&prepared, 0).is_err());
}

#[test]
fn fixed_input_overflow_is_distinct_from_compactable_history_and_missing_floor() {
    let policy = BudgetPolicy::new(10);
    assert!(matches!(
        policy.decision(&description(50), None).unwrap(),
        BudgetDecision::Ready { .. }
    ));
    assert!(policy.decision(&description(85), None).is_err());
    assert!(matches!(
        policy
            .decision(&description(85), Some(&description(50)))
            .unwrap(),
        BudgetDecision::CompactionRequired { .. }
    ));
    assert!(matches!(
        policy
            .decision(&description(85), Some(&description(85)))
            .unwrap(),
        BudgetDecision::InputTooLarge { .. }
    ));
    let mut wrong = description(50);
    wrong.model.identity.model = "other".into();
    assert!(policy.decision(&description(85), Some(&wrong)).is_err());
}

#[test]
fn inspection_floor_keeps_complete_pending_input_instructions_and_tools() {
    use swarmie_providers::{AgentContext, Message, ModelRequest};
    let request = ModelRequest {
        context: AgentContext {
            sources: Some(
                swarmie_providers::ContextSources::new(
                    swarmie_providers::SourceScope {
                        node: "node".into(),
                        principal: "user".into(),
                        directory: "project".into(),
                    },
                    vec![
                        swarmie_providers::SourcePart::new(
                            swarmie_providers::SourceKind::ProjectInstructions,
                            "id".into(),
                            "revision".into(),
                            "AGENTS.md".into(),
                            "Fixed source budget".into(),
                        )
                        .unwrap(),
                    ],
                )
                .unwrap()
                .at(1),
            ),
            instructions: "current instructions".into(),
            messages: vec![
                Message::User {
                    content: "old history".into(),
                },
                Message::User {
                    content: "🙂\"\\".repeat(1024),
                },
            ],
        },
        tools: vec![],
    };
    let fixed = BudgetPolicy::new(10)
        .fixed_request(&request, &description(85))
        .unwrap()
        .unwrap();
    assert_eq!(fixed.context.messages, request.context.messages[1..]);
    assert_eq!(fixed.context.instructions, request.context.instructions);
    assert_eq!(fixed.tools, request.tools);
    assert_eq!(fixed.context.sources.as_ref().unwrap().before(), 0);
    assert_eq!(
        fixed.context.sources.as_ref().unwrap().parts(),
        request.context.sources.as_ref().unwrap().parts()
    );
    assert!(
        serde_json::to_string(&fixed.context.model_messages().unwrap())
            .unwrap()
            .contains("Fixed source budget")
    );
    assert_eq!(request.context.messages.len(), 2);
}

#[test]
fn published_inspection_validates_arithmetic_and_preserves_stale_measurements() {
    use swarmie_context::{Inspection, Snapshot};
    use swarmie_conversations::{ConversationId, Head};
    let prepared = description(85);
    let inspection = Inspection {
        at: Head {
            conversation: ConversationId::new(),
            entry: None,
        },
        decision: BudgetPolicy::new(10)
            .decision(&prepared, Some(&description(50)))
            .unwrap(),
        prepared,
        stale: true,
    };
    let mut snapshot = Snapshot {
        inspection: Some(inspection),
        ..Snapshot::default()
    };
    snapshot.validate().unwrap();
    let encoded = serde_json::to_value(&snapshot).unwrap();
    let restored: Snapshot = serde_json::from_value(encoded).unwrap();
    assert_eq!(restored, snapshot);
    if let BudgetDecision::CompactionRequired { fixed, .. } =
        &mut snapshot.inspection.as_mut().unwrap().decision
    {
        fixed.available += 1;
    }
    assert!(snapshot.validate().is_err());
}

#[test]
fn native_reservation_requires_its_distinct_proof_and_cannot_hide_a_lower_policy_ceiling() {
    use swarmie_model_catalog::{Evidence, Layer, NativeOutputMaximum, OutputAccounting};
    let mut request = description(85);
    request.output_reservation = OutputReservation::NativeMaximum;
    assert!(request.validate().is_err());
    let evidence = |layer| Evidence {
        source: "fixture-source".into(),
        revision: "one".into(),
        layer,
        observed_at_ms: 0,
        expires_at_ms: None,
    };
    request.model.native_output_maximum = Capability::Supported {
        evidence: evidence(Layer::Adapter),
        value: NativeOutputMaximum {
            tokens: NonZeroU64::new(20).unwrap(),
            configuration_revision: "fixture-config".into(),
            fact: evidence(Layer::Provider),
            accounting: OutputAccounting::TotalGeneratedTokens,
        },
    };
    request.validate().unwrap();
    let policy = BudgetPolicy::new(0);
    let mut fixed = request.clone();
    fixed.measurement.input_tokens = 10;
    fixed.measurement.components[0].tokens = 10;
    assert!(matches!(
        policy.decision(&request, Some(&fixed)).unwrap(),
        BudgetDecision::CompactionRequired { .. }
    ));
    fixed.output_reservation = OutputReservation::SentCap;
    assert!(policy.decision(&request, Some(&fixed)).is_err());
    assert_ne!(request, {
        let mut changed = request.clone();
        changed.output_reservation = OutputReservation::SentCap;
        changed
    });
    let encoded = serde_json::to_value(&request).unwrap();
    assert_eq!(encoded["output_reservation"]["kind"], "native_maximum");
    assert_eq!(
        serde_json::from_value::<PreparedDescription>(encoded).unwrap(),
        request
    );

    let mut lower_reserve = request.clone();
    lower_reserve.output_reserve = 19;
    assert!(lower_reserve.validate().is_err());
    request.model.output.tokens = NonZeroU64::new(19).unwrap();
    assert!(request.validate().is_err());
}
