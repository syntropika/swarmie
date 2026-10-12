//! Durable native submission ownership, independent of presentation caches.
use crate::node_gateway::NodeGateway;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::Arc};
use swarmie::{
    DeliveryKnowledge, Error, LocalStoreIdentity, Result, RuntimeIdentity, SessionStore,
    SubmissionReceipt,
};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct ClientSubmissions {
    nodes: NodeGateway,
    store: storage::SqliteSessionStore,
    sessions: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    writing: Arc<Mutex<()>>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    key: String,
    session_id: String,
    fingerprint: String,
    receipt: SubmissionReceipt,
}
impl ClientSubmissions {
    pub fn new(nodes: NodeGateway, store: storage::SqliteSessionStore) -> Self {
        Self {
            nodes,
            store,
            sessions: Default::default(),
            writing: Default::default(),
        }
    }
    fn storage_key(node: &LocalStoreIdentity) -> Result<String> {
        Ok(format!(
            "client-submissions:{:x}",
            Sha256::digest(serde_json::to_vec(node)?)
        ))
    }
    async fn read(&self, node: &LocalStoreIdentity) -> Result<Vec<Entry>> {
        Ok(
            match self
                .store
                .extension_state(&Self::storage_key(node)?)
                .await?
            {
                Some(value) => serde_json::from_value(value)?,
                None => Vec::new(),
            },
        )
    }
    async fn save_entry(&self, node: &LocalStoreIdentity, entry: Entry) -> Result<()> {
        let _writing = self.writing.lock().await;
        let _file = self
            .file_lock("journal", true)
            .await?
            .expect("blocking journal lock");
        let mut entries = self.read(node).await?;
        if let Some(old) = entries
            .iter_mut()
            .find(|old| old.session_id == entry.session_id && old.key == entry.key)
        {
            *old = entry;
        } else {
            entries.push(entry);
        }
        self.store
            .save_extension_state(&Self::storage_key(node)?, serde_json::to_value(entries)?)
            .await
    }
    async fn session_lock(
        &self,
        target: &RuntimeIdentity,
        session: &str,
    ) -> Result<Arc<Mutex<()>>> {
        let key = format!("{}:{session}", Self::storage_key(&target.identity)?);
        Ok(self.sessions.lock().await.entry(key).or_default().clone())
    }
    async fn file_lock(&self, key: &str, wait: bool) -> Result<Option<std::fs::File>> {
        let path = self.store.root().join(format!(
            "submission-{:x}.lock",
            Sha256::digest(key.as_bytes())
        ));
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            match fs2::FileExt::try_lock_exclusive(&file) {
                Ok(()) => return Ok(Some(file)),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if !wait {
                        return Ok(None);
                    }
                    if tokio::time::Instant::now() >= deadline {
                        return Err(Error::Storage("another client is checking or sending work in this session; try again when it finishes".into()));
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
    async fn session_file(
        &self,
        target: &RuntimeIdentity,
        session: &str,
        wait: bool,
    ) -> Result<Option<std::fs::File>> {
        self.file_lock(
            &format!("{}:{session}", Self::storage_key(&target.identity)?),
            wait,
        )
        .await
    }
    fn target(request: &Value) -> Result<RuntimeIdentity> {
        let target: RuntimeIdentity = serde_json::from_value(request["expectedRuntime"].clone())?;
        if target.incarnation_id.is_empty()
            || [
                &target.identity.environment_id,
                &target.identity.owner_scope_id,
                &target.identity.store_id,
            ]
            .iter()
            .any(|part| part.is_empty() || part.len() > 128 || part.chars().any(char::is_control))
        {
            return Err(Error::Invalid("invalid submission node identity".into()));
        }
        Ok(target)
    }
    fn session(request: &Value) -> Result<&str> {
        request["params"]["sessionId"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 128)
            .ok_or_else(|| Error::Invalid("sessionId is required".into()))
    }
    async fn call(
        &self,
        node: &str,
        target: &RuntimeIdentity,
        method: &str,
        params: Value,
    ) -> Value {
        self.nodes.forward(node, json!({"jsonrpc":"2.0", "id":uuid::Uuid::new_v4().to_string(), "method":method, "params":params, "expectedRuntime":target})).await
    }
    async fn verified(&self, node: &str, target: &RuntimeIdentity) -> Result<()> {
        let response = self
            .call(node, target, "runtime/initialize", json!({}))
            .await;
        if response["result"]["runtimeIdentity"] != serde_json::to_value(target)? {
            return Err(Error::Invalid("submission runtime target mismatch".into()));
        }
        Ok(())
    }
    pub async fn prompt(&self, node: &str, request: Value) -> Result<Value> {
        let target = Self::target(&request)?;
        if target.identity.environment_id != node {
            return Err(Error::Invalid("submission node target mismatch".into()));
        }
        let session = Self::session(&request)?.to_owned();
        let key = request["params"]["idempotencyKey"]
            .as_str()
            .filter(|key| !key.is_empty() && key.len() <= 128)
            .ok_or_else(|| Error::Invalid("idempotencyKey is required".into()))?
            .to_owned();
        let fingerprint = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&request["params"])?)
        );
        let lock = self.session_lock(&target, &session).await?;
        let _session = lock.lock().await;
        let _file = self
            .session_file(&target, &session, true)
            .await?
            .expect("blocking session lock");
        self.verified(node, &target).await?;
        let entries = self.read(&target.identity).await?;
        if let Some(entry) = entries
            .iter()
            .find(|entry| entry.session_id == session && entry.key == key)
        {
            if entry.fingerprint != fingerprint {
                return Err(Error::Invalid(
                    "submission key is already bound to different input".into(),
                ));
            }
            // A duplicate never redispatches an uncertain input, even after a client restart.
            return Ok(json!({"jsonrpc":"2.0","id":request["id"],"result":entry.receipt}));
        }
        if entries
            .iter()
            .any(|entry| entry.session_id == session && unresolved(&entry.receipt))
        {
            return Err(Error::Invalid(
                "check the original submission before sending more work".into(),
            ));
        }
        let mut entry = Entry {
            key: key.clone(), session_id: session.clone(), fingerprint,
            receipt: SubmissionReceipt {
                id: uuid::Uuid::new_v4().to_string(), session_id: session.clone(), idempotency_key: key,
                turn_id: String::new(), delivery: DeliveryKnowledge::Unknown, recovery_required: true,
                outcome: None, error: Some("Delivery confirmation is missing. Check the original submission before sending more work.".into()),
            },
        };
        // Intent is durable before the request can reach the runtime. Crashes preserve uncertainty.
        self.save_entry(&target.identity, entry.clone()).await?;
        let response = self.nodes.forward(node, request.clone()).await;
        if let Some(receipt) = matching_receipt(&response["result"], &entry) {
            entry.receipt = receipt;
        } else if response["error"]["data"]["effectKnowledge"] == "none" {
            entry.receipt.delivery = DeliveryKnowledge::Rejected;
            entry.receipt.recovery_required = false;
            entry.receipt.error = response["error"]["message"].as_str().map(str::to_owned);
        }
        self.save_entry(&target.identity, entry.clone()).await?;
        Ok(json!({"jsonrpc":"2.0", "id":request["id"], "result":entry.receipt}))
    }
    pub async fn recover(&self, node: &str, request: Value) -> Result<Value> {
        let target = Self::target(&request)?;
        if target.identity.environment_id != node {
            return Err(Error::Invalid("submission node target mismatch".into()));
        }
        let session = Self::session(&request)?.to_owned();
        let lock = self.session_lock(&target, &session).await?;
        let _session = lock.lock().await;
        let _file = self
            .session_file(&target, &session, true)
            .await?
            .expect("blocking session lock");
        self.verified(node, &target).await?;
        let reconcile = request["params"]["reconcile"] == true;
        let entries = self.refresh(node, &target, &session, reconcile).await?;
        let snapshot = self
            .call(node, &target, "session/get", json!({"sessionId":session}))
            .await;
        if let Some(error) = snapshot.get("error") {
            return Err(Error::Provider(
                error["message"]
                    .as_str()
                    .unwrap_or("session recovery failed")
                    .into(),
            ));
        }
        Ok(json!({"jsonrpc":"2.0", "id":request["id"], "result":{
            "entries":entries.iter().map(|entry| json!({"key":entry.key,"sessionId":entry.session_id,"receipt":entry.receipt})).collect::<Vec<_>>(), "unresolved":entries.iter().any(|entry| unresolved(&entry.receipt)), "snapshot":snapshot["result"]
        }}))
    }
    async fn refresh(
        &self,
        node: &str,
        target: &RuntimeIdentity,
        session: &str,
        reconcile: bool,
    ) -> Result<Vec<Entry>> {
        let mut entries: Vec<_> = self
            .read(&target.identity)
            .await?
            .into_iter()
            .filter(|entry| entry.session_id == session)
            .collect();
        for entry in &mut entries {
            if !unresolved(&entry.receipt) {
                continue;
            }
            let response = self
                .call(
                    node,
                    target,
                    if reconcile {
                        "session/submission/reconcile"
                    } else {
                        "session/submission"
                    },
                    json!({"sessionId":session,"idempotencyKey":entry.key}),
                )
                .await;
            if response.get("error").is_some() {
                if reconcile {
                    return Err(Error::Provider(
                        response["error"]["message"]
                            .as_str()
                            .unwrap_or("submission reconciliation failed")
                            .into(),
                    ));
                }
                continue;
            }
            // Missing receipts and malformed/cross-session receipts cannot prove rejection.
            if let Some(receipt) = matching_receipt(&response["result"], entry) {
                entry.receipt = receipt;
                self.save_entry(&target.identity, entry.clone()).await?;
            }
        }
        Ok(entries)
    }
    pub async fn observe(&self, node: &str, target: &RuntimeIdentity) -> Result<()> {
        let sessions: std::collections::HashSet<_> = self
            .read(&target.identity)
            .await?
            .into_iter()
            .filter(|entry| unresolved(&entry.receipt))
            .map(|entry| entry.session_id)
            .collect();
        for session in sessions {
            let lock = self.session_lock(target, &session).await?;
            let Ok(_session) = lock.try_lock() else {
                continue;
            };
            let Some(_file) = self.session_file(target, &session, false).await? else {
                continue;
            };
            self.refresh(node, target, &session, false).await?;
        }
        Ok(())
    }
}
fn unresolved(receipt: &SubmissionReceipt) -> bool {
    receipt.recovery_required
        || !matches!(
            receipt.delivery,
            DeliveryKnowledge::Accepted | DeliveryKnowledge::Rejected
        )
}
fn matching_receipt(value: &Value, entry: &Entry) -> Option<SubmissionReceipt> {
    serde_json::from_value::<SubmissionReceipt>(value.clone())
        .ok()
        .filter(|receipt| {
            receipt.session_id == entry.session_id && receipt.idempotency_key == entry.key
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    async fn fixture(root: &std::path::Path) -> (ClientSubmissions, RuntimeIdentity) {
        let service = protocol::RuntimeService::new(
            swarmie::RuntimeBuilder::default()
                .store(storage::SqliteSessionStore::new(root).await.unwrap())
                .build(),
        );
        let target = service.runtime().identity().await.unwrap();
        let nodes = NodeGateway::new(root, service).await.unwrap();
        let cache = storage::SqliteSessionStore::new(root.join("client"))
            .await
            .unwrap();
        (ClientSubmissions::new(nodes, cache), target)
    }
    fn request(target: &RuntimeIdentity, session: &str, key: &str) -> Value {
        json!({"jsonrpc":"2.0","id":1,"method":"session/prompt","expectedRuntime":target,
            "params":{"sessionId":session,"idempotencyKey":key,"text":"Original input"}})
    }
    fn intent(request: &Value) -> Entry {
        Entry {
            key: request["params"]["idempotencyKey"].as_str().unwrap().into(),
            session_id: request["params"]["sessionId"].as_str().unwrap().into(),
            fingerprint: format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&request["params"]).unwrap())
            ),
            receipt: SubmissionReceipt {
                id: "native-intent".into(),
                session_id: request["params"]["sessionId"].as_str().unwrap().into(),
                idempotency_key: request["params"]["idempotencyKey"].as_str().unwrap().into(),
                turn_id: String::new(),
                delivery: DeliveryKnowledge::Unknown,
                recovery_required: true,
                outcome: None,
                error: Some("Unconfirmed input".into()),
            },
        }
    }
    #[tokio::test]
    async fn crash_after_intent_survives_restart_and_never_resends() {
        let root = tempfile::tempdir().unwrap();
        let (client, target) = fixture(root.path()).await;
        let original = request(&target, "session", "original");
        client
            .save_entry(&target.identity, intent(&original))
            .await
            .unwrap();
        // Equivalent to client termination after intent and before a confirmation is retained.
        let restarted = ClientSubmissions::new(
            client.nodes.clone(),
            storage::SqliteSessionStore::new(root.path().join("client"))
                .await
                .unwrap(),
        );
        drop(client);
        let duplicate = restarted
            .prompt(&target.identity.environment_id, original.clone())
            .await
            .unwrap();
        assert_eq!(duplicate["result"]["id"], "native-intent");
        assert_eq!(duplicate["result"]["delivery"], "unknown");
        assert!(
            restarted
                .prompt(
                    &target.identity.environment_id,
                    request(&target, "session", "new")
                )
                .await
                .is_err()
        );
        let mut changed = original.clone();
        changed["params"]["text"] = "Different input".into();
        assert!(
            restarted
                .prompt(&target.identity.environment_id, changed)
                .await
                .is_err()
        );
        // No runtime session exists: missing evidence must retain uncertainty, not reject it.
        let entries = restarted
            .refresh(&target.identity.environment_id, &target, "session", false)
            .await
            .unwrap();
        assert!(unresolved(&entries[0].receipt));
    }
    #[tokio::test]
    async fn stale_runtime_cannot_create_or_read_submission_intent() {
        let root = tempfile::tempdir().unwrap();
        let (client, target) = fixture(root.path()).await;
        let mut stale = target.clone();
        stale.incarnation_id = "earlier-incarnation".into();
        assert!(
            client
                .prompt(
                    &target.identity.environment_id,
                    request(&stale, "session", "key")
                )
                .await
                .is_err()
        );
        assert!(client.read(&target.identity).await.unwrap().is_empty());
        let recover = json!({"jsonrpc":"2.0","id":1,"method":"client/submissions","expectedRuntime":stale,"params":{"sessionId":"session"}});
        assert!(
            client
                .recover(&target.identity.environment_id, recover)
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn independent_clients_preserve_both_journal_entries_and_share_session_guards() {
        let root = tempfile::tempdir().unwrap();
        let (first, target) = fixture(root.path()).await;
        let second = ClientSubmissions::new(
            first.nodes.clone(),
            storage::SqliteSessionStore::new(root.path().join("client"))
                .await
                .unwrap(),
        );
        let a = intent(&request(&target, "first", "a"));
        let b = intent(&request(&target, "second", "b"));
        let (a, b) = tokio::join!(
            first.save_entry(&target.identity, a),
            second.save_entry(&target.identity, b)
        );
        a.unwrap();
        b.unwrap();
        assert_eq!(first.read(&target.identity).await.unwrap().len(), 2);
        let file = first
            .session_file(&target, "first", false)
            .await
            .unwrap()
            .unwrap();
        assert!(
            second
                .session_file(&target, "first", false)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            second
                .session_file(&target, "other", false)
                .await
                .unwrap()
                .is_some()
        );
        drop(file);
        assert!(
            second
                .session_file(&target, "first", false)
                .await
                .unwrap()
                .is_some()
        );
    }
    #[tokio::test]
    async fn native_projection_recovers_a_lost_confirmation_and_pages_a_real_transcript() {
        use swarmie::mock::{MockHarness, MockScript, MockStep};
        let root = tempfile::tempdir().unwrap();
        let mut steps: Vec<_> = (0..600)
            .map(|index| MockStep::Text {
                text: format!("part-{index}"),
                item_id: Some("message".into()),
            })
            .collect();
        steps.push(MockStep::End {
            outcome: swarmie::TurnOutcome::Completed,
        });
        let harness = MockHarness::new(MockScript::default().turn(steps));
        let runtime = swarmie::RuntimeBuilder::default()
            .store(storage::SqliteSessionStore::new(root.path()).await.unwrap())
            .harness(harness.clone())
            .build();
        let agent = runtime
            .spawn(
                serde_json::from_value(json!({"harness":"mock","cwd":root.path(),"config":{}}))
                    .unwrap(),
            )
            .await
            .unwrap();
        let target = runtime.identity().await.unwrap();
        let nodes = NodeGateway::new(root.path(), protocol::RuntimeService::new(runtime))
            .await
            .unwrap();
        let client = ClientSubmissions::new(
            nodes.clone(),
            storage::SqliteSessionStore::new(root.path().join("client"))
                .await
                .unwrap(),
        );
        let session = agent.snapshot().await.id;
        let original = request(&target, &session, "original");
        client
            .save_entry(&target.identity, intent(&original))
            .await
            .unwrap();
        // Runtime received the request, but its response never reached the client's journal.
        let response = nodes
            .forward(&target.identity.environment_id, original)
            .await;
        assert_eq!(response["result"]["delivery"], "accepted");
        for _ in 0..500 {
            let snapshot = agent.snapshot().await;
            if snapshot.state == swarmie::SessionState::Idle && snapshot.sequence > 600 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert!(agent.snapshot().await.sequence > 600, "scripted turn did not finish");
        let projection = crate::client_projection::restore(&nodes, &client, &target.identity.environment_id,
            json!({"jsonrpc":"2.0","id":2,"method":"client/sessionProjection","expectedRuntime":target,"params":{"sessionId":session}})).await.unwrap();
        assert_eq!(projection["result"]["unresolved"], false);
        let events = projection["result"]["events"].as_array().unwrap();
        assert!(events.len() > 256);
        assert_eq!(
            events.last().unwrap()["sequence"],
            projection["result"]["snapshot"]["sequence"]
        );
        assert_eq!(harness.prompts().await.len(), 1);
        assert_eq!(
            client.read(&target.identity).await.unwrap()[0]
                .receipt
                .delivery,
            DeliveryKnowledge::Accepted
        );
    }
    #[tokio::test]
    async fn native_open_blocks_unconfirmed_intent_and_explicit_resume_without_resending() {
        use swarmie::mock::{MockHarness, MockScript};
        let root = tempfile::tempdir().unwrap();
        let harness = MockHarness::new(MockScript::default());
        let runtime = swarmie::RuntimeBuilder::default()
            .store(storage::SqliteSessionStore::new(root.path()).await.unwrap())
            .harness(harness.clone()).build();
        let agent = runtime.spawn(serde_json::from_value(json!({"harness":"mock","cwd":root.path(),"config":{}})).unwrap()).await.unwrap();
        let target = runtime.identity().await.unwrap();
        let nodes = NodeGateway::new(root.path(), protocol::RuntimeService::new(runtime)).await.unwrap();
        let client = ClientSubmissions::new(nodes.clone(), storage::SqliteSessionStore::new(root.path().join("client")).await.unwrap());
        let session = agent.snapshot().await.id;
        let original = request(&target, &session, "unconfirmed");
        client.save_entry(&target.identity, intent(&original)).await.unwrap();
        let operations = crate::client_operations::ClientOperations::new(nodes, client);
        let open = json!({"id":2,"method":"client/sessionOpen","expectedRuntime":target,"params":{"sessionId":session}});
        let response = operations.handle(&target.identity.environment_id, open.clone()).await;
        assert_eq!(response["result"]["activation"]["status"], "blocked");
        let mut explicit = open.clone();
        explicit["params"]["resume"] = "explicit".into();
        let response = operations.handle(&target.identity.environment_id, explicit).await;
        assert_eq!(response["error"]["data"]["effectKnowledge"], "none");
        assert!(harness.prompts().await.is_empty());
        assert_eq!(harness.launches().await.len(), 1);
    }
    #[test]
    fn only_correlated_confirmations_can_resolve_uncertainty() {
        let target: RuntimeIdentity = serde_json::from_value(json!({"identity":{"environmentId":"node","storeId":"store","ownerScopeId":"owner"},"incarnationId":"runtime"})).unwrap();
        let entry = intent(&request(&target, "session", "original"));
        assert!(matching_receipt(&Value::Null, &entry).is_none());
        let mut confirmed = entry.receipt.clone();
        confirmed.delivery = DeliveryKnowledge::Accepted;
        confirmed.recovery_required = false;
        assert!(!unresolved(
            &matching_receipt(&serde_json::to_value(&confirmed).unwrap(), &entry).unwrap()
        ));
        confirmed.session_id = "other-session".into();
        assert!(matching_receipt(&serde_json::to_value(&confirmed).unwrap(), &entry).is_none());
        confirmed.session_id = entry.session_id.clone();
        confirmed.idempotency_key = "other-key".into();
        assert!(matching_receipt(&serde_json::to_value(&confirmed).unwrap(), &entry).is_none());
    }
}
