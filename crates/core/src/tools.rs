use crate::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, Weak},
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostToolGrant {
    pub extension_id: String,
    pub tool_id: String,
    /// Each constrained argument must equal one of the explicitly allowed values.
    #[serde(default)]
    pub argument_constraints: BTreeMap<String, Vec<Value>>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HostTool {
    pub id: String,
    pub description: String,
    pub input_schema: Value,
    pub result_schema: Value,
    pub mutates: bool,
    pub open_world: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HostToolState {
    Prepared,
    Running,
    Completed,
    Cancelled,
    Failed,
}
impl HostToolState {
    fn active(&self) -> bool {
        matches!(self, Self::Prepared | Self::Running)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HostToolInvocation {
    pub id: String,
    pub session_id: String,
    pub turn_id: String,
    pub binding_id: String,
    pub extension_id: String,
    pub tool_id: String,
    pub state: HostToolState,
}
pub struct HostToolContext {
    pub invocation: HostToolInvocation,
    pub working_context: WorkingContext,
    pub cancellation: CancellationToken,
}
impl HostToolContext {
    pub fn check_cancelled(&self) -> Result<()> {
        if self.cancellation.is_cancelled() {
            Err(Error::Cancelled("host tool invocation cancelled".into()))
        } else {
            Ok(())
        }
    }
}
struct Definition {
    grant: HostToolGrant,
    tool: HostTool,
    extension: Arc<dyn RuntimeExtension>,
    input: jsonschema::Validator,
    output: jsonschema::Validator,
}
struct Call {
    executing: bool,
    receipt: HostToolInvocation,
    arguments: Value,
    definition: Arc<Definition>,
    cancellation: CancellationToken,
}
struct ScopeState {
    native: Option<(String, String)>,
    turn: Option<String>,
    pending: bool,
    blocked: bool,
    revoked: bool,
    calls: BTreeMap<String, Call>,
}
pub(crate) struct HostToolScope {
    context: WorkingContext,
    definitions: BTreeMap<String, Arc<Definition>>,
    state: Mutex<ScopeState>,
}
impl HostToolScope {
    pub(crate) fn observe(&self, snapshot: &SessionSnapshot) {
        let mut state = self.state.lock().unwrap();
        let native = (snapshot.native.provider.clone(), snapshot.native.id.clone());
        if state
            .native
            .as_ref()
            .is_some_and(|previous| *previous != native)
        {
            state.revoked = true;
        }
        state.native = Some(native);
        let next = if state.pending && snapshot.turn.is_none() {
            state.turn.clone()
        } else {
            state.pending = false;
            snapshot.turn.clone()
        };
        let blocked = snapshot.execution_policy.dispatch_blocked
            || matches!(
                snapshot.state,
                SessionState::Cancelling | SessionState::Closed | SessionState::Failed
            );
        if state.revoked || blocked || state.turn != next {
            Self::stop(&mut state, None);
        }
        state.turn = next;
        state.blocked = state.blocked || blocked;
    }
    pub(crate) fn begin_turn(&self, turn: &str) {
        let mut state = self.state.lock().unwrap();
        Self::stop(&mut state, None);
        state.turn = Some(turn.into());
        state.pending = true;
        state.blocked = false;
    }
    fn stop(state: &mut ScopeState, id: Option<&str>) {
        for call in state
            .calls
            .values_mut()
            .filter(|call| id.is_none_or(|id| call.receipt.id == id))
        {
            if call.receipt.state.active() {
                call.cancellation.cancel();
                call.receipt.state = HostToolState::Cancelled;
            }
        }
    }
    pub(crate) fn cancel_turn(&self) {
        let mut state = self.state.lock().unwrap();
        state.blocked = true;
        Self::stop(&mut state, None);
    }
    pub(crate) fn revoke(&self) {
        let mut state = self.state.lock().unwrap();
        state.revoked = true;
        Self::stop(&mut state, None);
    }
    fn catalog(&self) -> Result<Value> {
        if self.state.lock().unwrap().revoked {
            return Err(Error::Closed);
        }
        Ok(
            json!({"tools":self.definitions.iter().map(|(name,def)|json!({
            "name":name,"description":def.tool.description,"inputSchema":def.tool.input_schema,
            "annotations":{"readOnlyHint":!def.tool.mutates,"destructiveHint":def.tool.mutates,"openWorldHint":def.tool.open_world}
        })).collect::<Vec<_>>() }),
        )
    }
    fn prepare(&self, name: &str, arguments: Value) -> Result<HostToolInvocation> {
        let def = self
            .definitions
            .get(name)
            .ok_or_else(|| Error::Invalid("host tool is not granted".into()))?;
        if serde_json::to_vec(&arguments)?.len() > 1024 * 1024
            || !arguments.is_object()
            || !def.input.is_valid(&arguments)
        {
            return Err(Error::Invalid(
                "host tool arguments do not match the declared schema".into(),
            ));
        }
        for key in [
            "sessionId",
            "projectId",
            "cwd",
            "directory",
            "turnId",
            "bindingId",
        ] {
            if arguments.get(key).is_some() {
                return Err(Error::Invalid(
                    "host context cannot be supplied as tool arguments".into(),
                ));
            }
        }
        if def
            .grant
            .argument_constraints
            .iter()
            .any(|(key, allowed)| !arguments.get(key).is_some_and(|v| allowed.contains(v)))
        {
            return Err(Error::Rejected(
                "host tool invocation is outside its argument grant".into(),
            ));
        }
        let mut state = self.state.lock().unwrap();
        if state.revoked {
            return Err(Error::Closed);
        }
        if state.blocked || state.native.is_none() {
            return Err(Error::Rejected(
                "host tool context is not executable".into(),
            ));
        }
        let turn = state
            .turn
            .clone()
            .ok_or_else(|| Error::Rejected("host tool requires an active turn".into()))?;
        if state.calls.len() >= 256 {
            state
                .calls
                .retain(|_, call| call.executing || call.receipt.state.active());
            if state.calls.len() >= 256 {
                return Err(Error::Busy);
            }
        }
        let receipt = HostToolInvocation {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: self.context.session_id.clone(),
            turn_id: turn,
            binding_id: self.context.binding_id.clone(),
            extension_id: def.grant.extension_id.clone(),
            tool_id: def.tool.id.clone(),
            state: HostToolState::Prepared,
        };
        state.calls.insert(
            receipt.id.clone(),
            Call {
                executing: false,
                receipt: receipt.clone(),
                arguments,
                definition: def.clone(),
                cancellation: CancellationToken::new(),
            },
        );
        Ok(receipt)
    }
    fn cancel(&self, id: &str) -> Result<HostToolInvocation> {
        let mut state = self.state.lock().unwrap();
        if !state.calls.contains_key(id) {
            return Err(Error::Invalid("unknown host tool invocation".into()));
        }
        Self::stop(&mut state, Some(id));
        Ok(state.calls[id].receipt.clone())
    }
    fn status(&self, id: &str) -> Result<HostToolInvocation> {
        self.state
            .lock()
            .unwrap()
            .calls
            .get(id)
            .map(|c| c.receipt.clone())
            .ok_or_else(|| Error::Invalid("unknown host tool invocation".into()))
    }
    async fn execute(&self, id: &str) -> Result<Value> {
        let (def, arguments, context) = {
            let mut state = self.state.lock().unwrap();
            if state.revoked || state.blocked {
                return Err(Error::Rejected(
                    "host tool context is no longer executable".into(),
                ));
            }
            let turn = state.turn.clone();
            let call = state
                .calls
                .get_mut(id)
                .ok_or_else(|| Error::Invalid("unknown host tool invocation".into()))?;
            if call.receipt.state != HostToolState::Prepared
                || turn.as_deref() != Some(&call.receipt.turn_id)
            {
                return Err(Error::Rejected(
                    "host tool invocation is no longer prepared for this turn".into(),
                ));
            }
            call.executing = true;
            call.receipt.state = HostToolState::Running;
            (
                call.definition.clone(),
                call.arguments.clone(),
                HostToolContext {
                    invocation: call.receipt.clone(),
                    working_context: self.context.clone(),
                    cancellation: call.cancellation.clone(),
                },
            )
        };
        let result = tokio::select! {
            biased;
            _ = context.cancellation.cancelled() => Err(Error::Cancelled("host tool invocation cancelled".into())),
            _ = tokio::time::sleep(std::time::Duration::from_secs(120)) => {
                context.cancellation.cancel();
                Err(Error::Timeout("host tool execution".into()))
            },
            value = async {
                crate::verify_context(&context.working_context).await?;
                context.check_cancelled()?;
                let value = def.extension.execute_tool(&def.tool.id, arguments, &context).await?;
                if serde_json::to_vec(&value)?.len() > 1024*1024 || !def.output.is_valid(&value) {
                    return Err(Error::Invalid("host tool result does not match its bounded schema".into()));
                }
                Ok(value)
            } => value,
        };
        let mut state = self.state.lock().unwrap();
        let call = state.calls.get_mut(id).expect("active calls are retained");
        call.executing = false;
        if matches!(&result, Err(Error::Timeout(_))) {
            call.receipt.state = HostToolState::Failed;
            return result;
        }
        if call.cancellation.is_cancelled() {
            call.receipt.state = HostToolState::Cancelled;
            return Err(Error::Cancelled(
                "host tool invocation cancelled; late result discarded".into(),
            ));
        }
        call.receipt.state = if result.is_ok() {
            HostToolState::Completed
        } else {
            HostToolState::Failed
        };
        let value = result?;
        Ok(
            json!({"invocation":call.receipt,"origin":{"extensionId":def.grant.extension_id,"toolId":def.tool.id},"result":value}),
        )
    }
}
#[derive(Default)]
pub(crate) struct HostTools {
    channels: Mutex<BTreeMap<String, Weak<HostToolScope>>>,
    transport: Mutex<Option<McpServer>>,
}
impl HostTools {
    pub(crate) fn transport(&self, server: McpServer) -> Result<()> {
        if server.name != "swarmie_host" || !matches!(server.transport, McpTransport::Stdio { .. })
        {
            return Err(Error::Invalid(
                "host tool delivery requires the reserved swarmie_host stdio server".into(),
            ));
        }
        *self.transport.lock().unwrap() = Some(server);
        Ok(())
    }
    pub(crate) fn launch(
        &self,
        options: &mut SpawnOptions,
        context: &WorkingContext,
        extensions: &[Arc<dyn RuntimeExtension>],
    ) -> Result<Option<Arc<HostToolScope>>> {
        if options.execution.host_tools.is_empty() {
            return Ok(None);
        }
        if options.execution.tools == ToolPolicy::Disabled {
            return Err(Error::Invalid(
                "disabled tools cannot have host tool grants".into(),
            ));
        }
        let mut server = self
            .transport
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| Error::Unsupported("host tool transport is not installed".into()))?;
        if options
            .execution
            .mcp_servers
            .iter()
            .any(|s| s.name == server.name)
        {
            return Err(Error::Invalid(
                "swarmie_host is reserved for host tool delivery".into(),
            ));
        }
        let mut definitions = BTreeMap::new();
        for grant in &options.execution.host_tools {
            let extension = extensions
                .iter()
                .find(|e| e.id() == grant.extension_id)
                .ok_or_else(|| Error::Invalid("unknown host tool extension".into()))?;
            let tool = extension
                .tools()
                .into_iter()
                .find(|t| t.id == grant.tool_id)
                .ok_or_else(|| {
                    Error::Unsupported("extension does not expose this host tool".into())
                })?;
            if ![extension.id(), &tool.id]
                .iter()
                .all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
            {
                return Err(Error::Invalid(
                    "host tool identifiers must use letters, digits or underscores".into(),
                ));
            }
            if tool.input_schema["type"] != "object"
                || tool.input_schema["additionalProperties"] != false
            {
                return Err(Error::Invalid(
                    "host tool input schema must reject undeclared arguments".into(),
                ));
            }
            if grant.argument_constraints.iter().any(|(key, values)| {
                values.is_empty() || tool.input_schema["properties"].get(key).is_none()
            }) {
                return Err(Error::Invalid(
                    "argument constraints require declared parameters and allowed values".into(),
                ));
            }
            let input = jsonschema::validator_for(&tool.input_schema)
                .map_err(|_| Error::Invalid("invalid host tool input schema".into()))?;
            let output = jsonschema::validator_for(&tool.result_schema)
                .map_err(|_| Error::Invalid("invalid host tool result schema".into()))?;
            let name = format!("{}__{}", extension.id(), tool.id);
            if definitions
                .insert(
                    name,
                    Arc::new(Definition {
                        grant: grant.clone(),
                        tool,
                        extension: extension.clone(),
                        input,
                        output,
                    }),
                )
                .is_some()
            {
                return Err(Error::Invalid("duplicate host tool grant".into()));
            }
        }
        let scope = Arc::new(HostToolScope {
            context: context.clone(),
            definitions,
            state: Mutex::new(ScopeState {
                native: None,
                turn: None,
                pending: false,
                blocked: false,
                revoked: false,
                calls: BTreeMap::new(),
            }),
        });
        let token = uuid::Uuid::new_v4().to_string();
        let McpTransport::Stdio { env, .. } = &mut server.transport else {
            unreachable!()
        };
        env.insert("SWARMIE_TOOL_CHANNEL".into(), token.clone());
        let mut channels = self.channels.lock().unwrap();
        channels.retain(|_, scope| scope.strong_count() > 0);
        channels.insert(token, Arc::downgrade(&scope));
        options.execution.mcp_servers.push(server);
        Ok(Some(scope))
    }
    pub(crate) fn channel(&self, token: &str) -> bool {
        self.scope(token).is_ok()
    }
    fn scope(&self, token: &str) -> Result<Arc<HostToolScope>> {
        self.channels
            .lock()
            .unwrap()
            .get(token)
            .and_then(Weak::upgrade)
            .filter(|s| !s.state.lock().unwrap().revoked)
            .ok_or_else(|| Error::Rejected("unknown or expired host tool channel".into()))
    }
    pub(crate) async fn dispatch(&self, token: &str, method: &str, params: Value) -> Result<Value> {
        let scope = self.scope(token)?;
        let required = |key: &str| {
            params
                .get(key)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| Error::Invalid(format!("missing {key}")))
        };
        match method {
            "tools/list" => scope.catalog(),
            "tools/prepare" => Ok(json!(
                scope.prepare(
                    required("name")?,
                    params
                        .get("arguments")
                        .cloned()
                        .unwrap_or_else(|| json!({}))
                )?
            )),
            "tools/execute" => scope.execute(required("invocationId")?).await,
            "tools/cancel" => Ok(json!(scope.cancel(required("invocationId")?)?)),
            "tools/status" => Ok(json!(scope.status(required("invocationId")?)?)),
            _ => Err(Error::Unsupported(
                "method is not available on a host tool channel".into(),
            )),
        }
    }
}
