//! Reference-only agent tools. Human input and decisions use a different, private shell port.
use crate::{executors::*, *};
use futures::{FutureExt, future::LocalBoxFuture};
use serde::{Deserialize, Serialize};
use std::rc::Rc;
use swarmie_permissions::ExecutionContexts;
use swarmie_tools::{
    OperationId, ToolCall, ToolDecision, ToolExecution, ToolResult, ToolSpec, Tools,
};
use tembra::{Context, Orb, Service};
use uuid::Uuid;

pub use crate::tools_schema::definitions;

pub const SERVICE: &str = "swarmie.vault.tools";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputIntent {
    pub operation: Uuid,
    pub execution: ExecutionIdentity,
    pub reference: SecretReference,
    pub expected: Option<Uuid>,
    pub metadata: SecretMetadata,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputReceipt {
    pub mutation: Uuid,
    pub outcome: CommitOutcome,
    pub reference: Option<SecretReference>,
}
pub trait PrivateInputs {
    fn request<'a>(
        &'a self,
        execution: ExecutionBinding,
        intent: InputIntent,
    ) -> LocalBoxFuture<'a, Result<InputReceipt>>;
    fn capture<'a>(
        &'a self,
        handle: Rc<CaptureHandle>,
        source: CaptureSource,
    ) -> LocalBoxFuture<'a, Result<CapturedSecretReceipt>>;
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InputRequest {
    secret: Option<Uuid>,
    expected: Option<Uuid>,
    metadata: SecretMetadata,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CaptureRequest {
    secret: Option<Uuid>,
    expected: Option<Uuid>,
    metadata: SecretMetadata,
    source: CaptureSource,
}

/// Frozen native owner for one account/root/application session. New roots require new bindings.
pub struct VaultTools {
    pub node: VaultNode,
    pub session: VaultSession,
    pub plugin: PluginGrant,
    pub executions: ExecutionContexts,
    pub inputs: Rc<dyn PrivateInputs>,
    pub decisions: Rc<dyn UseDecisions>,
    pub bash: Rc<BashExecutor>,
    pub http: Rc<HttpExecutor>,
}
fn denied(_: impl Sized) -> tembra::Error {
    tembra::Error::new(
        tembra::Code::Forbidden,
        "swarmie.vault.tools",
        "Secret operation was refused or remains unconfirmed",
    )
}
impl VaultTools {
    fn owner(&self, context: &Context) -> tembra::Result<ExecutionBinding> {
        let binding = self.executions.resolve(context)?;
        if self.plugin.provider() != SERVICE || !self.session.owner().same_owner(binding.shell()) {
            return Err(denied(()));
        }
        Ok(binding)
    }
    fn validate_call(&self, context: &Context, call: &ToolCall) -> tembra::Result<()> {
        self.owner(context)?;
        match call.name.as_str() {
            "vault_list" => {
                if !call
                    .arguments
                    .as_object()
                    .is_some_and(serde_json::Map::is_empty)
                {
                    return Err(denied(()));
                }
            }
            "vault_request_input" => {
                let request: InputRequest =
                    serde_json::from_value(call.arguments.clone()).map_err(denied)?;
                request.metadata.validate().map_err(denied)?;
            }
            "vault_capture" => {
                let request: CaptureRequest =
                    serde_json::from_value(call.arguments.clone()).map_err(denied)?;
                request.metadata.validate().map_err(denied)?;
                request.source.profile().map_err(denied)?;
            }
            "vault_bash" => {
                let _: BashRequest =
                    serde_json::from_value(call.arguments.clone()).map_err(denied)?;
            }
            "vault_http" => {
                let _: HttpRequest =
                    serde_json::from_value(call.arguments.clone()).map_err(denied)?;
            }
            _ => return Err(denied(())),
        }
        Ok(())
    }
    async fn run(
        &self,
        context: &Context,
        binding: &ExecutionBinding,
        operation: OperationId,
        call: &ToolCall,
    ) -> tembra::Result<serde_json::Value> {
        self.validate_call(context, call)?;
        let attempt = Uuid::new_v4();
        match call.name.as_str() {
            "vault_list" => Ok(serde_json::to_value(
                self.node
                    .discover(&self.session, binding, &self.plugin)
                    .map_err(denied)?,
            )
            .map_err(denied)?),
            "vault_request_input" => {
                let request: InputRequest =
                    serde_json::from_value(call.arguments.clone()).map_err(denied)?;
                self.plugin
                    .check(SERVICE, Permission::RequestUse)
                    .map_err(denied)?;
                let reference = SecretReference {
                    account: self.node.identity().account,
                    vault: self.node.identity().vault,
                    secret: request.secret.unwrap_or(operation.as_uuid()),
                };
                self.node
                    .check_agent_mutation(
                        &self.session,
                        binding,
                        reference,
                        request.expected,
                        &request.metadata,
                    )
                    .map_err(denied)?;
                let receipt = self
                    .inputs
                    .request(
                        binding.clone(),
                        InputIntent {
                            operation: operation.as_uuid(),
                            execution: binding.identity().clone(),
                            reference,
                            expected: request.expected,
                            metadata: request.metadata,
                        },
                    )
                    .await
                    .map_err(denied)?;
                serde_json::to_value(receipt).map_err(denied)
            }
            "vault_capture" => {
                let request: CaptureRequest =
                    serde_json::from_value(call.arguments.clone()).map_err(denied)?;
                let intent = CaptureIntent {
                    identity: self.node.identity(),
                    originating_operation: operation.as_uuid(),
                    execution: binding.identity().clone(),
                    provider: SERVICE.into(),
                    producer_generation: self.plugin.generation(),
                    destination: request.source.destination().map_err(denied)?,
                    profile: request.source.profile().map_err(denied)?,
                    secret: request.secret.unwrap_or(operation.as_uuid()),
                    expected: request.expected,
                    metadata: request.metadata,
                    attempt,
                    settlement_ms: request.source.timeout_ms(),
                };
                let handle = self
                    .node
                    .prepare_capture(&self.session, binding, &self.plugin, intent)
                    .await
                    .map_err(denied)?;
                serde_json::to_value(
                    self.inputs
                        .capture(Rc::new(handle), request.source)
                        .await
                        .map_err(denied)?,
                )
                .map_err(denied)
            }
            name => {
                let attempt = ExecutionAttempt {
                    node: &self.node,
                    session: &self.session,
                    execution: binding,
                    plugin: &self.plugin,
                    decisions: self.decisions.as_ref(),
                    operation: operation.as_uuid(),
                    attempt,
                };
                if name == "vault_bash" {
                    serde_json::to_value(
                        self.bash
                            .execute(
                                &attempt,
                                serde_json::from_value(call.arguments.clone()).map_err(denied)?,
                            )
                            .await
                            .map_err(denied)?,
                    )
                    .map_err(denied)
                } else {
                    serde_json::to_value(
                        self.http
                            .execute(
                                &attempt,
                                serde_json::from_value(call.arguments.clone()).map_err(denied)?,
                            )
                            .await
                            .map_err(denied)?,
                    )
                    .map_err(denied)
                }
            }
        }
    }
}
impl Tools for VaultTools {
    fn validate<'a>(
        &'a self,
        context: &'a Context,
        spec: &'a ToolSpec,
        call: &'a ToolCall,
    ) -> LocalBoxFuture<'a, tembra::Result<ToolDecision>> {
        async move {
            context.check()?;
            if spec.name != call.name || self.validate_call(context, call).is_err() {
                return Ok(ToolDecision::Block(
                    "The secret request is invalid or its authority is unavailable.".into(),
                ));
            }
            Ok(ToolDecision::Allow)
        }
        .boxed_local()
    }
    fn execute<'a>(
        &'a self,
        context: &'a Context,
        execution: ToolExecution<'a>,
    ) -> LocalBoxFuture<'a, tembra::Result<ToolResult>> {
        async move {
            context.check()?;
            if execution
                .provider
                .is_some_and(|provider| provider.service != SERVICE || provider.key != ["main"])
            {
                return Ok(ToolResult::Blocked(
                    "The secret provider does not match this execution.".into(),
                ));
            }
            let binding = match self.owner(context) {
                Ok(owner) => owner.scoped(),
                Err(_) => {
                    return Ok(ToolResult::Blocked(
                        "The secret owner is unavailable.".into(),
                    ));
                }
            };
            let _lifetime = CallLifetime(binding.clone());
            let signal = context.signal();
            let result = self
                .run(context, &binding, execution.operation, execution.call)
                .fuse();
            let cancelled = signal.cancelled().fuse();
            futures::pin_mut!(result, cancelled);
            futures::select_biased! {
                _ = cancelled => { binding.close(); context.check()?; Err(denied(())) },
                result = result => result.map(ToolResult::Success),
            }
        }
        .boxed_local()
    }
}
/// Ordinary Tembra service. The embedding supplies the actual execution context and private ports.
pub fn orb(tools: Rc<VaultTools>) -> Orb {
    orb_with_owner(move |_| Ok(tools.clone()))
}

