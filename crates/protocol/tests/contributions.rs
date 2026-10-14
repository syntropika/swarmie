use async_trait::async_trait;
use protocol::RuntimeService;
use serde_json::{Value, json};
use swarmie::{ExtensionCommand, ExtensionView, Result, Runtime, RuntimeExtension, UiManifest};

struct Extension {
    declarative: bool,
}

#[async_trait]
impl RuntimeExtension for Extension {
    fn id(&self) -> &str {
        if self.declarative {
            "declarative"
        } else {
            "legacy"
        }
    }
    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![ExtensionCommand {
            name: "inspect".into(),
            title: "Inspect".into(),
            mutates: false,
        }]
    }
    fn views(&self) -> Vec<ExtensionView> {
        vec![ExtensionView {
            id: "tool".into(),
            title: "Tool".into(),
            kind: "web".into(),
            command: "inspect".into(),
        }]
    }
    fn ui(&self) -> Option<UiManifest> {
        self.declarative.then(|| serde_json::from_value(json!({
            "version": 1,
            "commands": [{"id":"inspect","title":"Inspect","runtimeCommand":"inspect"}],
            "slots": [{"id":"inspect","slot":"message.actions","kind":"action","title":"Inspect","command":"inspect"}],
            "panels": [{"id":"panel","title":"Panel","command":"inspect"}]
        })).unwrap())
    }
    async fn invoke(&self, _command: &str, params: Value) -> Result<Value> {
        Ok(params)
    }
}

#[tokio::test]
async fn extension_list_advertises_optional_ui_without_changing_legacy_invocation() {
    let service = RuntimeService::new(
        Runtime::builder()
            .extension(Extension { declarative: true })
            .extension(Extension { declarative: false })
            .build(),
    );
    let catalog = service.invoke("extension/list", json!({})).await.unwrap();
    assert_eq!(catalog[0]["ui"]["version"], 1);
    assert_eq!(catalog[0]["ui"]["slots"][0]["slot"], "message.actions");
    assert_eq!(catalog[0]["ui"]["panels"][0]["command"], "inspect");
    assert!(catalog[1].get("ui").is_none());
    for id in ["declarative", "legacy"] {
        assert_eq!(service.invoke("extension/invoke", json!({"extensionId":id,"command":"inspect","arguments":{"messageId":"message"}})).await.unwrap(), json!({"messageId":"message"}));
    }
}
