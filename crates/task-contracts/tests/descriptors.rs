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
