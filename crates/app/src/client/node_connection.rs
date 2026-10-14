//! Client-owned node registration and verified common-protocol access.
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{path::Path, time::Duration};
use storage::{NodeConnection, NodeTransport, SqliteSessionStore};
use swarmie::{Error, LocalStoreIdentity, Result, RuntimeIdentity};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

fn endpoint(url: &str) -> Result<reqwest::Url> {
    let mut parsed =
        reqwest::Url::parse(url).map_err(|_| Error::Invalid("invalid daemon URL".into()))?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(Error::Invalid(
            "daemon URL must not contain credentials".into(),
        ));
    }
    if (parsed.query().is_some() && !matches!(parsed.scheme(), "ssh" | "pipe"))
        || parsed.fragment().is_some()
    {
        return Err(Error::Invalid(
            "node endpoint must not contain a query or fragment".into(),
        ));
    }
    if !matches!(
        parsed.scheme(),
        "http" | "https" | "ws" | "wss" | "unix" | "pipe" | "ssh"
    ) {
        return Err(Error::Invalid(
            "daemon URL requires HTTP(S) or WS(S)".into(),
        ));
    }
    if matches!(parsed.scheme(), "http" | "https") && parsed.path() == "/" {
        parsed.set_path("/rpc");
    }
    Ok(parsed)
}
pub async fn rpc(url: &str, method: &str, params: Value, token: Option<&str>) -> Result<Value> {
    rpc_targeted(url, method, params, token, None).await
}
pub async fn rpc_targeted(
    url: &str,
    method: &str,
    params: Value,
    token: Option<&str>,
    target: Option<&RuntimeIdentity>,
) -> Result<Value> {
    let mut request = json!({"jsonrpc":"2.0","id":1,"method":method,"params":params});
    if let Some(target) = target {
        request["expectedRuntime"] = serde_json::to_value(target)?;
    }
    let response = rpc_message(url, request, token).await?;
    rpc_result(response)
}
fn rpc_result(response: Value) -> Result<Value> {
    if let Some(error) = response.get("error") {
        return Err(Error::Provider(
            error["message"]
                .as_str()
                .unwrap_or("remote RPC failed")
                .into(),
        ));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| Error::Provider("daemon returned no RPC result".into()))
}
async fn rpc_message(url: &str, request: Value, token: Option<&str>) -> Result<Value> {
    let parsed = endpoint(url)?;
    let request_id = request["id"].clone();
    let response = match parsed.scheme() {
        "http" | "https" => {
            let mut builder = reqwest::Client::builder()
                .timeout(Duration::from_secs(120))
                .redirect(reqwest::redirect::Policy::none());
            if parsed.host_str().is_some_and(|host| {
                host.eq_ignore_ascii_case("localhost")
                    || host
                        .trim_matches(['[', ']'])
                        .parse::<std::net::IpAddr>()
                        .is_ok_and(|ip| ip.is_loopback())
            }) {
                builder = builder.no_proxy();
            }
            let client = builder
                .build()
                .map_err(|error| Error::Provider(error.to_string()))?;
            let mut request = client.post(parsed).json(&request);
            if let Some(token) = token {
                request = request.bearer_auth(token);
            }
            let response = request
                .send()
                .await
                .map_err(|error| Error::Provider(error.to_string()))?;
            if !response.status().is_success() {
                return Err(Error::Provider(format!(
                    "daemon HTTP status {}",
                    response.status()
                )));
            }
            response
                .json::<Value>()
                .await
                .map_err(|error| Error::Provider(error.to_string()))?
        }
        "ws" | "wss" => {
            let mut connection = url
                .into_client_request()
                .map_err(|error| Error::Invalid(error.to_string()))?;
            if let Some(token) = token {
                connection.headers_mut().insert(
                    "authorization",
                    format!("Bearer {token}")
                        .parse()
                        .map_err(|_| Error::Invalid("invalid bearer token".into()))?,
                );
            }
            let (mut socket, _) = tokio::time::timeout(
                Duration::from_secs(15),
                tokio_tungstenite::connect_async(connection),
            )
            .await
            .map_err(|_| Error::Timeout("remote connection".into()))?
            .map_err(|error| Error::Provider(error.to_string()))?;
            socket
                .send(Message::Text(request.to_string().into()))
                .await
                .map_err(|error| Error::Provider(error.to_string()))?;
            let response = tokio::time::timeout(Duration::from_secs(120), async {
                while let Some(frame) = socket.next().await {
                    let frame = frame.map_err(|error| Error::Provider(error.to_string()))?;
                    if let Message::Text(text) = frame {
                        let response: Value = serde_json::from_str(&text)?;
                        if response["id"] == request_id {
                            return Ok(response);
                        }
                    }
                }
                Err(Error::Closed)
            })
            .await
            .map_err(|_| Error::Timeout("remote request".into()))??;
            let _ = socket.close(None).await;
            response
        }
        "unix" | "pipe" | "ssh" => {
            let (events, _) = tokio::sync::broadcast::channel(16);
            let peer = crate::node_channel::Channel::connect(url, token, None, events).await?;
            peer.request(request).await?
        }
        _ => return Err(Error::Invalid("unsupported node transport".into())),
    };
    if response["jsonrpc"] != "2.0"
        || response["id"] != request_id
        || !(response.get("result").is_some() ^ response.get("error").is_some())
    {
        return Err(Error::Provider("invalid remote RPC response".into()));
    }
    Ok(response)
}
pub fn handshake(value: &Value, expected: Option<&LocalStoreIdentity>) -> Result<RuntimeIdentity> {
    if value["name"] != "swarmie" || value["protocolVersion"] != protocol::PROTOCOL_VERSION {
        return Err(Error::Invalid(
            "endpoint has an incompatible runtime protocol".into(),
        ));
    }
    let target: RuntimeIdentity = serde_json::from_value(value["runtimeIdentity"].clone())
        .map_err(|_| Error::Invalid("endpoint has no valid runtime identity".into()))?;
    for id in [
        &target.identity.store_id,
        &target.identity.environment_id,
        &target.identity.owner_scope_id,
        &target.incarnation_id,
    ] {
        if id.is_empty() || id.len() > 128 || id.chars().any(char::is_control) {
            return Err(Error::Invalid(
                "endpoint has no valid runtime identity".into(),
            ));
        }
    }
    if expected.is_some_and(|expected| expected != &target.identity) {
        return Err(Error::Invalid("environment identity changed; remove the saved connection before pairing a different environment".into()));
    }
    Ok(target)
}

