use swarmie_context::*;
use swarmie_conversations::{ConversationId, EntryId, Head, HistoryPoint};
use swarmie_providers::{OperationId, Recovery};

fn request() -> Request {
    Request {
        operation: OperationId::new(),
        plan: Plan {
            through: HistoryPoint {
                conversation: ConversationId::new(),
                entry: EntryId::new(),
            },
            previous: None,
            configuration: Configuration([1; 32]),
            strategy: Strategy::Textual {
                model: Configuration([2; 32]),
            },
        },
        required: false,
    }
}

#[test]
fn a_recognized_unusable_paid_result_retains_its_artifact_without_replay_or_activation() {
    let mut input = request();
    input.required = true;
    let result = candidate(&input);
    let mut state = Checkpoint::default();
    state.request(input.clone()).unwrap();
    state.dispatch(input.operation).unwrap();
    state.unknown(input.operation).unwrap();
    state
        .reject(
            input.operation,
            result.clone(),
            "The complete next request still exceeds its budget".into(),
        )
        .unwrap();
    let state = restore(&state);
    state.validate().unwrap();
    assert!(state.compaction().unwrap().required);
    assert_eq!(
        state.compaction().unwrap().phase,
        Phase::Rejected {
            artifact: result.artifact,
            reason: "The complete next request still exceeds its budget".into()
        }
    );
    let mut unchanged = state.clone();
    assert!(unchanged.dispatch(input.operation).is_err());
    assert!(
        unchanged
            .activate(input.operation, boundary(&input))
            .is_err()
    );
    assert_eq!(unchanged, state);
    let mut stopped = Checkpoint::default();
    stopped.request(input.clone()).unwrap();
    stopped.dispatch(input.operation).unwrap();
    stopped.stop(input.operation).unwrap();
    stopped
        .reject(input.operation, result.clone(), "Unusable summary".into())
        .unwrap();
    assert_eq!(
        stopped.compaction().unwrap().phase,
        Phase::Stopped {
            artifact: Some(result.artifact)
        }
    );
}

