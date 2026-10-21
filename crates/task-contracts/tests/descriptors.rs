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
fn lifecycle_schema_is_bounded_and_distinguishes_commits_from_physical_settlement()
-> Result<(), Box<dyn std::error::Error>> {
    use swarmie_task_contracts::{
        TaskLifecycle, TaskLifecycleEvent, TaskLifecycleObserver, TaskLifecycleTask, TaskPhaseKind,
        TaskPhaseOutcome, task_lifecycle,
    };
    use tembra::contracts::{Member, ValueSchema, validate};

    for contract in [
        TaskLifecycleObserver.descriptor()?,
        TaskLifecycle.descriptor()?,
    ] {
        let encoded = serde_json::to_string(contract.descriptor())?;
        assert_eq!(
            Contract::from_json(&encoded)?.fingerprint(),
            contract.fingerprint()
        );
    }
    let definition = task_lifecycle()?;
    let contract = TaskLifecycleObserver.descriptor()?;
    let Some(Member::Method {
        args: Some(ValueSchema::Portable(schema)),
        ..
    }) = contract
        .members("")
        .and_then(|members| members.get("observe"))
    else {
        panic!("lifecycle observer must export portable input");
    };
    assert_eq!(
        definition.descriptor().input(),
        &Some(ValueSchema::Portable(schema.clone()))
    );
    assert_eq!(
        definition.descriptor().strategy(),
        tembra_hooks::Strategy::Series
    );
    assert!(definition.descriptor().result().is_none());
    let event = TaskLifecycleEvent::PhaseSettled {
        task: TaskLifecycleTask {
            task_id: "task:1".into(),
            task_kind: "example".into(),
            revision: 2,
            phase: "run".into(),
            attempt: 1,
            definition_version: 1,
        },
        invocation: TaskPhaseKind::Run,
        outcome: TaskPhaseOutcome::Returned,
    };
    let value = serde_json::to_value(event)?;
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
    let mut invented_commit = value.clone();
    invented_commit["type"] = "committed".into();
    assert!(validate(schema, &invented_commit).is_err());
    let mut oversized_result = value;
    oversized_result["result"] = serde_json::json!("x".repeat(4096));
    assert!(validate(schema, &oversized_result).is_err());
    Ok(())
}
