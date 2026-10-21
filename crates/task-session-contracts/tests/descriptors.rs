use swarmie_task_session_contracts::*;
use tembra::ContractToken;

#[test]
fn portable_contracts_have_stable_distinct_identities() {
    for (token, name) in [
        (
            TaskSessionBindings.descriptor().unwrap(),
            "swarmie.task-session-bindings",
        ),
        (
            TaskSessionPublisher.descriptor().unwrap(),
            "swarmie.task-session-publisher",
        ),
    ] {
        let json = serde_json::to_value(token.descriptor()).unwrap();
        assert_eq!(json["name"], name);
        assert_eq!(json["version"], 1);
        assert!(!json.to_string().contains("oneOf"));
    }
}
