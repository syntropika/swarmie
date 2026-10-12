//! Native application workflows. Views provide intent; runtime calls retain one exact target.
mod projects;
mod sessions;
use crate::{client_submissions::ClientSubmissions, node_gateway::NodeGateway};
use serde_json::{Value, json};
use std::sync::Arc;
use swarmie::RuntimeIdentity;
use tokio::sync::Mutex;

type Outcome = std::result::Result<Value, Value>;
#[derive(Clone)]
pub struct ClientOperations {
    nodes: NodeGateway,
    submissions: ClientSubmissions,
    projects: Arc<Mutex<()>>,
}
impl ClientOperations {
    pub fn new(nodes: NodeGateway, submissions: ClientSubmissions) -> Self {
        Self {
            nodes,
            submissions,
            projects: Default::default(),
        }
    }
    pub fn handles(method: &str) -> bool {
        matches!(
            method,
            "client/sessionCreate"
                | "client/sessionOpen"
                | "client/sessionPrepare"
                | "client/sessionControls"
                | "client/sessionConfigure"
                | "client/projectOpen"
                | "client/projectCatalog"
                | "client/projectGet"
                | "client/projectDefaults"
                | "client/chatDirectory"
                | "client/extensionInvoke"
                | "client/worktree"
        )
    }
    pub async fn handle(&self, node: &str, request: Value) -> Value {
        let id = request["id"].clone();
        let result = self.execute(node, &request).await;
        match result {
            Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
            Err(error) => json!({"jsonrpc":"2.0","id":id,"error":error}),
        }
    }
    async fn execute(&self, node: &str, request: &Value) -> Outcome {
        let target: RuntimeIdentity = serde_json::from_value(request["expectedRuntime"].clone())
            .map_err(|_| invalid("an exact runtime target is required"))?;
        if target.identity.environment_id != node || target.incarnation_id.is_empty() {
            return Err(invalid("application node target mismatch"));
        }
        let mut call = Calls {
            nodes: &self.nodes,
            node,
            target,
            effects: false,
        };
        let observed = call.call("runtime/initialize", json!({}), false).await?;
        if observed["runtimeIdentity"] != request["expectedRuntime"] {
            return Err(
                json!({"code":-32005,"message":"runtime_target_mismatch","data":{"effectKnowledge":"none"}}),
            );
        }
        let params = &request["params"];
        let result = match request["method"].as_str().unwrap_or("") {
            "client/sessionCreate" | "client/sessionPrepare" => {
                sessions::launch(
                    &mut call,
                    params,
                    request["method"] == "client/sessionPrepare",
                )
                .await
            }
            "client/sessionOpen" => sessions::open(&mut call, &self.submissions, request).await,
            "client/sessionControls" => {
                sessions::controls(&mut call, text(params, "sessionId")?).await
            }
            "client/sessionConfigure" => sessions::configure(&mut call, params).await,
            "client/projectOpen" | "client/projectGet" | "client/projectCatalog" => {
                let _lock = self.projects.lock().await;
                projects::project(&mut call, params, request["method"].as_str().unwrap()).await
            }
            "client/projectDefaults" => projects::defaults(&mut call, params).await,
            "client/chatDirectory" => projects::chat_directory(&mut call, params).await,
            "client/extensionInvoke" => projects::invoke(&mut call, params).await,
            "client/worktree" => projects::worktree(&mut call, params).await,
            _ => Err(invalid("unknown application operation")),
        };
        result.map_err(|mut error| {
            if call.effects {
                error_data(&mut error)["effectKnowledge"] = "unknown".into();
            }
            error
        })
    }
}
struct Calls<'a> {
    nodes: &'a NodeGateway,
    node: &'a str,
    target: RuntimeIdentity,
    effects: bool,
}
impl Calls<'_> {
    async fn call(&mut self, method: &str, params: Value, mutation: bool) -> Outcome {
        // Keep the local runtime dispatch future off callers' stacks.
        let response = Box::pin(self.nodes.forward(
            self.node,
            json!({"jsonrpc":"2.0","id":uuid::Uuid::new_v4().to_string(),
            "method":method,"params":params,"expectedRuntime":self.target}),
        ))
        .await;
        if let Some(error) = response.get("error") {
            if mutation && error["data"]["effectKnowledge"] != "none" {
                self.effects = true;
            }
            return Err(error.clone());
        }
        self.effects |= mutation;
        response
            .get("result")
            .cloned()
            .ok_or_else(|| failure("runtime returned no result"))
    }
    async fn extension(
        &mut self,
        extension: &str,
        command: &str,
        arguments: Value,
        mutation: bool,
    ) -> Outcome {
        self.call(
            "extension/invoke",
            json!({"extensionId":extension,"command":command,"arguments":arguments}),
            mutation,
        )
        .await
    }
    async fn extensions(&mut self) -> std::result::Result<Vec<Value>, Value> {
        self.call("extension/list", json!({}), false)
            .await?
            .as_array()
            .cloned()
            .ok_or_else(|| failure("invalid extension catalog"))
    }
}
fn text<'a>(value: &'a Value, key: &str) -> std::result::Result<&'a str, Value> {
    value[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid(&format!("{key} is required")))
}
fn invalid(message: &str) -> Value {
    json!({"code":-32001,"message":message,"data":{"effectKnowledge":"none"}})
}
fn failure(message: &str) -> Value {
    json!({"code":-32001,"message":message,"data":{"effectKnowledge":"unknown"}})
}
fn unavailable(error: &Value) -> bool {
    matches!(error["code"].as_i64(), Some(-32005 | -32006))
}

fn error_data(error: &mut Value) -> &mut Value {
    if !error.is_object() {
        *error = failure("invalid runtime error");
    }
    if !error["data"].is_object() {
        error["data"] = json!({});
    }
    &mut error["data"]
}
#[cfg(test)]
mod tests;