pub fn validate_name(name: &str) -> Result<()> {
    if name.trim().is_empty() || name.len() > 100 || name.chars().any(char::is_control) {
        Err(Error::Invalid(
            "connection name must contain 1 to 100 printable characters".into(),
        ))
    } else {
        Ok(())
    }
}
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
pub fn public_connection(connection: &NodeConnection) -> Value {
    json!({"name":connection.name,"url":connection.transport.endpoint(),"sshHost":match &connection.transport { NodeTransport::Ssh {host,..}=>Some(host),_=>None },"identity":connection.identity,"authenticated":connection.credential_ref.is_some(),"revision":connection.revision,"createdAtMs":connection.created_at_ms,"lastVerifiedAtMs":connection.last_verified_at_ms})
}
#[derive(Clone)]
pub struct ConnectionManager {
    store: SqliteSessionStore,
    channels: crate::node_channel::Channels,
    events: tokio::sync::broadcast::Sender<Value>,
}
impl ConnectionManager {
    pub async fn open(root: &Path) -> Result<Self> {
        Ok(Self {
            store: SqliteSessionStore::new(root).await?,
            channels: Default::default(),
            events: tokio::sync::broadcast::channel(512).0,
        })
    }
    pub async fn list(&self) -> Result<Vec<NodeConnection>> {
        self.store.node_connections().await
    }
    pub async fn find(&self, name: &str) -> Result<Option<NodeConnection>> {
        Ok(self
            .list()
            .await?
            .into_iter()
            .find(|connection| connection.name == name))
    }
    pub async fn catalog(&self) -> Result<storage::NodeCatalog> {
        self.store.node_catalog().await
    }
    pub async fn remember_local(&self, identity: &RuntimeIdentity) -> Result<()> {
        self.store
            .register_local_node(identity.identity.clone(), now())
            .await
    }
    pub async fn pair(
        &self,
        name: String,
        url: String,
        token: Option<String>,
    ) -> Result<NodeConnection> {
        self.pair_inner(name, url, token, false, None).await
    }
    pub async fn pair_new(
        &self,
        name: String,
        url: String,
        token: Option<String>,
    ) -> Result<NodeConnection> {
        self.pair_inner(name, url, token, true, None).await
    }
    pub async fn pair_prepared(
        &self,
        name: String,
        url: String,
        token: Option<String>,
        target: &RuntimeIdentity,
    ) -> Result<NodeConnection> {
        self.pair_inner(name, url, token, true, Some(target)).await
    }
    async fn pair_inner(
        &self,
        name: String,
        url: String,
        token: Option<String>,
        new: bool,
        prepared: Option<&RuntimeIdentity>,
    ) -> Result<NodeConnection> {
        validate_name(&name)?;
        let previous = self.find(&name).await?;
        if new && previous.is_some() {
            return Err(Error::Invalid("connection name is already in use".into()));
        }
        let url = endpoint(&url)?.to_string();
        let initialized = rpc(&url, "runtime/initialize", json!({}), token.as_deref()).await?;
        let identity = handshake(
            &initialized,
            previous.as_ref().map(|connection| &connection.identity),
        )?;
        if prepared.is_some_and(|target| target != &identity) {
            return Err(Error::Invalid(
                "remote runtime changed since readiness verification; check again".into(),
            ));
        }
        let transport = match endpoint(&url)?.scheme() {
            "http" | "https" => NodeTransport::Http { url },
            "ws" | "wss" => NodeTransport::Websocket { url },
            "unix" => NodeTransport::LocalSocket {
                path: crate::node_channel::socket_path(&url)?,
            },
            "ssh" => {
                let (host, socket_path) = crate::node_channel::ssh_route(&url)?;
                NodeTransport::Ssh { host, socket_path }
            }
            _ => return Err(Error::Invalid("unsupported node transport".into())),
        };
        self.save_verified(
            name,
            transport,
            &identity,
            token.as_deref(),
            previous.as_ref().map_or(0, |c| c.revision),
        )
        .await
    }
    pub async fn save_verified(
        &self,
        name: String,
        transport: NodeTransport,
        identity: &RuntimeIdentity,
        token: Option<&str>,
        expected_revision: u64,
    ) -> Result<NodeConnection> {
        let previous = self.find(&name).await?;
        let credentials = crate::node_credentials::Credentials::new(self.store.root());
        let reference = token.map(|token| credentials.save(token)).transpose()?;
        let incoming = NodeConnection {
            name,
            identity: identity.identity.clone(),
            transport,
            credential_ref: reference.clone(),
            revision: 0,
            created_at_ms: 0,
            last_verified_at_ms: now(),
        };
        match self
            .store
            .register_node_connection(incoming, expected_revision)
            .await
        {
            Ok(connection) => {
                if let Some(old) = previous.and_then(|c| c.credential_ref) {
                    let _ = credentials.remove(&old);
                }
                Ok(connection)
            }
            Err(error) => {
                if let Some(reference) = reference {
                    let _ = credentials.remove(&reference);
                }
                Err(error)
            }
        }
    }
    pub async fn remove(&self, name: &str) -> Result<()> {
        let record = self
            .find(name)
            .await?
            .ok_or_else(|| Error::Invalid("unknown connection".into()))?;
        self.remove_revision(name, record.revision).await
    }
    pub async fn remove_revision(&self, name: &str, revision: u64) -> Result<()> {
        let removed = self
            .store
            .remove_node_connection(name.to_owned(), revision)
            .await?;
        self.channels
            .remove(&[format!("{}:{}", removed.name, removed.revision)])
            .await;
        if let Some(reference) = removed.credential_ref {
            crate::node_credentials::Credentials::new(self.store.root())
                .remove(&reference)
                .map_err(|error| {
                    Error::Storage(format!(
                        "node connection was removed, but credential cleanup failed: {error}"
                    ))
                })?;
        }
        Ok(())
    }
    pub async fn select(&self, environment_id: String) -> Result<()> {
        self.store.select_node(environment_id).await
    }
    pub async fn forward(&self, environment_id: &str, request: Value) -> Result<Value> {
        let route = self
            .list()
            .await?
            .into_iter()
            .find(|route| route.identity.environment_id == environment_id)
            .ok_or_else(|| Error::Invalid("node has no registered connection".into()))?;
        let supplied: Option<RuntimeIdentity> = request
            .get("expectedRuntime")
            .map(|value| serde_json::from_value(value.clone()))
            .transpose()
            .map_err(|_| Error::Invalid("invalid expected runtime".into()))?;
        if request["method"] != "runtime/initialize" && supplied.is_none() {
            return Err(Error::Invalid(
                "node commands require a verified runtime target".into(),
            ));
        }
        if supplied
            .as_ref()
            .is_some_and(|target| target.identity != route.identity)
        {
            return Err(Error::Invalid("request targets another node".into()));
        }
        let token = route
            .credential_ref
            .as_ref()
            .map(|reference| {
                crate::node_credentials::Credentials::new(self.store.root()).read(reference)
            })
            .transpose()?;
        let url = route.transport.endpoint();
        let initialized = self
            .channels
            .request(
                format!("{}:{}", route.name, route.revision),
                &url,
                token.as_deref(),
                &route.identity,
                &self.events,
                json!({"jsonrpc":"2.0","id":"verify","method":"runtime/initialize","params":{}}),
            )
            .await?;
        let target = handshake(&rpc_result(initialized.clone())?, Some(&route.identity))?;
        self.store
            .observe_node_connection(route.name.clone(), route.revision, now())
            .await?;
        if request["method"] == "runtime/initialize" && supplied.is_none() {
            return Ok(
                json!({"jsonrpc":"2.0","id":request["id"],"result":rpc_result(initialized)?}),
            );
        }
        // Preserve the caller's exact incarnation; verification must not retarget a pending effect.
        if supplied.as_ref() != Some(&target) {
            return Ok(json!({"jsonrpc":"2.0","id":request["id"],"error":{
                "code":-32005,"message":"runtime_target_mismatch", "data":{"effectKnowledge":"none"}}}));
        }
        self.channels
            .request(
                format!("{}:{}", route.name, route.revision),
                &url,
                token.as_deref(),
                &route.identity,
                &self.events,
                request,
            )
            .await
    }
    pub async fn disconnect(&self, environment_id: &str) {
        if let Ok(routes) = self.list().await {
            self.channels
                .remove(
                    &routes
                        .iter()
                        .filter(|r| r.identity.environment_id == environment_id)
                        .map(|r| format!("{}:{}", r.name, r.revision))
                        .collect::<Vec<_>>(),
                )
                .await;
        }
    }
    pub fn publish(&self, event: Value) {
        let _ = self.events.send(event);
    }
    pub fn events(&self) -> tokio::sync::broadcast::Receiver<Value> {
        self.events.subscribe()
    }
    pub async fn close(&self) {
        self.channels.clear().await;
    }
    pub async fn local_request(
        &self,
        url: &str,
        identity: &LocalStoreIdentity,
        request: Value,
    ) -> Result<Value> {
        self.channels
            .request("local".into(), url, None, identity, &self.events, request)
            .await
    }

