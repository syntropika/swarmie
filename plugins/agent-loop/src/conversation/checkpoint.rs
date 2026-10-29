use crate::{Message, ModelRequest, OperationId, Output, ToolCall, ToolName, ToolSpec, Usage};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU32;
use swarmie_conversations::{EntryId, Head};

/// A fixed initial history selection and model-neutral execution configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub at: Head,
    pub instructions: String,
    pub tools: Vec<ToolSpec>,
    pub max_turns: NonZeroU32,
}

/// Typed agent records inside a generic conversation. Prepared requests are evidence,
/// never model-visible messages and never replacements for the canonical transcript.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Record {
    Message(Message),
    PreparedRequest {
        operation: OperationId,
        request: ModelRequest,
    },
    PreparedTool {
        operation: OperationId,
        call: ToolCall,
        provider: Option<crate::ToolProvider>,
    },
}

/// Serializable execution progress. Payloads live once in immutable history records.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    pub(crate) input: Request,
    pub(crate) head: Head,
    pub(crate) turn: u32,
    pub(crate) usage: Usage,
    pub(crate) available_tools: Vec<ToolName>,
    pub(crate) phase: Phase,
}

impl Checkpoint {
    pub(super) fn start(input: Request) -> Self {
        Self {
            available_tools: input.tools.iter().map(|tool| tool.name.clone()).collect(),
            head: input.at,
            input,
            turn: 0,
            usage: Usage::default(),
            phase: Phase::PrepareTurn,
        }
    }

    /// Return the last confirmed history selection.
    pub fn head(&self) -> Head {
        self.head
    }

    /// Return whether confirmed assistant calls still need terminal tool results.
    pub fn has_unfinished_tools(&self) -> bool {
        matches!(
            self.phase,
            Phase::PrepareTools { .. } | Phase::ToolPending { .. }
        )
    }

    /// Return the next confirmed operation without exposing stored request payloads.
    /// A pending operation can require recovery even when dispatch is not confirmed.
    pub fn pending(&self) -> Option<Pending> {
        match self.phase {
            Phase::PrepareTurn => Some(Pending::Turn),
            Phase::ModelPending { operation, .. } => Some(Pending::Model { operation }),
            Phase::PrepareTools { next, .. } => Some(Pending::Tools { next }),
            Phase::ToolPending {
                operation, next, ..
            } => Some(Pending::Tool { operation, next }),
            Phase::Finished { .. } => None,
        }
    }
}

/// Read-only execution progress. Positions refer to the last assistant's tool calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pending {
    Turn,
    Model { operation: OperationId },
    Tools { next: usize },
    Tool { operation: OperationId, next: usize },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Phase {
    PrepareTurn,
    ModelPending {
        operation: OperationId,
        request: EntryId,
    },
    PrepareTools {
        assistant: EntryId,
        next: usize,
    },
    ToolPending {
        operation: OperationId,
        prepared: Option<EntryId>,
        assistant: EntryId,
        next: usize,
    },
    Finished {
        reason: crate::FinishReason,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Completion {
    pub output: Output,
    pub checkpoint: Checkpoint,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionFailure {
    #[serde(with = "crate::failure::error_wire")]
    pub error: tembra::Error,
    pub checkpoint: Option<Box<Checkpoint>>,
}

impl std::fmt::Display for ExecutionFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}

impl std::error::Error for ExecutionFailure {}

pub type ExecutionResult = Result<Completion, ExecutionFailure>;
