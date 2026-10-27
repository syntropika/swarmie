use crate::{Configuration, Observation, invalid, mounted::Mounted};
use futures::FutureExt;
use std::collections::BTreeMap;
use swarmie_tools::{
    Catalog, OperationId, Recovery, ToolCall, ToolDecision, ToolExecution, ToolMethod, ToolResult,
    ToolSpec, Tools,
};
use tembra::{Orb, Service};

pub const SERVICE: &str = "swarmie.mcp";

/// Each principal receives a private catalog projection selected by the host.
/// Use Tools' `orb_with_catalog` to expose that same projection to consumers.
#[derive(Clone)]
pub struct ServerBinding {
    pub configuration: Configuration,
    pub principal: String,
    pub catalog: Catalog,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum McpMethod {
    Connect,
    Describe,
    Disable,
    Result,
}

impl McpMethod {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Connect => "connect",
            Self::Describe => "describe",
            Self::Disable => "disable",
            Self::Result => "result",
        }
    }
}

/// Mount configured providers without connecting. Connect is an explicit method;
/// Describe, Result and Recover never open transports or publish catalog entries.
/// Tembra owns the resource and physically settles its admitted calls before close.
pub fn orb(bindings: Vec<ServerBinding>) -> tembra::Result<Orb> {
    let mut configured = BTreeMap::new();
    for binding in bindings {
        binding.configuration.validate()?;
        if binding.principal.is_empty() {
            return Err(invalid("MCP binding requires its owning principal"));
        }
        if configured
            .insert(
                (
                    binding.principal.clone(),
                    binding.configuration.id.as_str().to_owned(),
                ),
                binding,
            )
            .is_some()
        {
            return Err(invalid("duplicate MCP server identity within a principal"));
        }
    }
    let service = Service::new(SERVICE, move |declaration| {
        let address = declaration.address()?;
        let [id] = address.key.as_slice() else {
            return Err(invalid("MCP service key requires one server identity"));
        };
        let binding = configured
            .get(&(address.principal, id.clone()))
            .ok_or_else(|| invalid("MCP server is not bound to this principal and key"))?
            .clone();
        declaration.resource(
            "mcp.connection",
            1,
            move || Ok(Mounted::new(binding)),
            |mounted, _| async move { mounted.close().await }.boxed_local(),
        )
    });
    Ok(Orb::new(SERVICE, 1).service(connection_methods(service)))
}

pub(crate) fn connection_methods(
    service: Service<tembra::Resource<Mounted>>,
) -> Service<tembra::Resource<Mounted>> {
    service
        .method(
            McpMethod::Connect.as_str(),
            |env, context, (): ()| async move { env.value()?.connect(&context).await },
        )
        .method(
            McpMethod::Describe.as_str(),
            |env, context, (): ()| async move {
                context.check()?;
                Ok::<Observation, tembra::Error>(env.value()?.observation())
            },
        )
        .method(
            McpMethod::Disable.as_str(),
            |env, context, (): ()| async move {
                context.check()?;
                env.value()?.disable();
                Ok(())
            },
        )
        .method(
            McpMethod::Result.as_str(),
            |env, context, (operation, call): (OperationId, ToolCall)| async move {
                context.check()?;
                match env.value()?.connection() {
                    Some(connection) => connection.result(&context, operation, &call),
                    None => Ok(None),
                }
            },
        )
        .method(
            ToolMethod::Validate.as_str(),
            |env, context, (spec, call): (ToolSpec, ToolCall)| async move {
                context.check()?;
                match env.value()?.connection() {
                    Some(connection) => connection.validate(&context, &spec, &call).await,
                    None => Ok(ToolDecision::Block("MCP is not connected".into())),
                }
            },
        )
        .method(
            ToolMethod::Execute.as_str(),
            |env, context, (operation, call): (OperationId, ToolCall)| async move {
                context.check()?;
                let mounted = env.value()?;
                match mounted.connection() {
                    Some(connection) => {
                        let result = connection
                            .execute(&context, ToolExecution::new(operation, &call))
                            .await;
                        mounted.withdraw_unavailable();
                        result
                    }
                    None => Ok(ToolResult::Blocked("MCP is not connected".into())),
                }
            },
        )
        .method(
            ToolMethod::Recover.as_str(),
            |env, context, (operation, call): (OperationId, ToolCall)| async move {
                context.check()?;
                match env.value()?.connection() {
                    Some(connection) => {
                        connection
                            .recover(&context, ToolExecution::new(operation, &call))
                            .await
                    }
                    None => Ok(Recovery::<ToolResult>::Unknown),
                }
            },
        )
}
