use swarmie_task_contracts::{DurableTasks, TaskHandler, TaskStorage};
use tembra::{ContractToken, contracts::Contract};

#[test]
fn canonical_descriptors_round_trip_without_backend_dependencies()
-> Result<(), Box<dyn std::error::Error>> {
    for contract in [
        TaskStorage.descriptor()?,
        DurableTasks.descriptor()?,
        TaskHandler.descriptor()?,
    ] {
        let encoded = serde_json::to_string(contract.descriptor())?;
        let decoded = Contract::from_json(&encoded)?;
        assert_eq!(decoded.fingerprint(), contract.fingerprint());
    }
    Ok(())
}

#[test]
fn tagged_transitions_remain_disjoint_in_the_portable_schema()
-> Result<(), Box<dyn std::error::Error>> {
    use tembra::contracts::{Member, ValueSchema, validate};

    let contract = TaskHandler.descriptor()?;
    let Some(Member::Method {
        result: Some(ValueSchema::Portable(schema)),
        ..
    }) = contract.members("").and_then(|members| members.get("step"))
    else {
        panic!("canonical step must have a portable result");
    };
    for value in [
        serde_json::json!({"type":"complete", "value":null}),
        serde_json::json!({"type":"checkpoint", "checkpoint":{"operation_id":"task:publish"}, "phase":"publish"}),
        serde_json::json!({"type":"reject_migration", "reason":"unsupported checkpoint"}),
    ] {
        validate(schema, &value)?;
    }
    for value in [
        serde_json::json!({"type":"unknown", "value":null}),
        serde_json::json!({"type":"complete"}),
        serde_json::json!({"type":"complete", "value":null, "reason":"ambiguous"}),
        serde_json::json!({"type":"checkpoint", "checkpoint":{}, "phase":5}),
    ] {
        assert!(
            validate(schema, &value).is_err(),
            "accepted invalid transition: {value}"
        );
    }
    Ok(())
}

#[test]
fn lifecycle_hooks_have_independent_typed_series_registries()
-> Result<(), Box<dyn std::error::Error>> {
    use swarmie_task_contracts::*;
    use tembra::contracts::{Member, ValueSchema};

    let definitions = [
        (
            on_task_created()?.descriptor().clone(),
            TaskCreatedObserver.descriptor()?,
        ),
        (
            on_task_phase_started()?.descriptor().clone(),
            TaskPhaseStartedObserver.descriptor()?,
        ),
        (
            on_task_phase_settled()?.descriptor().clone(),
            TaskPhaseSettledObserver.descriptor()?,
        ),
        (
            on_task_checkpoint()?.descriptor().clone(),
            TaskCheckpointObserver.descriptor()?,
        ),
        (
            on_task_waiting()?.descriptor().clone(),
            TaskWaitingObserver.descriptor()?,
        ),
        (
            on_task_sleeping()?.descriptor().clone(),
            TaskSleepingObserver.descriptor()?,
        ),
        (
            on_task_recovered()?.descriptor().clone(),
            TaskRecoveredObserver.descriptor()?,
        ),
        (
            on_task_blocked()?.descriptor().clone(),
            TaskBlockedObserver.descriptor()?,
        ),
        (
            on_task_parked()?.descriptor().clone(),
            TaskParkedObserver.descriptor()?,
        ),
        (
            on_task_retry()?.descriptor().clone(),
            TaskRetryObserver.descriptor()?,
        ),
        (
            on_task_cancellation_requested()?.descriptor().clone(),
            TaskCancellationRequestedObserver.descriptor()?,
        ),
        (
            on_task_terminal()?.descriptor().clone(),
            TaskTerminalObserver.descriptor()?,
        ),
    ];
    let mut identities = std::collections::BTreeSet::new();
    for (definition, contract) in definitions {
        let encoded = serde_json::to_string(contract.descriptor())?;
        assert_eq!(
            Contract::from_json(&encoded)?.fingerprint(),
            contract.fingerprint()
        );
        let Some(Member::Method {
            args: Some(ValueSchema::Portable(schema)),
            ..
        }) = contract
            .members("")
            .and_then(|members| members.get("observe"))
        else {
            panic!("lifecycle input must be portable");
        };
        assert_eq!(
            definition.input(),
            &Some(ValueSchema::Portable(schema.clone()))
        );
        assert_eq!(definition.strategy(), tembra_hooks::Strategy::Series);
        assert!(definition.result().is_none());
        assert!(identities.insert(definition.descriptor().name.clone()));
    }
    assert_eq!(identities.len(), 12);
    Ok(())
}

