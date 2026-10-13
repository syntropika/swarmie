//! Local client gateway; remote credentials never cross the browser interface.
use crate::node_connection::ConnectionManager;
use swarmie::{Error, Result};
use protocol::RuntimeService;
use serde_json::{Value, json};

#[derive(Clone)]
pub struct NodeGateway {
    pub(super) manager: ConnectionManager,
    pub(super) local: Local,
    pub(super) local_id: String,
}
#[derive(Clone)]
pub(super) enum Local {
    Runtime(RuntimeService),
    Endpoint {
        url: String,
        identity: swarmie::LocalStoreIdentity,
        root: std::path::PathBuf,
    },
}
impl NodeGateway {
    pub async fn new(root: &std::path::Path, service: RuntimeService) -> Result<Self> {
        let manager = ConnectionManager::open(root).await?;
        let identity = service.runtime().identity().await?;
        manager.remember_local(&identity).await?;
        Ok(Self {
            local_id: identity.identity.environment_id,
            manager,
            local: Local::Runtime(service),
        })
    }
    pub async fn attached(
        root: &std::path::Path,
        endpoint: &crate::runtime_host::Endpoint,
    ) -> Result<Self> {
        let manager = ConnectionManager::open(root).await?;
        manager.remember_local(&endpoint.runtime_identity).await?;
        Ok(Self {
            local_id: endpoint.runtime_identity.identity.environment_id.clone(),
            manager,
            local: Local::Endpoint {
                url: local_endpoint(endpoint),
                identity: endpoint.runtime_identity.identity.clone(),
                root: root.to_owned(),
            },
        })
    }
    pub fn local_id(&self) -> &str {
        &self.local_id
    }
    pub fn publish(&self, event: Value) {
        self.manager.publish(event);
    }
    pub fn events(&self) -> tokio::sync::broadcast::Receiver<Value> {
        self.manager.events()
    }
    pub async fn disconnect(&self, node: &str) {
        self.manager.disconnect(node).await;
    }
    pub async fn close(&self) {
        self.manager.close().await;
    }
    async fn local_request(&self, request: Value) -> Result<Value> {
        match &self.local {
            Local::Runtime(service) => Ok(service.handle(request).await.unwrap_or(Value::Null)),
            Local::Endpoint {
                url,
                identity,
                root,
            } => {
                let current = crate::runtime_host::read(root)?;
                if current
                    .as_ref()
                    .is_some_and(|endpoint| endpoint.runtime_identity.identity != *identity)
                {
                    return Err(Error::Invalid(
                        "local runtime discovery now targets another node".into(),
                    ));
                }
                let url = current.as_ref().map_or_else(|| url.clone(), local_endpoint);
                self.manager.local_request(&url, identity, request).await
            }
        }
    }
    async fn identity(&self) -> Result<swarmie::RuntimeIdentity> {
        match &self.local {
            Local::Runtime(service) => service.runtime().identity().await,
            Local::Endpoint { .. } => {
                let value = self.local_request(json!({"jsonrpc":"2.0","id":"identity","method":"runtime/initialize","params":{}})).await?;
                crate::node_connection::handshake(&value["result"], None)
            }
        }
    }
    pub async fn control(&self, request: Value) -> Value {
        let id = request["id"].clone();
        let target: Option<swarmie::RuntimeIdentity> = request
            .get("expectedRuntime")
            .and_then(|value| serde_json::from_value(value.clone()).ok());
        if target.as_ref() != self.identity().await.ok().as_ref() {
            return json!({"jsonrpc":"2.0","id":id,"error":{"code":-32005,"message":"runtime_target_mismatch","data":{"effectKnowledge":"none"}}});
        }
        let params = &request["params"];
        let result: Result<Value> = match request["method"].as_str().unwrap_or("") {
            "nodes/list" => self.manager.catalog().await.and_then(|catalog| {
                let mut value = serde_json::to_value(catalog)?;
                for node in value["nodes"].as_array_mut().unwrap() {
                    for route in node["connections"].as_array_mut().unwrap() {
                        let reference = route.as_object_mut().unwrap().remove("credentialRef");
                        route["authenticated"] =
                            reference.is_some_and(|value| !value.is_null()).into();
                    }
                }
                Ok(value)
            }),
            "nodes/select" => match params["environmentId"].as_str() {
                Some(id) => self.manager.select(id.into()).await.map(|_| Value::Null),
                None => Err(Error::Invalid("environmentId is required".into())),
            },
            "nodes/pair" => match (params["name"].as_str(), params["url"].as_str()) {
                (Some(name), Some(url)) => self
                    .manager
                    .pair_new(
                        name.into(),
                        url.into(),
                        params["token"]
                            .as_str()
                            .filter(|s| !s.is_empty())
                            .map(str::to_owned),
                    )
                    .await
                    .map(|route| crate::node_connection::public_connection(&route)),
                _ => Err(Error::Invalid(
                    "connection name and URL are required".into(),
                )),
            },
            "nodes/inspectSsh" => match (params["host"].as_str(), Some(params["socketPath"].as_str().unwrap_or(""))) {
                (Some(host), Some(socket)) => crate::remote::inspect_ssh(host, socket, params["token"].as_str()).await,
                _ => Err(Error::Invalid("SSH host and socketPath are required".into())),
            },
            "nodes/startup" => self.startup(params).await,
            "nodes/prepareSsh" => match target.as_ref() {
                Some(target) => self.prepare_ssh(params, target).await,
                None => Err(Error::Invalid("verified local runtime is required".into())),
            },
            "nodes/pairSsh" => match (params["name"].as_str(), params["host"].as_str(), Some(params["socketPath"].as_str().unwrap_or("")), serde_json::from_value::<swarmie::RuntimeIdentity>(params["runtimeIdentity"].clone())) {
                (Some(name), Some(host), Some(socket), Ok(target)) => {
                    match crate::remote::ssh_endpoint(host, socket) {
                        Ok(url) => self.manager.pair_prepared(name.into(), url, params["token"].as_str().filter(|s| !s.is_empty()).map(str::to_owned), &target).await.map(|route| crate::node_connection::public_connection(&route)),
                        Err(error) => Err(error),
                    }
                }
                _ => Err(Error::Invalid("connection name, SSH host, socketPath and verified runtimeIdentity are required".into())),
            },
            "nodes/removeRoute" => match (params["name"].as_str(), params["revision"].as_u64()) {
                (Some(name), Some(revision)) => self
                    .manager
                    .remove_revision(name, revision)
                    .await
                    .map(|_| Value::Null),
                _ => Err(Error::Invalid(
                    "connection name and revision are required".into(),
                )),
            },
            _ => Err(Error::Unsupported("unknown client node action".into())),
        };
        match result {
            Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
            Err(error) => protocol::rpc_error(id, -32001, &error.to_string()),
        }
    }
    async fn prepare_ssh(&self, params: &Value, local_target: &swarmie::RuntimeIdentity) -> Result<Value> {
        let operation = async {
            let name = params["name"].as_str().ok_or_else(|| Error::Invalid("connection name is required".into()))?;
            crate::node_connection::validate_name(name)?;
            if self.manager.find(name).await?.is_some() {
                return Err(Error::Invalid("connection name is already in use".into()));
            }
            let host = params["host"].as_str().ok_or_else(|| Error::Invalid("SSH host is required".into()))?;
            let action = params["action"].as_str().ok_or_else(|| Error::Invalid("preparation action is required".into()))?;
            let socket = params["socketPath"].as_str().unwrap_or("");
            let result = crate::client::ssh_preparation::prepare(host, socket, action).await?;
            if self.identity().await? != *local_target {
                return Err(Error::Invalid("local runtime changed during preparation; check the connection again".into()));
            }
            let target = serde_json::from_value(result["runtimeIdentity"].clone())?;
            self.manager.pair_prepared(name.into(), crate::remote::ssh_endpoint(host, socket)?, None, &target).await?;
            Ok(result)
        };
        tokio::time::timeout(std::time::Duration::from_secs(600), operation).await
            .map_err(|_| Error::Timeout("SSH preparation/enrollment; remote changes may have completed, check before retrying".into()))?
    }
    pub async fn forward(&self, environment_id: &str, request: Value) -> Value {
        let id = request["id"].clone();
        let result = if environment_id == self.local_id {
            self.local_request(request).await
        } else {
            self.manager.forward(environment_id, request).await
        };
        match result {
            Ok(response) => response,
            Err(error) => json!({"jsonrpc":"2.0","id":id,"error":{
                "code":if matches!(error, Error::Invalid(_)) {-32005} else {-32006},
                "message":error.to_string(),"data":{"effectKnowledge":if matches!(error, Error::Invalid(_)) {"none"} else {"unknown"}}}}),
        }
    }
    pub async fn preview(&self, environment_id: &str, id: &str, path: &str) -> Result<Value> {
        let params = json!({"previewId":id,"path":path});
        if environment_id == self.local_id {
            let target = self.identity().await?;
            let response = self.local_request(json!({"jsonrpc":"2.0","id":"preview","method":"extension/invoke","params":{"extensionId":"workspace","command":"preview.read","arguments":params},"expectedRuntime":target})).await?;
            if let Some(error) = response.get("error") {
                return Err(Error::Provider(
                    error["message"].as_str().unwrap_or("preview failed").into(),
                ));
            }
            Ok(response["result"].clone())
        } else {
            self.manager
                .call_node(
                    environment_id,
                    "extension/invoke",
                    json!({"extensionId":"workspace","command":"preview.read","arguments":params}),
                )
                .await
        }
    }
}

fn local_endpoint(endpoint: &crate::runtime_host::Endpoint) -> String {
    #[cfg(any(unix, windows))]
    if let Some(path) = &endpoint.local_socket {
        return storage::NodeTransport::LocalSocket { path: path.to_string_lossy().into_owned() }.endpoint();
    }
    endpoint.url.clone()
}
