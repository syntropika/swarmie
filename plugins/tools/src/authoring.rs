use serde::de::DeserializeOwned;
use serde_json::Value;
use std::future::Future;
use tembra::Context;

use crate::{
    IntoToolOutput, OperationId, Recovery, ToolCall, ToolContext, ToolDecision, ToolExecutionError,
    ToolName, ToolResult, ToolSpec,
};

/// Typed authoring contract. Tembra supplies execution, cancellation and identity.
/// Argument decoding and admission happen before `call`; implementations must
/// enforce permission again when executing and recovering external effects.
pub trait Tool: Sized + 'static {
    const NAME: &'static str;
    type Args: DeserializeOwned;
    type Output: IntoToolOutput;
    type Error: std::error::Error + Send + Sync + 'static;

    fn description(&self) -> String;
    fn parameters(&self) -> Value;

    /// Descriptive hints. They grant no permission or replay authority.
    fn annotations(&self) -> crate::ToolAnnotations {
        crate::ToolAnnotations::default()
    }

    fn footprint(&self, args: &Self::Args) -> crate::Touches {
        let _ = args;
        crate::Touches::from_annotations(&self.annotations())
    }

    /// Domain permission check. Metadata and JSON schema are never authority.
    fn validate(
        &self,
        context: &Context,
        args: &Self::Args,
    ) -> impl Future<Output = tembra::Result<ToolDecision>> {
        let _ = args;
        async { context.check().map(|()| ToolDecision::Allow) }
    }

    fn call(
        &self,
        context: &mut ToolContext,
        args: Self::Args,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>>;

    fn map_error(&self, error: Self::Error) -> ToolExecutionError {
        ToolExecutionError::from_error(error)
    }

    /// Reconcile the exact saved operation. This must not repeat the effect.
    /// Only a confirmed provider verdict can authorize a later retry.
    fn recover(
        &self,
        context: &Context,
        operation: OperationId,
        call: &ToolCall,
    ) -> impl Future<Output = tembra::Result<Recovery<ToolResult>>> {
        let _ = (operation, call);
        async { context.check().map(|()| Recovery::Unknown) }
    }
}

pub fn tool_spec<T: Tool>(tool: &T) -> tembra::Result<ToolSpec> {
    Ok(ToolSpec {
        name: ToolName::new(T::NAME)?,
        description: tool.description(),
        input_schema: tool.parameters(),
    })
}
