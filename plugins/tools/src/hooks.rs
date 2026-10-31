//! Typed Tembra extension points for provider admission and physical outcomes.
//! Admission hooks are pure and may only narrow a decision. Argument rewriting
//! belongs before the caller confirms intent. Observers cannot change outcomes,
//! authorize replay or claim exactly-once delivery.

use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use tembra::{Context, Declaration, Hook, HookHandle, Orb};

use crate::{
    OperationId, Recovery, ToolCall, ToolDecision, ToolExecution, ToolResult, ToolResultContext,
    ToolSpec, invalid,
};

pub const FAILURE_METHOD: &str = "hook_failure";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Preparation,
    Execution,
    Recovery,
}

/// Immutable invocation data and a decision that contributions may restrict.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Admission {
    pub phase: Phase,
    pub operation: Option<OperationId>,
    pub spec: ToolSpec,
    pub call: ToolCall,
    pub decision: ToolDecision,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Failure {
    pub code: String,
    pub reason: String,
    pub message: String,
}

impl From<&tembra::Error> for Failure {
    fn from(error: &tembra::Error) -> Self {
        Self {
            code: error.code.as_str().into(),
            reason: error.reason.clone(),
            message: error.message.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Outcome {
    Executed(ToolResult),
    Recovered(Recovery<ToolResult>),
    Unconfirmed(Failure),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub root: tembra::core::Id,
    pub invocation: tembra::core::Id,
    pub cancelled: bool,
}

/// Outcome from the still-owned physical invocation, with explicitly published
/// metadata. Inbound tool context and runtime authority are never included.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finished {
    pub source: Source,
    pub phase: Phase,
    pub operation: OperationId,
    pub call: ToolCall,
    pub outcome: Outcome,
    pub metadata: ToolResultContext,
}

/// Descriptors shared with contributing Orbs. `tool_orb` declares and binds these
/// automatically. Custom provider Orbs use `declare` and `ToolHandler::bind_hooks`.
/// Plans, ordering, versioning and physical delivery belong entirely to Tembra.
pub struct Hooks {
    pub before_call: Hook<Admission, Admission>,
    pub after_call: Hook<Finished, ()>,
}

impl Default for Hooks {
    fn default() -> Self {
        Self {
            before_call: Hook::waterfall_until("before_call", |input: &Admission| {
                matches!(input.decision, ToolDecision::Block(_))
            }),
            after_call: Hook::series("after_call"),
        }
    }
}

impl Hooks {
    pub fn declare(self, orb: Orb) -> Orb {
        orb.hook(self.before_call).hook(self.after_call)
    }
}

pub(crate) struct Bound {
    before_call: HookHandle<Admission, Admission>,
    after_call: HookHandle<Finished, ()>,
    last_failure: RefCell<Option<Failure>>,
}

impl Bound {
    pub fn bind(declaration: &Declaration) -> tembra::Result<Self> {
        let hooks = Hooks::default();
        Ok(Self {
            before_call: declaration.hook(&hooks.before_call)?,
            after_call: declaration.hook(&hooks.after_call)?,
            last_failure: RefCell::new(None),
        })
    }

    pub fn failure(&self) -> Option<Failure> {
        self.last_failure.borrow().clone()
    }

    pub async fn admit(&self, context: &Context, input: Admission) -> tembra::Result<ToolDecision> {
        let output = self.before_call.run(context, input.clone()).await?;
        if output.phase != input.phase
            || output.operation != input.operation
            || output.spec != input.spec
            || output.call != input.call
        {
            return Err(invalid(
                "admission hook changed confirmed tool identity, arguments or metadata",
            ));
        }
        Ok(output.decision)
    }

    pub async fn finished(
        &self,
        context: &Context,
        execution: ToolExecution<'_>,
        phase: Phase,
        outcome: Outcome,
        metadata: ToolResultContext,
    ) {
        let result = async {
            let invocation = context.invocation()?;
            self.after_call
                .observe(
                    context,
                    Finished {
                        source: Source {
                            root: context.root,
                            invocation: invocation.id,
                            cancelled: context.signal().is_cancelled(),
                        },
                        phase,
                        operation: execution.operation,
                        call: execution.call.clone(),
                        outcome,
                        metadata,
                    },
                )
                .await
        }
        .await;
        if let Err(error) = result {
            self.last_failure.replace(Some(Failure::from(&error)));
        }
    }
}
