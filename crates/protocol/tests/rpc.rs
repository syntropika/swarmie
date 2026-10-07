use mpai::Runtime;
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
    use mpai::SpawnOptions;
    use mpai::mock::{MockHarness, MockScript};
    let harness = MockHarness::new(MockScript {
        models: vec![json!({"id":"native-model","label":"Native Model"})],
        configuration: vec![
            json!({"id":"model","label":"Model","type":"select","value":"native-model","choices":[{"id":"native-model","label":"Native Model"}],"live":true}),
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
