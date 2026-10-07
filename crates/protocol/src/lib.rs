use mpai::*;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::RwLock;
mod schema;
pub use schema::protocol_schema;

pub const PROTOCOL_VERSION: &str = "1";
#[derive(Clone)]
pub struct RuntimeService {
    runtime: Runtime,
    agents: Arc<RwLock<BTreeMap<String, Agent>>>,
}
impl RuntimeService {
    pub fn new(runtime: Runtime) -> Self {
        Self {
            runtime,
            agents: Arc::default(),
        }
    }
    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }
    pub async fn shutdown(&self) -> Result<()> {
        self.runtime.shutdown().await?;
        self.agents.write().await.clear();
        Ok(())
    }
    async fn agent(&self, p: &Value) -> Result<Agent> {
        let id = required(p, "sessionId")?;
        self.runtime
            .agent(id)
            .await
            .ok_or_else(|| Error::Invalid(format!("session is not open: {id}")))
    }
    async fn retain(&self, a: Agent) -> Result<Value> {
        let snapshot = a.snapshot().await;
        self.agents.write().await.insert(snapshot.id.clone(), a);
        Ok(serde_json::to_value(snapshot)?)
    }
    pub async fn invoke(&self, method: &str, p: Value) -> Result<Value> {
        match method {
            "runtime/initialize" => Ok(
                json!({"name":"mpai","version":env!("CARGO_PKG_VERSION"),"protocolVersion":PROTOCOL_VERSION,"methods":METHODS,"eventHistoryLimit":4096,"transcriptDurability":self.runtime.transcript_durability()}),
            ),
            "runtime/schema" => Ok(protocol_schema()),
            "runtime/discover" => Ok(serde_json::to_value(self.runtime.discover().await?)?),
            "runtime/configuration" => {
                let options: SpawnOptions =
                    serde_json::from_value(p).map_err(|error| Error::Invalid(error.to_string()))?;
                self.runtime.preview_configuration(options).await
            }
            "runtime/probe" => Ok(serde_json::to_value(
                self.runtime.probe(required(&p, "harness")?).await?,
            )?),
            "runtime/history" => {
                let query: NativeHistoryQuery =
                    serde_json::from_value(p).map_err(|error| Error::Invalid(error.to_string()))?;
                Ok(serde_json::to_value(
                    self.runtime.native_history(query).await?,
                )?)
            }
            "runtime/history/read" | "session/import" => {
                let native_id = required(&p, "nativeId")?.to_string();
                let query: NativeHistoryQuery =
                    serde_json::from_value(p).map_err(|error| Error::Invalid(error.to_string()))?;
                if method == "session/import" {
                    Ok(serde_json::to_value(
                        self.runtime
                            .import_native_history(query, &native_id)
                            .await?,
                    )?)
                } else {
                    Ok(serde_json::to_value(
                        self.runtime.read_native_history(query, &native_id).await?,
                    )?)
                }
            }
            "runtime/quotas" => {
                let query: AccountQuery =
                    serde_json::from_value(p).map_err(|error| Error::Invalid(error.to_string()))?;
                self.runtime.account_quotas(query).await
            }
            "runtime/quotas/all" => {
                let queries: Vec<AccountQuery> = serde_json::from_value(p["queries"].clone())
                    .map_err(|error| Error::Invalid(error.to_string()))?;
                Ok(serde_json::to_value(
                    self.runtime.account_quotas_all(queries).await?,
                )?)
            }
            "runtime/generate" => {
                let options: SpawnOptions =
                    serde_json::from_value(p.clone()).map_err(|e| Error::Invalid(e.to_string()))?;
                let timeout = p
                    .get("timeoutMs")
                    .map(|value| {
                        value
                            .as_u64()
                            .filter(|value| *value > 0 && *value <= 600_000)
                            .ok_or_else(|| {
                                Error::Invalid("timeoutMs must be between 1 and 600000".into())
                            })
                    })
                    .transpose()?
                    .unwrap_or(120_000);
                Ok(Value::String(
                    self.runtime
                        .generate_with_timeout(
                            options,
                            prompt(&p)?,
                            std::time::Duration::from_millis(timeout),
                        )
                        .await?,
                ))
            }
            "extension/list" => Ok(serde_json::to_value(self.runtime.extensions())?),
            "extension/invoke" => {
                self.runtime
                    .invoke_extension(
                        required(&p, "extensionId")?,
                        required(&p, "command")?,
                        p.get("arguments").cloned().unwrap_or_else(|| json!({})),
                    )
                    .await
            }
            "session/tree" => Ok(serde_json::to_value(
                self.runtime
                    .session_tree(required(&p, "sessionId")?)
                    .await?,
            )?),
            "session/list" => Ok(serde_json::to_value(self.runtime.sessions().await?)?),
            "agent/spawn" | "session/create" => {
                let o: SpawnOptions =
                    serde_json::from_value(p).map_err(|e| Error::Invalid(e.to_string()))?;
                self.retain(self.runtime.spawn(o).await?).await
            }
            "session/resume" => {
                let id = required(&p, "sessionId")?;
                let agent = if let Some(execution) = p.get("execution") {
                    self.runtime
                        .resume_with(
                            id,
                            serde_json::from_value(execution.clone())
                                .map_err(|e| Error::Invalid(e.to_string()))?,
                        )
                        .await?
                } else {
                    self.runtime.resume(id).await?
                };
                self.retain(agent).await
            }
            "session/fork" => {
                let id = required(&p, "sessionId")?;
                let message_id = p
                    .get("messageId")
                    .map(|value| {
                        value
                            .as_str()
                            .ok_or_else(|| Error::Invalid("messageId must be a string".into()))
                    })
                    .transpose()?;
                let execution = p
                    .get("execution")
                    .map(|value| {
                        serde_json::from_value(value.clone())
                            .map_err(|error| Error::Invalid(error.to_string()))
                    })
                    .transpose()?;
                let position = p
                    .get("position")
                    .map(|value| {
                        value
                            .as_str()
                            .ok_or_else(|| Error::Invalid("position must be a string".into()))
                    })
                    .transpose()?
                    .unwrap_or("after_assistant");
                if !matches!(position, "after_assistant" | "before_user") {
                    return Err(Error::Invalid("invalid fork position".into()));
                }
                if position == "before_user" {
                    let message_id = message_id
                        .ok_or_else(|| Error::Invalid("before_user requires messageId".into()))?;
                    return self
                        .retain(
                            self.runtime
                                .fork_before_user(id, message_id, execution)
                                .await?,
                        )
                        .await;
                }
                let agent = match (message_id, execution) {
                    (Some(message_id), Some(execution)) => {
                        self.runtime.fork_at_with(id, message_id, execution).await?
                    }
                    (Some(message_id), None) => self.runtime.fork_at(id, message_id).await?,
                    (None, Some(execution)) => self.runtime.fork_with(id, execution).await?,
                    (None, None) => self.runtime.fork(id).await?,
                };
                self.retain(agent).await
            }
            "session/get" => Ok(serde_json::to_value(
                self.runtime.snapshot(required(&p, "sessionId")?).await?,
            )?),
            "session/metadata" => {
                let update: SessionMetadataUpdate =
                    serde_json::from_value(p.clone()).map_err(|e| Error::Invalid(e.to_string()))?;
                Ok(serde_json::to_value(
                    self.runtime
                        .update_metadata(required(&p, "sessionId")?, update)
                        .await?,
                )?)
            }
            "session/transcript" => Ok(serde_json::to_value(
                self.runtime
                    .transcript(
                        required(&p, "sessionId")?,
                        p["afterSequence"].as_u64().unwrap_or(0),
                        event_limit(&p)?,
                    )
                    .await?,
            )?),
            "session/queue/list" => Ok(serde_json::to_value(
                self.runtime
                    .snapshot(required(&p, "sessionId")?)
                    .await?
                    .queue,
            )?),
            "session/queue" => Ok(serde_json::to_value(
                self.agent(&p).await?.enqueue(prompt(&p)?).await?,
            )?),
            "session/queue/update" => {
                self.agent(&p)
                    .await?
                    .update_queued(required(&p, "id")?, prompt(&p)?)
                    .await?;
                Ok(Value::Null)
            }
            "session/queue/remove" => {
                self.agent(&p)
                    .await?
                    .remove_queued(required(&p, "id")?)
                    .await?;
                Ok(Value::Null)
            }
            "session/queue/pause" => {
                let paused = p["paused"]
                    .as_bool()
                    .ok_or_else(|| Error::Invalid("paused must be boolean".into()))?;
                self.agent(&p).await?.pause_queue(paused).await?;
                Ok(Value::Null)
            }
            "session/events" => {
                let a = self.agent(&p).await?;
                let after = p["afterSequence"].as_u64().unwrap_or(0);
                let limit = event_limit(&p)?;
                Ok(serde_json::to_value(
                    a.replay(after)
                        .await?
                        .into_iter()
                        .take(limit)
                        .collect::<Vec<_>>(),
                )?)
            }
            "session/prompt" => {
                self.agent(&p).await?.prompt(prompt(&p)?).await?;
                Ok(json!({"accepted":true}))
            }
            "session/steer" => {
                self.agent(&p).await?.steer(prompt(&p)?).await?;
                Ok(json!({"accepted":true}))
            }
            "session/cancel" => {
                let agent = self.agent(&p).await?;
                if let Some(turn) = p.get("turnId") {
                    agent
                        .cancel_turn(
                            turn.as_str()
                                .ok_or_else(|| Error::Invalid("turnId must be a string".into()))?,
                        )
                        .await?;
                } else {
                    agent.cancel().await?;
                }
                Ok(Value::Null)
            }
            "permission/respond" => {
                let r: PermissionResponse =
                    serde_json::from_value(p.clone()).map_err(|e| Error::Invalid(e.to_string()))?;
                self.agent(&p).await?.respond_permission(r).await?;
                Ok(Value::Null)
            }
            "agent/configure" => self.agent(&p).await?.configure(p["config"].clone()).await,
            "agent/models" => self.agent(&p).await?.models().await,
            "agent/configuration" => self.agent(&p).await?.configuration().await,
            "agent/commands" => self.agent(&p).await?.slash_commands().await,
            "agent/quotas" => self.agent(&p).await?.quotas().await,
            "session/rollback" => {
                let n = p["turns"]
                    .as_u64()
                    .filter(|n| *n > 0)
                    .ok_or_else(|| Error::Invalid("turns must be positive".into()))?;
                let scope = p
                    .get("scope")
                    .map(|value| {
                        serde_json::from_value(value.clone())
                            .map_err(|error| Error::Invalid(error.to_string()))
                    })
                    .transpose()?
                    .unwrap_or_default();
                self.agent(&p).await?.rollback_with(n, scope).await?;
                Ok(Value::Null)
            }
            "session/compact" => {
                self.agent(&p).await?.compact().await?;
                Ok(Value::Null)
            }
            "session/close" | "agent/kill" => {
                let a = self.agent(&p).await?;
                if method == "agent/kill" {
                    a.kill().await?;
                } else {
                    a.close().await?;
                }
                self.agents.write().await.remove(required(&p, "sessionId")?);
                Ok(Value::Null)
            }
            _ => Err(Error::Unsupported(format!("method {method}"))),
        }
    }
    /// JSON-RPC 2.0 requests, notifications and batches share one dispatcher.
    pub async fn handle(&self, request: Value) -> Option<Value> {
        if let Some(batch) = request.as_array() {
            if batch.is_empty() {
                return Some(rpc_error(Value::Null, -32600, "empty batch"));
            }
            let mut responses = vec![];
            for r in batch {
                if let Some(response) = self.single(r.clone()).await {
                    responses.push(response);
                }
            }
            if responses.is_empty() {
                None
            } else {
                Some(Value::Array(responses))
            }
        } else {
            self.single(request).await
        }
    }
    async fn single(&self, r: Value) -> Option<Value> {
        let id = r.get("id").cloned();
        if !r.is_object()
            || r["jsonrpc"] != "2.0"
            || r["method"].as_str().is_none()
            || id
                .as_ref()
                .is_some_and(|v| !v.is_null() && !v.is_string() && !v.is_number())
            || r.get("params")
                .is_some_and(|p| !p.is_object() && !p.is_array())
        {
            return Some(rpc_error(Value::Null, -32600, "invalid JSON-RPC request"));
        }
        let method = r["method"].as_str().unwrap();
        let result = if !METHODS.contains(&method) {
            Err(Error::Unsupported(format!("method {method}")))
        } else if r["params"].is_array() {
            Err(Error::Invalid("named parameters required".into()))
        } else {
            self.invoke(
                method,
                r.get("params").cloned().unwrap_or_else(|| json!({})),
            )
            .await
        };
        id.map(|id| match result {
            Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
            Err(e) => {
                let code = match &e {
                    Error::Unsupported(_) if !METHODS.contains(&method) => -32601,
                    Error::Invalid(_) | Error::Json(_) => -32602,
                    Error::Unsupported(_) => -32001,
                    Error::Busy => -32002,
                    Error::Closed => -32003,
                    _ => -32000,
                };
                rpc_error(id, code, &e.to_string())
            }
        })
    }
}
pub const METHODS: &[&str] = &[
    "runtime/initialize",
    "runtime/schema",
    "runtime/discover",
    "runtime/configuration",
    "runtime/probe",
    "runtime/history",
    "runtime/history/read",
    "session/import",
    "runtime/quotas",
    "runtime/quotas/all",
    "runtime/generate",
    "extension/list",
    "extension/invoke",
    "agent/spawn",
    "session/create",
    "session/list",
    "session/tree",
    "session/resume",
    "session/fork",
    "session/get",
    "session/metadata",
    "session/transcript",
    "session/queue",
    "session/queue/list",
    "session/queue/update",
    "session/queue/remove",
    "session/queue/pause",
    "session/events",
    "session/prompt",
    "session/steer",
    "session/cancel",
    "permission/respond",
    "agent/configure",
    "agent/models",
    "agent/configuration",
    "agent/commands",
    "agent/quotas",
    "session/rollback",
    "session/compact",
    "session/close",
    "agent/kill",
];
fn event_limit(p: &Value) -> Result<usize> {
    Ok(p.get("limit")
        .map(|v| {
            v.as_u64()
                .filter(|n| *n > 0 && *n <= 4096)
                .ok_or_else(|| Error::Invalid("limit must be between 1 and 4096".into()))
        })
        .transpose()?
        .unwrap_or(4096) as usize)
}
fn required<'a>(p: &'a Value, key: &str) -> Result<&'a str> {
    p[key]
        .as_str()
        .ok_or_else(|| Error::Invalid(format!("missing string parameter {key}")))
}
fn prompt(p: &Value) -> Result<Prompt> {
    if let Some(text) = p["text"].as_str() {
        Ok(text.into())
    } else if let Some(content) = p.get("content") {
        Ok(Prompt {
            content: serde_json::from_value(content.clone())
                .map_err(|e| Error::Invalid(e.to_string()))?,
        })
    } else {
        Err(Error::Invalid("provide text or content".into()))
    }
}
pub fn notification(event: RuntimeEvent) -> Value {
    json!({"jsonrpc":"2.0","method":"session/event","params":{"sessionId":event.session_id,"event":event}})
}
pub fn rpc_error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}