#[test]
fn artifact_locators_require_an_entry_and_cannot_follow_themselves() {
    let conversation = ConversationId::new();
    let entry = EntryId::new();
    assert!(
        ArtifactRef::new(
            Head {
                conversation,
                entry: None
            },
            None
        )
        .is_err()
    );
    assert!(
        ArtifactRef::new(
            Head {
                conversation,
                entry: Some(entry)
            },
            Some(entry)
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<ArtifactRef>(
            serde_json::json!({"at":{"conversation":conversation,"entry":null},"after":null})
        )
        .is_err()
    );
    let valid = ArtifactRef::new(
        Head {
            conversation,
            entry: Some(entry),
        },
        Some(EntryId::new()),
    )
    .unwrap();
    let restored: ArtifactRef =
        serde_json::from_value(serde_json::to_value(valid).unwrap()).unwrap();
    assert_eq!(restored, valid);
}

fn restore(checkpoint: &Checkpoint) -> Checkpoint {
    serde_json::from_slice(&serde_json::to_vec(checkpoint).unwrap()).unwrap()
}

fn candidate(request: &Request) -> Candidate {
    Candidate {
        operation: request.operation,
        plan: request.plan.clone(),
        artifact: ArtifactRef::new(
            Head {
                conversation: request.plan.through.conversation,
                entry: Some(EntryId::new()),
            },
            Some(request.plan.through.entry),
        )
        .unwrap(),
    }
}

fn boundary(request: &Request) -> Activation {
    Activation {
        head: Head {
            conversation: request.plan.through.conversation,
            entry: Some(EntryId::new()),
        },
        configuration: request.plan.configuration,
        prefix_is_visible: true,
        execution_is_safe: true,
        request_fits: true,
    }
}

#[test]
fn automatic_and_manual_requests_join_one_operation_and_promote_required_work() {
    let request = request();
    let mut state = Checkpoint::default();
    assert_eq!(
        state.request(request.clone()).unwrap(),
        Requested::Started(request.operation)
    );
    state.dispatch(request.operation).unwrap();
    let mut automatic = request.clone();
    automatic.operation = OperationId::new();
    automatic.required = true;
    automatic.plan.through.entry = EntryId::new();
    assert_eq!(
        state.request(automatic).unwrap(),
        Requested::Joined(request.operation)
    );
    assert!(state.compaction().unwrap().required);
    assert!(state.dispatch(request.operation).is_err());
}

#[test]
fn restart_after_dispatch_requires_recognition_and_never_admits_a_replacement() {
    let request = request();
    let mut state = Checkpoint::default();
    state.request(request.clone()).unwrap();
    state.dispatch(request.operation).unwrap();
    let mut state = restore(&state);
    assert!(state.dispatch(request.operation).is_err());
    state.unknown(request.operation).unwrap();
    let mut replacement = request.clone();
    replacement.plan.strategy = Strategy::Native {
        qualification: Configuration([3; 32]),
    };
    assert!(state.request(replacement).is_err());
    state
        .recognize(request.operation, Recovery::Unknown)
        .unwrap();
    assert!(state.dispatch(request.operation).is_err());
    state
        .recognize(request.operation, Recovery::SafeToRetry)
        .unwrap();
    state.dispatch(request.operation).unwrap();
}

#[test]
fn stop_before_dispatch_is_terminal_and_after_dispatch_retains_uncertainty() {
    let request = request();
    let mut planned = Checkpoint::default();
    planned.request(request.clone()).unwrap();
    planned.stop(request.operation).unwrap();
    assert_eq!(
        planned.compaction().unwrap().phase,
        Phase::Stopped { artifact: None }
    );
    assert!(planned.dispatch(request.operation).is_err());

    let mut dispatched = Checkpoint::default();
    dispatched.request(request.clone()).unwrap();
    dispatched.dispatch(request.operation).unwrap();
    dispatched.stop(request.operation).unwrap();
    let mut dispatched = restore(&dispatched);
    assert_eq!(dispatched.compaction().unwrap().phase, Phase::Unknown);
    assert!(dispatched.dispatch(request.operation).is_err());
    let result = candidate(&request);
    dispatched
        .recognize(request.operation, Recovery::Completed(result.clone()))
        .unwrap();
    assert_eq!(
        dispatched.compaction().unwrap().phase,
        Phase::Stopped {
            artifact: Some(result.artifact)
        }
    );
    assert!(
        dispatched
            .activate(request.operation, boundary(&request))
            .is_err()
    );
    assert!(dispatched.active().is_none());
}

#[test]
fn safe_retry_after_stop_recognizes_no_replay_permission() {
    let request = request();
    let mut state = Checkpoint::default();
    state.request(request.clone()).unwrap();
    state.dispatch(request.operation).unwrap();
    state.stop(request.operation).unwrap();
    state
        .recognize(request.operation, Recovery::SafeToRetry)
        .unwrap();
    assert_eq!(
        state.compaction().unwrap().phase,
        Phase::Stopped { artifact: None }
    );
    assert!(state.dispatch(request.operation).is_err());
}

#[test]
fn activation_keeps_fixed_prefix_after_append_and_requires_safe_current_configuration() {
    let request = request();
    let mut state = Checkpoint::default();
    state.request(request.clone()).unwrap();
    state.dispatch(request.operation).unwrap();
    state
        .recognize(request.operation, Recovery::Completed(candidate(&request)))
        .unwrap();
    let at = boundary(&request);
    let mut changed = at;
    changed.configuration = Configuration([9; 32]);
    assert!(state.activate(request.operation, changed).is_err());
    changed = at;
    changed.execution_is_safe = false;
    assert!(state.activate(request.operation, changed).is_err());
    changed = at;
    changed.prefix_is_visible = false;
    assert!(state.activate(request.operation, changed).is_err());
    changed = at;
    changed.request_fits = false;
    assert!(state.activate(request.operation, changed).is_err());
    changed = at;
    changed.head.conversation = ConversationId::new();
    assert!(state.activate(request.operation, changed).is_err());
    let active = state.activate(request.operation, at).unwrap();
    assert_eq!(active.plan.through, request.plan.through);
    assert_ne!(Some(active.plan.through.entry), at.head.entry);
    assert_eq!(state.active(), Some(&active));
    assert!(state.activate(request.operation, at).is_err());
}

#[test]
fn candidate_must_match_operation_prefix_and_strategy_before_mutation() {
    let request = request();
    let mut state = Checkpoint::default();
    state.request(request.clone()).unwrap();
    state.dispatch(request.operation).unwrap();
    let before = state.clone();
    let mut wrong = candidate(&request);
    wrong.operation = OperationId::new();
    assert!(
        state
            .recognize(request.operation, Recovery::Completed(wrong))
            .is_err()
    );
    let mut wrong = candidate(&request);
    wrong.plan.through.entry = EntryId::new();
    assert!(
        state
            .recognize(request.operation, Recovery::Completed(wrong))
            .is_err()
    );
    assert_eq!(state, before);
}

#[test]
fn repeated_compaction_selects_previous_artifact_without_transcript_in_checkpoint() {
    let mut request = request();
    let mut state = Checkpoint::default();
    for _ in 0..8 {
        state.request(request.clone()).unwrap();
        state.dispatch(request.operation).unwrap();
        let result = candidate(&request);
        state
            .recognize(request.operation, Recovery::Completed(result.clone()))
            .unwrap();
        state
            .activate(request.operation, boundary(&request))
            .unwrap();
        request.operation = OperationId::new();
        request.plan.previous = Some(result.artifact.entry());
        request.plan.through.entry = EntryId::new();
        state = restore(&state);
        assert!(serde_json::to_vec(&state).unwrap().len() < 3000);
    }
    let mut stale = request.clone();
    stale.plan.previous = None;
    assert!(state.request(stale).is_err());
}

#[test]
fn refusal_cannot_erase_unknown_work_and_retains_required_pending_obligation() {
    let mut request = request();
    request.required = true;
    let mut state = Checkpoint::default();
    state.request(request.clone()).unwrap();
    state.dispatch(request.operation).unwrap();
    state.unknown(request.operation).unwrap();
    assert!(
        state
            .refuse(request.operation, "unavailable".into())
            .is_err()
    );
    assert!(state.compaction().unwrap().required);
    assert_eq!(state.compaction().unwrap().phase, Phase::Unknown);
}

#[test]
fn model_change_cannot_reuse_opaque_state_or_supersede_an_unknown_paid_result() {
    let mut input = request();
    input.plan.strategy = Strategy::Native {
        qualification: Configuration([3; 32]),
    };
    let mut state = Checkpoint::default();
    state.request(input.clone()).unwrap();
    state.dispatch(input.operation).unwrap();
    assert!(state.supersede(input.operation).is_err());
    state
        .recognize(input.operation, Recovery::Completed(candidate(&input)))
        .unwrap();
    state.activate(input.operation, boundary(&input)).unwrap();
    assert!(state.active_for(input.plan.configuration).is_some());
    input.operation = OperationId::new();
    input.plan.configuration = Configuration([4; 32]);
    assert!(state.active_for(input.plan.configuration).is_none());
    state.request(input.clone()).unwrap();
    state.dispatch(input.operation).unwrap();
    let result = candidate(&input);
    state
        .recognize(input.operation, Recovery::Completed(result.clone()))
        .unwrap();
    state.supersede(input.operation).unwrap();
    assert_eq!(
        state.compaction().unwrap().phase,
        Phase::Superseded {
            artifact: Some(result.artifact)
        }
    );
}

#[test]
fn reused_identity_with_changed_plan_and_oversized_imported_reasons_are_refused() {
    let input = request();
    let mut state = Checkpoint::default();
    state.request(input.clone()).unwrap();
    let mut changed = input.clone();
    changed.plan.through.entry = EntryId::new();
    assert!(state.request(changed).is_err());
    let before = state.clone();
    assert!(
        state
            .refuse(input.operation, "x".repeat(MAX_REASON_BYTES + 1))
            .is_err()
    );
    assert_eq!(state, before);
    state
        .refuse(input.operation, "The prepared model is unavailable".into())
        .unwrap();
    let mut imported = serde_json::to_value(&state).unwrap();
    imported["compaction"]["phase"]["reason"] = serde_json::json!("x".repeat(MAX_REASON_BYTES + 1));
    let imported: Checkpoint = serde_json::from_value(imported).unwrap();
    assert!(imported.validate().is_err());
    assert!(
        Snapshot {
            checkpoint: imported,
            ..Snapshot::default()
        }
        .validate()
        .is_err()
    );
}