    pub async fn call_node(
        &self,
        environment_id: &str,
        method: &str,
        params: Value,
    ) -> Result<Value> {
        let initialized = self.forward(environment_id, json!({"jsonrpc":"2.0","id":"preview-initialize","method":"runtime/initialize","params":{}})).await?;
        let target = handshake(&rpc_result(initialized)?, None)?;
        rpc_result(self.forward(environment_id, json!({"jsonrpc":"2.0","id":"preview","method":method,"params":params,"expectedRuntime":target})).await?)
    }
    pub async fn call(&self, name: &str, method: &str, params: Value) -> Result<Value> {
        let record = self
            .find(name)
            .await?
            .ok_or_else(|| Error::Invalid("unknown connection".into()))?;
        self.call_node(&record.identity.environment_id, method, params)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn registry_credentials_are_private_and_failed_update_preserves_the_original_reference() {
        let root = tempfile::tempdir().unwrap();
        let manager = ConnectionManager::open(root.path()).await.unwrap();
        let identity = swarmie::Runtime::default().identity().await.unwrap();
        let transport = NodeTransport::Http {
            url: "http://127.0.0.1:7480/rpc".into(),
        };
        let original = manager
            .save_verified(
                "node".into(),
                transport.clone(),
                &identity,
                Some("private-original-token"),
                0,
            )
            .await
            .unwrap();
        assert!(
            !serde_json::to_string(&manager.catalog().await.unwrap())
                .unwrap()
                .contains("private-original-token")
        );
        assert!(
            !public_connection(&original)
                .to_string()
                .contains("private-original-token")
        );
        let credentials = crate::node_credentials::Credentials::new(root.path());
        assert_eq!(
            credentials
                .read(original.credential_ref.as_ref().unwrap())
                .unwrap(),
            "private-original-token"
        );
        assert!(
            manager
                .save_verified(
                    "node".into(),
                    transport,
                    &identity,
                    Some("private-replacement-token"),
                    0
                )
                .await
                .is_err()
        );
        assert_eq!(
            manager.find("node").await.unwrap().unwrap().credential_ref,
            original.credential_ref
        );
        assert_eq!(
            std::fs::read_dir(root.path().join("credentials/node-connections"))
                .unwrap()
                .count(),
            1
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let path = root
                .path()
                .join("credentials/node-connections")
                .join(format!(
                    "{}.json",
                    original.credential_ref.as_ref().unwrap()
                ));
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
            assert_eq!(
                std::fs::metadata(root.path().join("credentials/node-connections"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
        manager.remove("node").await.unwrap();
        assert!(manager.list().await.unwrap().is_empty());
        assert!(!manager.catalog().await.unwrap().nodes[0].registered);
        assert_eq!(
            std::fs::read_dir(root.path().join("credentials/node-connections"))
                .unwrap()
                .count(),
            0
        );
        assert!(!root.path().join("connections.json").exists());
    }
}
