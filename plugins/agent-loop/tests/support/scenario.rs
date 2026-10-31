use futures::{FutureExt, future::LocalBoxFuture};
use serde_json::json;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
};
use swarmie_agent_loop::*;
use tembra::{Code, Context, Error};

#[derive(Default)]
pub struct Scenario {
    pub models: RefCell<Vec<OperationId>>,
    pub effects: RefCell<Vec<OperationId>>,
    pub validations: RefCell<Vec<ToolCall>>,
    pub tool_inputs: RefCell<Vec<ToolCall>>,
    pub recovered_inputs: RefCell<Vec<ToolCall>>,
    pub reconciled_models: RefCell<Vec<OperationId>>,
    pub reconciled_tools: RefCell<Vec<OperationId>>,
    pub uncertain_model: Cell<bool>,
    pub cancel_model: Cell<bool>,
    pub safe_retry: Cell<bool>,
    pub not_started: Cell<bool>,
    pub tool_gate: RefCell<Option<futures::channel::oneshot::Receiver<()>>>,
    pub tool_entered: Cell<bool>,
    pub uncertain_tool: Cell<bool>,
    pub tool_count: Cell<u32>,
    pub uncertain_at: Cell<usize>,
    pub unknown_tool: Cell<bool>,
    pub unavailable_recovery: Cell<bool>,
    pub block: Cell<bool>,
    pub known_failure: Cell<bool>,
    model_receipts: RefCell<BTreeMap<String, ModelResponse>>,
    tool_receipts: RefCell<BTreeMap<String, ToolResult>>,
}

impl Model for Scenario {
    fn complete<'a>(
        &'a self,
        ctx: &'a Context,
        operation: OperationId,
        request: &'a ModelRequest,
    ) -> LocalBoxFuture<'a, tembra::Result<ModelResponse>> {
        async move {
            self.models.borrow_mut().push(operation);
            if self.cancel_model.get() {
                ctx.cancel()?;
            }
            let tool_calls = if request
                .context
                .messages
                .iter()
                .any(|message| matches!(message, Message::Tool { .. }))
            {
                vec![]
            } else {
                (1..=self.tool_count.get().max(1))
                    .map(|index| ToolCall {
                        id: ToolCallId::new(format!("call-{index}")).unwrap(),
                        name: ToolName::new("counter.add").unwrap(),
                        arguments: json!({"amount":1}),
                    })
                    .collect()
            };
            let response = ModelResponse {
                protection: swarmie_providers::SubmissionProtection::Unprotected,
                message: AssistantMessage {
                    provider: None,
                    content: if tool_calls.is_empty() {
                        "Done".into()
                    } else {
                        String::new()
                    },
                    tool_calls,
                },
                usage: Usage {
                    input_tokens: 2,
                    output_tokens: 3,
                },
            };
            self.model_receipts
                .borrow_mut()
                .insert(operation.to_string(), response.clone());
            if self.uncertain_model.replace(false) {
                return Err(Error::new(
                    Code::Unavailable,
                    "fixture.disconnected",
                    "model reply lost",
                ));
            }
            Ok(response)
        }
        .boxed_local()
    }

    fn recover<'a>(
        &'a self,
        _: &'a Context,
        operation: OperationId,
        _: &'a ModelRequest,
    ) -> LocalBoxFuture<'a, tembra::Result<Recovery<ModelResponse>>> {
        async move {
            self.reconciled_models.borrow_mut().push(operation);
            Ok(self
                .model_receipts
                .borrow()
                .get(&operation.to_string())
                .cloned()
                .map(Recovery::Completed)
                .unwrap_or(Recovery::Unknown))
        }
        .boxed_local()
    }
}

impl Tools for Scenario {
    fn validate<'a>(
        &'a self,
        _: &'a Context,
        _: &'a ToolSpec,
        call: &'a ToolCall,
    ) -> LocalBoxFuture<'a, tembra::Result<ToolDecision>> {
        async move {
            self.validations.borrow_mut().push(call.clone());
            Ok(
                if self.block.get() || call.arguments != json!({"amount":1}) {
                    ToolDecision::Block("invalid or unauthorized arguments".into())
                } else {
                    ToolDecision::Allow
                },
            )
        }
        .boxed_local()
    }

    fn execute<'a>(
        &'a self,
        _: &'a Context,
        execution: ToolExecution<'a>,
    ) -> LocalBoxFuture<'a, tembra::Result<ToolResult>> {
        async move {
            let ToolExecution {
                operation, call, ..
            } = execution;
            self.tool_inputs.borrow_mut().push(call.clone());
            self.tool_entered.set(true);
            let gate = self.tool_gate.borrow_mut().take();
            if let Some(gate) = gate {
                let _ = gate.await;
            }
            if self.not_started.replace(false) {
                return Err(Error::new(
                    Code::Unavailable,
                    "fixture.not_started",
                    "no external operation was dispatched",
                ));
            }
            self.effects.borrow_mut().push(operation);
            let result = if self.known_failure.get() {
                ToolResult::Failed("known failure".into())
            } else {
                ToolResult::Success(json!(1))
            };
            self.tool_receipts
                .borrow_mut()
                .insert(operation.to_string(), result.clone());
            if self.uncertain_tool.replace(false)
                || self.uncertain_at.get() == self.effects.borrow().len()
            {
                return Err(Error::new(
                    Code::Unavailable,
                    "fixture.disconnected",
                    "tool reply lost",
                ));
            }
            Ok(result)
        }
        .boxed_local()
    }

    fn recover<'a>(
        &'a self,
        _: &'a Context,
        execution: ToolExecution<'a>,
    ) -> LocalBoxFuture<'a, tembra::Result<Recovery<ToolResult>>> {
        async move {
            let ToolExecution {
                operation, call, ..
            } = execution;
            self.recovered_inputs.borrow_mut().push(call.clone());
            self.reconciled_tools.borrow_mut().push(operation);
            if self.unavailable_recovery.get() {
                return Err(Error::new(
                    Code::Unavailable,
                    "fixture.recovery_unavailable",
                    "tool provider is unavailable",
                ));
            }
            Ok(if self.safe_retry.replace(false) {
                Recovery::SafeToRetry
            } else if self.unknown_tool.get() {
                Recovery::Unknown
            } else {
                self.tool_receipts
                    .borrow()
                    .get(&operation.to_string())
                    .cloned()
                    .map(Recovery::Completed)
                    .unwrap_or(Recovery::Unknown)
            })
        }
        .boxed_local()
    }
}