#[test]
fn lifecycle_schema_is_bounded_and_distinguishes_commits_from_physical_settlement()
-> Result<(), Box<dyn std::error::Error>> {
    use swarmie_task_contracts::*;
    use tembra::contracts::{Member, ValueSchema, validate};

    let contract = TaskPhaseSettledObserver.descriptor()?;
    let Some(Member::Method {
        args: Some(ValueSchema::Portable(schema)),
        ..
    }) = contract
        .members("")
        .and_then(|members| members.get("observe"))
    else {
        panic!("phase settlement must export portable input");
    };
    let task = TaskLifecycleTask {
        task_id: "task:1".into(),
        task_kind: "example".into(),
        revision: 2,
        phase: "run".into(),
        attempt: 1,
        definition_version: 1,
    };
    let value = serde_json::to_value(TaskPhaseSettled {
        task: task.clone(),
        invocation: TaskPhaseKind::Run,
        outcome: TaskPhaseOutcome::Returned,
    })?;
    validate(schema, &value)?;
    assert!(serde_json::to_vec(&value)?.len() < 1024);
    for (field, invalid) in [
        ("task_id", serde_json::json!("x".repeat(129))),
        ("task_kind", serde_json::json!("x".repeat(129))),
        ("revision", serde_json::json!(9_007_199_254_740_992_u64)),
        ("phase", serde_json::json!("")),
        ("definition_version", serde_json::json!(0)),
    ] {
        let mut invalid_value = value.clone();
        invalid_value["task"][field] = invalid;
        assert!(
            validate(schema, &invalid_value).is_err(),
            "accepted invalid {field}"
        );
    }
    for field in ["sequence", "checkpoint", "result"] {
        let mut invalid_value = value.clone();
        invalid_value[field] = serde_json::json!("x".repeat(4096));
        assert!(validate(schema, &invalid_value).is_err());
    }
    let contract = TaskCheckpointObserver.descriptor()?;
    let Some(Member::Method {
        args: Some(ValueSchema::Portable(schema)),
        ..
    }) = contract
        .members("")
        .and_then(|members| members.get("observe"))
    else {
        panic!("checkpoint must export portable input");
    };
    let checkpoint = serde_json::to_value(TaskCheckpoint {
        transition: TaskCommittedObservation {
            task,
            sequence: 3,
            previous_status: Some(TaskStatus::Running),
            current_status: TaskStatus::Waiting,
            origin: TaskLifecycleOrigin::Handler,
            checkpoint_committed: true,
            cancellation_requested: false,
            outcome: None,
            blocking: None,
        },
    })?;
    validate(schema, &checkpoint)?;
    assert!(serde_json::to_vec(&checkpoint)?.len() < 1024);
    assert!(validate(schema, &value).is_err());
    Ok(())
}

#[test]
fn outbox_consumer_results_require_an_explicit_owner() -> Result<(), Box<dyn std::error::Error>> {
    use tembra::contracts::{Member, ValueSchema, validate};

    let contract = TaskStorage.descriptor()?;
    let Some(Member::Method {
        result: Some(ValueSchema::Portable(schema)),
        ..
    }) = contract
        .members("")
        .and_then(|members| members.get("register_outbox_consumer"))
    else {
        panic!("consumer registration must export a portable result");
    };
    let value = serde_json::json!({
        "owner_facet": "fixture.publisher",
        "consumer_id": "history",
        "store_id": "source",
        "acknowledged": null,
        "delivered": null,
        "retained_after": null,
    });
    validate(schema, &value)?;
    for owner in [
        serde_json::Value::Null,
        serde_json::json!(""),
        serde_json::json!(7),
    ] {
        let mut invalid = value.clone();
        invalid["owner_facet"] = owner;
        assert!(validate(schema, &invalid).is_err());
    }
    let mut missing = value;
    missing.as_object_mut().unwrap().remove("owner_facet");
    assert!(validate(schema, &missing).is_err());
    Ok(())
}
