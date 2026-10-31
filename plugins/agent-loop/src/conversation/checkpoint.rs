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
    /// Immutable project sources. Restore binds placement to the admitted User.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sources: Option<swarmie_providers::ContextSources>,
    pub tools: Vec<ToolSpec>,
    pub max_turns: NonZeroU32,
    #[cfg(feature = "context-projection")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projection: Option<swarmie_context::Projection>,
}

impl Request {
    pub fn new(
        at: Head,
        instructions: String,
        tools: Vec<ToolSpec>,
        max_turns: NonZeroU32,
    ) -> Self {
        Self {
            at,
            instructions,
            sources: None,
            tools,
            max_turns,
            #[cfg(feature = "context-projection")]
            projection: None,
        }
    }
}

impl Request {
    pub fn with_sources(
        mut self,
        sources: swarmie_providers::ContextSources,
    ) -> tembra::Result<Self> {
        sources.validate()?;
        self.sources = Some(sources.at(0));
        Ok(self)
    }
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<Box<swarmie_providers::PreparedDescription>>,
    },
    PreparedTool {
        operation: OperationId,
        call: ToolCall,
        provider: Option<crate::ToolProvider>,
    },
    #[cfg(feature = "context-projection")]
    ContextResult {
        operation: OperationId,
        plan: swarmie_context::Plan,
        response: crate::ModelResponse,
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
    #[cfg(feature = "context-projection")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    projection: Option<swarmie_context::Projection>,
    #[cfg(feature = "context-projection")]
    #[serde(default)]
    context_progress: swarmie_context::Checkpoint,
    #[cfg(feature = "context-projection")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    context_fence: Option<swarmie_context::WorkFence>,
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
            #[cfg(feature = "context-projection")]
            projection: None,
            #[cfg(feature = "context-projection")]
            context_progress: swarmie_context::Checkpoint::default(),
            #[cfg(feature = "context-projection")]
            context_fence: None,
        }
    }

    /// Construct the initial checkpoint for an owner-validated immutable request.
    /// This value is not a persistence receipt or execution authority.
    pub fn from_request(input: Request) -> Self {
        Self::start(input)
    }

    #[cfg(feature = "context-projection")]
    pub fn context_progress(&self) -> &swarmie_context::Checkpoint {
        &self.context_progress
    }

    /// The original admission fence for the current compaction. Recovery must not
    /// replace it with a later conversation fence.
    #[cfg(feature = "context-projection")]
    pub fn context_fence(&self) -> Option<swarmie_context::WorkFence> {
        self.context_fence
    }

    /// Bind a newly admitted compaction while the turn is paused.
    #[cfg(feature = "context-projection")]
    pub fn with_context_admission(
        self,
        progress: swarmie_context::Checkpoint,
        fence: swarmie_context::WorkFence,
    ) -> tembra::Result<Self> {
        let mut checkpoint = self.with_context_progress(progress)?;
        checkpoint.context_fence = Some(fence);
        Ok(checkpoint)
    }

    /// Retain compaction recognition alongside a paused turn on the same task.
    /// Progress cannot be attached across a pending model/tool operation.
    #[cfg(feature = "context-projection")]
    pub fn with_context_progress(
        mut self,
        progress: swarmie_context::Checkpoint,
    ) -> tembra::Result<Self> {
        progress.validate()?;
        if self.phase != Phase::PrepareTurn {
            return Err(crate::model::invalid(
                "context progress requires a paused turn",
            ));
        }
        self.context_progress = progress;
        Ok(self)
    }

    /// Captured source selection for provider reconciliation. This grants no access.
    pub fn sources(&self) -> Option<&swarmie_providers::ContextSources> {
        self.input.sources.as_ref()
    }

    /// Return the last confirmed history selection.
    pub fn head(&self) -> Head {
        self.head
    }

    /// The selected projection is checkpoint data, never the current global active
    /// value. Only a safe PrepareTurn can explicitly adopt a recognized successor.
    #[cfg(feature = "context-projection")]
    pub(super) fn with_projection(
        mut self,
        projection: swarmie_context::Projection,
    ) -> tembra::Result<Self> {
        projection.validate()?;
        if self.phase != Phase::PrepareTurn
            || projection.plan.through.conversation != self.input.at.conversation
            || projection.plan.previous
                != self.projection().map(|selected| selected.artifact.entry())
        {
            return Err(crate::model::invalid(
                "checkpoint cannot adopt this context projection",
            ));
        }
        self.projection = Some(projection);
        Ok(self)
    }

    #[cfg(feature = "context-projection")]
    pub fn projection(&self) -> Option<&swarmie_context::Projection> {
        self.projection.as_ref().or(self.input.projection.as_ref())
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
