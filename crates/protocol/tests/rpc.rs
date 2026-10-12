use swarmie::Runtime;
use protocol::RuntimeService;
use serde_json::json;
#[tokio::test]
async fn json_rpc_validates_ids_params_notifications_and_batches() {
    let service = RuntimeService::new(Runtime::default());
    assert_eq!(
        service
            .handle(json!({"jsonrpc":"2.0","id":"request","method":"runtime/initialize"}))
            .await
            .unwrap()["id"],
        "request"
    );
    assert!(
        service
            .handle(json!({"jsonrpc":"2.0","method":"runtime/initialize"}))
            .await
            .is_none()
    );
    for r in [
        json!(null),
        json!({"jsonrpc":"2.0","method":"runtime/initialize","id":true}),
        json!([]),
    ] {
        assert_eq!(service.handle(r).await.unwrap()["error"]["code"], -32600);
    }
    assert_eq!(
        service
            .handle(json!({"jsonrpc":"2.0","id":1,"method":"unknown"}))
            .await
            .unwrap()["error"]["code"],
        -32601
    );
    assert_eq!(
        service
            .handle(json!({"jsonrpc":"2.0","id":1,"method":"session/prompt","params":{}}))
            .await
            .unwrap()["error"]["code"],
        -32602
    );
    let result = service.handle(json!([{"jsonrpc":"2.0","method":"runtime/initialize"},{"jsonrpc":"2.0","id":9,"method":"runtime/discover"}])).await.unwrap();
    assert_eq!(result.as_array().unwrap().len(), 1);
    assert_eq!(result[0]["id"], 9);
}