/// Resolve the native tool owner from its opaque physical execution context.
/// The resolver must authenticate ownership; serialized tool arguments grant no authority.
pub fn orb_with_owner(
    resolve: impl Fn(&Context) -> tembra::Result<Rc<VaultTools>> + 'static,
) -> Orb {
    let resolve = Rc::new(resolve);
    Orb::new(SERVICE, 1).service(
        Service::new(SERVICE, move |_| Ok(resolve.clone()))
            .method(
                "validate",
                |resolve, context, (spec, call): (ToolSpec, ToolCall)| async move {
                    context.check()?;
                    match resolve(&context) {
                        Ok(tools) => tools.validate(&context, &spec, &call).await,
                        Err(_) => Ok(ToolDecision::Block(
                            "The vault is unavailable for this execution.".into(),
                        )),
                    }
                },
            )
            .method(
                "execute",
                |resolve, context, (operation, call): (OperationId, ToolCall)| async move {
                    context.check()?;
                    match resolve(&context) {
                        Ok(tools) => {
                            tools
                                .execute(&context, ToolExecution::new(operation, &call))
                                .await
                        }
                        Err(_) => Ok(ToolResult::Blocked(
                            "The vault is unavailable for this execution.".into(),
                        )),
                    }
                },
            )
            .method("recover", |_, _, _: (OperationId, ToolCall)| async {
                Ok(swarmie_tools::Recovery::<ToolResult>::Unknown)
            }),
    )
}

struct CallLifetime(ExecutionBinding);
impl Drop for CallLifetime {
    fn drop(&mut self) {
        self.0.close();
    }
}
