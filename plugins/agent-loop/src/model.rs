use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, VecDeque},
    num::NonZeroU32,
};
use tembra::{Code, Error};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureReason {
    InvalidInput,
    RecoveryRequired,
}

impl FailureReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidInput => "swarmie.agent_loop.input",
            Self::RecoveryRequired => "swarmie.agent_loop.recovery_required",
        }
    }
}

pub(crate) fn invalid(message: impl ToString) -> Error {
    Error::new(
        Code::Invalid,
        FailureReason::InvalidInput.as_str(),
        message.to_string(),
    )
}

pub use swarmie_providers::{
    AgentContext, AssistantMessage, Message, ModelRequest, ModelResponse, OperationId, ToolCall,
    ToolCallId, ToolName, ToolResult, ToolSpec, Usage,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub context: AgentContext,
    pub tools: Vec<ToolSpec>,
    pub max_turns: NonZeroU32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Complete,
    TurnLimit,
}

/// Pending phases contain the effective request or final tool arguments.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Phase {
    PrepareTurn,
    ModelPending {
        operation: OperationId,
        request: ModelRequest,
    },
    PrepareTools {
        calls: Vec<ToolCall>,
        next: usize,
    },
    ToolPending {
        operation: OperationId,
        call: ToolCall,
        provider: Option<crate::ToolProvider>,
        calls: Vec<ToolCall>,
        next: usize,
    },
    Finished {
        reason: FinishReason,
    },
}

/// Confirmed progress with inline messages and no live provider or storage handles.
/// Conversation checkpoints store history references; restored State still materializes messages.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub(crate) request: Request,
    pub(crate) initial_messages: usize,
    pub(crate) turn: u32,
    pub(crate) usage: Usage,
    pub(crate) available_tools: Vec<ToolName>,
    pub(crate) phase: Phase,
}

impl State {
    pub fn start(request: Request) -> tembra::Result<Self> {
        let state = Self {
            initial_messages: request.context.messages.len(),
            available_tools: request.tools.iter().map(|tool| tool.name.clone()).collect(),
            request,
            turn: 0,
            usage: Usage::default(),
            phase: Phase::PrepareTurn,
        };
        state.validate()?;
        Ok(state)
    }

    pub fn phase(&self) -> &Phase {
        &self.phase
    }

    pub fn context(&self) -> &AgentContext {
        &self.request.context
    }

    /// Messages added by this execution, suitable for a caller-owned history append.
    pub fn messages(&self) -> &[Message] {
        self.request
            .context
            .messages
            .get(self.initial_messages..)
            .unwrap_or_default()
    }

    #[cfg(feature = "durable-tasks")]
    pub(crate) fn matches_request(&self, request: &Request) -> bool {
        self.initial_messages == request.context.messages.len()
            && self.request.context.instructions == request.context.instructions
            && self.request.context.messages.get(..self.initial_messages)
                == Some(request.context.messages.as_slice())
            && self.request.tools == request.tools
            && self.request.max_turns == request.max_turns
    }

    pub(crate) fn validate(&self) -> tembra::Result<()> {
        if self.initial_messages > self.request.context.messages.len()
            || self.turn > self.request.max_turns.get()
        {
            return Err(invalid("invalid progress bounds"));
        }
        let mut names = BTreeSet::new();
        for tool in &self.request.tools {
            if !names.insert(&tool.name) {
                return Err(invalid("duplicate tool definition"));
            }
        }
        let mut available = BTreeSet::new();
        if self
            .available_tools
            .iter()
            .any(|name| !names.contains(name) || !available.insert(name))
        {
            return Err(invalid("invalid prepared tool availability"));
        }
        let mut pending = VecDeque::new();
        let mut ids = BTreeSet::new();
        for (index, message) in self.request.context.messages.iter().enumerate() {
            if index == self.initial_messages && !pending.is_empty() {
                return Err(invalid("initial history has unfinished tool calls"));
            }
            match message {
                Message::Assistant(message) => {
                    if !pending.is_empty() {
                        return Err(invalid("assistant precedes pending tool results"));
                    }
                    for call in &message.tool_calls {
                        if !ids.insert(&call.id) {
                            return Err(invalid("duplicate tool call identity"));
                        }
                        pending.push_back(&call.id);
                    }
                }
                Message::Tool { call, .. } => {
                    if pending.pop_front() != Some(call) {
                        return Err(invalid("tool result does not match a pending call"));
                    }
                }
                Message::User { .. } if !pending.is_empty() => {
                    return Err(invalid("user message precedes pending tool results"));
                }
                _ => {}
            }
        }
        if self.initial_messages == self.request.context.messages.len() && !pending.is_empty() {
            return Err(invalid("initial history has unfinished tool calls"));
        }
        if !matches!(
            self.phase,
            Phase::PrepareTools { .. } | Phase::ToolPending { .. }
        ) && !pending.is_empty()
        {
            return Err(invalid("unfinished tools outside a tool phase"));
        }
        if let Phase::ToolPending {
            provider: Some(provider),
            ..
        } = &self.phase
        {
            provider.validate()?;
        }
        match &self.phase {
            Phase::ToolPending {
                call, calls, next, ..
            } if calls
                .get(*next)
                .is_none_or(|original| original.id != call.id || original.name != call.name) =>
            {
                return Err(invalid("prepared tool changed its call identity"));
            }
            Phase::ModelPending { request, .. } => {
                if self.turn == 0
                    || !request
                        .tools
                        .iter()
                        .map(|tool| &tool.name)
                        .eq(self.available_tools.iter())
                    || request
                        .tools
                        .iter()
                        .any(|tool| !self.request.tools.contains(tool))
                {
                    return Err(invalid(
                        "pending model request widens the allowed tool definitions",
                    ));
                }
                // A prepared request may project context without rewriting history.
                // Validate its own message pairing and tool uniqueness before recovery.
                if request.context != self.request.context || request.tools != self.request.tools {
                    Self::start(Request {
                        context: request.context.clone(),
                        tools: request.tools.clone(),
                        max_turns: self.request.max_turns,
                    })?;
                }
            }
            Phase::PrepareTools { calls, next } | Phase::ToolPending { calls, next, .. } => {
                if self.turn == 0 || *next >= calls.len() {
                    return Err(invalid("invalid pending tool position"));
                }
                let expected = self
                    .request
                    .context
                    .messages
                    .iter()
                    .rev()
                    .find_map(|message| {
                        if let Message::Assistant(message) = message {
                            Some(&message.tool_calls)
                        } else {
                            None
                        }
                    });
                if expected != Some(calls)
                    || !pending
                        .iter()
                        .copied()
                        .eq(calls[*next..].iter().map(|call| &call.id))
                {
                    return Err(invalid("pending tools do not match the assistant message"));
                }
            }
            Phase::PrepareTurn if self.turn >= self.request.max_turns.get() => {
                return Err(invalid("turn limit exceeded"));
            }
            _ => {}
        }
        Ok(())
    }

    pub(crate) fn output(&self, reason: FinishReason) -> Output {
        Output {
            messages: self.messages().to_vec(),
            usage: self.usage,
            reason,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Output {
    pub messages: Vec<Message>,
    pub usage: Usage,
    pub reason: FinishReason,
}