#[tokio::test]
async fn configuration_preview_reads_native_choices_without_persisting_a_chat_or_prompt() {
    use swarmie::SpawnOptions;
    use swarmie::mock::{MockHarness, MockScript};
    let harness = MockHarness::new(MockScript {
        models: vec![json!({"id":"native-model","label":"Native Model"})],
        configuration: vec![
            json!({"id":"model","label":"Model","type":"select","value":"native-model","choices":[{"id":"native-model","label":"Native Model"}],"application":"next_turn"}),
        ],
        ..Default::default()
    });
    let observation = harness.clone();
    let runtime = Runtime::builder().harness(harness).build();
    let service = RuntimeService::new(runtime.clone());
    let options = SpawnOptions::new("mock", std::env::current_dir().unwrap());
    let result = service
        .invoke(
            "runtime/configuration",
            serde_json::to_value(options).unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(result["configuration"][0]["value"], "native-model");
    assert_eq!(result["models"][0]["id"], "native-model");
    assert!(runtime.sessions().await.unwrap().is_empty());
    assert!(observation.prompts().await.is_empty());
    assert!(observation.launches().await[0].execution.ephemeral);
    assert!(
        service
            .invoke("runtime/initialize", json!({}))
            .await
            .unwrap()["methods"]
            .as_array()
            .unwrap()
            .contains(&json!("runtime/configuration"))
    );
    assert!(service.invoke("runtime/schema", json!({})).await.unwrap()["x-methods"]["runtime/configuration"].is_object());
}

#[test]
fn public_schema_publishes_canonical_outcomes_and_separate_native_reason() {
    let schema = protocol::protocol_schema();
    assert_eq!(
        schema["$defs"]["TurnOutcome"]["enum"],
        json!([
            "completed",
            "cancelled",
            "interrupted",
            "failed",
            "limited",
            "refused",
            "handled",
            "unknown"
        ])
    );
    let terminal = schema["$defs"]["EventKind"]["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .find(|variant| variant["properties"]["type"]["const"] == "turn.ended")
        .unwrap();
    let fields = &terminal["properties"]["data"]["properties"];
    assert!(fields.get("outcome").is_some());
    assert!(fields.get("nativeReason").is_some());
    assert!(fields.get("reason").is_none());
}

#[test]
fn public_schema_types_capabilities_catalogs_and_configuration_receipts() {
    let schema = protocol::protocol_schema();
    assert_eq!(
        schema["$defs"]["Capabilities"]["properties"]["configure"]["$ref"],
        "#/$defs/Capability"
    );
    assert_eq!(
        schema["x-methods"]["agent/configure"]["result"]["$ref"],
        "#/$defs/ConfigurationReceipt"
    );
    assert_eq!(
        schema["x-methods"]["agent/models"]["result"]["items"]["$ref"],
        "#/$defs/ModelDescriptor"
    );
    assert_eq!(
        schema["x-methods"]["agent/configuration"]["result"]["items"]["$ref"],
        "#/$defs/SettingDescriptor"
    );
    assert_eq!(
        schema["x-methods"]["runtime/configuration"]["result"]["$ref"],
        "#/$defs/ConfigurationPreview"
    );
}

#[tokio::test]
async fn runtime_identity_survives_client_clones_but_changes_incarnation_with_a_new_owner() {
    let store = std::sync::Arc::new(swarmie::MemoryStore::default());
    let first = Runtime::builder().shared_store(store.clone()).build();
    let identity = first.identity().await.unwrap();
    assert_eq!(first.clone().identity().await.unwrap(), identity);
    let initialized = RuntimeService::new(first)
        .invoke("runtime/initialize", json!({}))
        .await
        .unwrap();
    assert_eq!(initialized["runtimeIdentity"], json!(identity));
    let restarted = Runtime::builder()
        .shared_store(store)
        .build()
        .identity()
        .await
        .unwrap();
    assert_eq!(identity.identity, restarted.identity);
    assert_ne!(identity.incarnation_id, restarted.incarnation_id);
    assert_ne!(
        identity.identity,
        Runtime::default().identity().await.unwrap().identity
    );
}

#[tokio::test]
async fn target_guards_reject_stale_owners_and_other_environments_before_effects() {
    let store = std::sync::Arc::new(swarmie::MemoryStore::default());
    let old = Runtime::builder()
        .shared_store(store.clone())
        .build()
        .identity()
        .await
        .unwrap();
    let runtime = Runtime::builder().shared_store(store).build();
    let service = RuntimeService::new(runtime.clone());
    let actual = runtime.identity().await.unwrap();
    let foreign = Runtime::default().identity().await.unwrap();
    for target in [old, foreign] {
        let request = json!({"jsonrpc":"2.0","id":1,"method":"project/create",
            "params":{"name":"must not exist"},"expectedRuntime":target});
        let response = service.handle(request.clone()).await.unwrap();
        assert_eq!(response["error"]["code"], -32005);
        assert_eq!(response["error"]["data"]["effectKnowledge"], "none");
        let mut notification = request.clone();
        notification.as_object_mut().unwrap().remove("id");
        assert!(service.handle(notification).await.is_none());
        let batch = service.handle(json!([request])).await.unwrap();
        assert_eq!(batch[0]["error"]["data"]["code"], "runtime_target_mismatch");
    }
    assert!(runtime.local_catalog().await.unwrap().projects.is_empty());
    let malformed = service
        .handle(json!({"jsonrpc":"2.0","id":2,"method":"project/create",
        "params":{"name":"must not exist"},"expectedRuntime":{}}))
        .await
        .unwrap();
    assert_eq!(malformed["error"]["code"], -32602);
    let valid = service
        .handle(json!({"jsonrpc":"2.0","id":3,"method":"project/create",
        "params":{"name":"accepted"},"expectedRuntime":actual}))
        .await
        .unwrap();
    assert_eq!(valid["result"]["name"], "accepted");
    assert_eq!(runtime.local_catalog().await.unwrap().projects.len(), 1);
    let schema = protocol::protocol_schema();
    assert_eq!(
        schema["x-requestTarget"]["$ref"],
        "#/$defs/InputRuntimeIdentity"
    );
    assert!(
        schema["x-methods"]["runtime/initialize"]["result"]["required"]
            .as_array()
            .unwrap()
            .contains(&json!("runtimeIdentity"))
    );
}

#[tokio::test]
async fn shutdown_is_shared_retained_and_guarded_against_another_owner() {
    let service = RuntimeService::new(Runtime::default());
    let clone = service.clone();
    let foreign = Runtime::default().identity().await.unwrap();
    let rejected = service
        .handle(
            json!({"jsonrpc":"2.0","id":1,"method":"runtime/shutdown","expectedRuntime":foreign}),
        )
        .await
        .unwrap();
    assert_eq!(rejected["error"]["code"], -32005);
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(10),
            clone.wait_for_shutdown()
        )
        .await
        .is_err()
    );
    let identity = service.runtime().identity().await.unwrap();
    let accepted = service
        .handle(
            json!({"jsonrpc":"2.0","id":2,"method":"runtime/shutdown","expectedRuntime":identity}),
        )
        .await
        .unwrap();
    assert!(accepted.get("error").is_none());
    assert_eq!(accepted["result"], serde_json::Value::Null);
    tokio::time::timeout(std::time::Duration::from_secs(1), clone.wait_for_shutdown())
        .await
        .unwrap();
}
