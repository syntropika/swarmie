//! Typed extension contracts. Tembra owns contributions, plans and physical delivery.
use crate::{AgentContext, FinishReason, ModelRequest, ToolCall, ToolDecision, ToolResult, Usage};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, rc::Rc};
use tembra::{Context, Declaration, Hook, HookHandle, Orb};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookName {
    BeforeTurnAgent,
    BeforeTurn,
    PrepareContext,
    BeforeToolCall,
    AfterToolCall,
    AfterTurn,
    AfterTurnAgent,
}

impl HookName {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BeforeTurnAgent => "before_turn_agent",
            Self::BeforeTurn => "before_turn",
            Self::PrepareContext => "prepare_context",
            Self::BeforeToolCall => "before_tool_call",
            Self::AfterToolCall => "after_tool_call",
            Self::AfterTurn => "after_turn",
            Self::AfterTurnAgent => "after_turn_agent",
        }
    }
}

/// A request-local projection. Hooks cannot change the turn or widen admitted tools.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelInput {
    pub turn: u32,
    pub request: ModelRequest,
}

/// Arguments may change before intent confirmation; identity and name stay fixed.
/// A Block is terminal for the contribution chain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolInput {
    pub turn: u32,
    pub call: ToolCall,
    pub decision: ToolDecision,
}

/// Pure result transformations run before confirmation, including after recovery.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolOutput {
    pub turn: u32,
    pub call: ToolCall,
    pub result: ToolResult,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnOutcome {
    pub turn: u32,
    pub usage: Usage,
    pub reason: Option<FinishReason>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cause {
    #[serde(with = "code_wire")]
    pub code: tembra::Code,
    pub reason: String,
    pub message: String,
}

impl From<&tembra::Error> for Cause {
    fn from(error: &tembra::Error) -> Self {
        Self {
            code: error.code,
            reason: error.reason.clone(),
            message: error.message.clone(),
        }
    }
}

mod code_wire {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(code: &tembra::Code, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(code.as_str())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<tembra::Code, D::Error> {
        let value = String::deserialize(deserializer)?;
        tembra::Code::parse(&value).ok_or_else(|| serde::de::Error::custom("unknown error code"))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ExecutionStatus {
    Completed(FinishReason),
    Interrupted(Cause),
}

/// One physical attempt's outcome, not proof of task completion or effect rollback.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionOutcome {
    pub source: ExecutionSource,
    pub turn: u32,
    pub usage: Usage,
    pub status: ExecutionStatus,
}

/// Host-local correlation with the physical invocation and its original root.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionSource {
    pub invocation: tembra::core::Id,
    pub root: tembra::core::Id,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObservationFailure {
    pub hook: HookName,
    pub cause: Cause,
}

/// Share these descriptors with contributing orbs. Ready-made loop orbs declare
/// and bind them automatically; custom orbs use declare and Agent::bind_hooks.
/// Each contribution targets its declaring orb explicitly; ordinary, durable
/// and conversation-durable services do not share contributions implicitly.
/// BeforeToolCall is a preparation policy, not the final permission boundary:
/// pending recovery skips it. The Tools provider must enforce authorization.
///
/// ```
/// use swarmie_agent_loop::{SERVICE, ToolDecision, hooks::{Hooks, HookName}};
/// use tembra::{Orb, core::Requirement};
/// let hooks = Hooks::default();
/// let guard = Orb::new("tool-guard", 1)
///     .requires(Requirement {
///         orb: SERVICE.into(), api_version: 1, methods: Default::default(),
///         hooks: [HookName::BeforeToolCall.as_str().into()].into(),
///     })
///     .contribute(SERVICE, &hooks.before_tool_call, 0, |_, mut input| async move {
///         if input.call.arguments.get("requires_approval").is_some() {
///             input.decision = ToolDecision::Block("approval required".into());
///         }
///         Ok(input)
///     });
/// # let _ = guard;
/// ```
pub struct Hooks {
    pub before_turn_agent: Hook<ModelInput, ModelInput>,
    pub before_turn: Hook<ModelInput, ModelInput>,
    pub prepare_context: Hook<AgentContext, AgentContext>,
    pub before_tool_call: Hook<ToolInput, ToolInput>,
    pub after_tool_call: Hook<ToolOutput, ToolOutput>,
    pub after_turn: Hook<TurnOutcome, ()>,
    pub after_turn_agent: Hook<ExecutionOutcome, ()>,
}

impl Default for Hooks {
    fn default() -> Self {
        Self {
            before_turn_agent: Hook::waterfall(HookName::BeforeTurnAgent.as_str()),
            before_turn: Hook::waterfall(HookName::BeforeTurn.as_str()),
            prepare_context: Hook::waterfall(HookName::PrepareContext.as_str()),
            before_tool_call: Hook::waterfall_until(
                HookName::BeforeToolCall.as_str(),
                |input: &ToolInput| matches!(input.decision, ToolDecision::Block(_)),
            ),
            after_tool_call: Hook::waterfall(HookName::AfterToolCall.as_str()),
            after_turn: Hook::series(HookName::AfterTurn.as_str()),
            after_turn_agent: Hook::series(HookName::AfterTurnAgent.as_str()),
        }
    }
}

impl Hooks {
    pub fn declare(self, orb: Orb) -> Orb {
        orb.hook(self.before_turn_agent)
            .hook(self.before_turn)
            .hook(self.prepare_context)
            .hook(self.before_tool_call)
            .hook(self.after_tool_call)
            .hook(self.after_turn)
            .hook(self.after_turn_agent)
    }
}

pub(crate) struct Lifecycle {
    pub before_turn_agent: HookHandle<ModelInput, ModelInput>,
    pub before_turn: HookHandle<ModelInput, ModelInput>,
    pub prepare_context: HookHandle<AgentContext, AgentContext>,
    pub before_tool_call: HookHandle<ToolInput, ToolInput>,
    pub after_tool_call: HookHandle<ToolOutput, ToolOutput>,
    pub after_turn: HookHandle<TurnOutcome, ()>,
    pub after_turn_agent: HookHandle<ExecutionOutcome, ()>,
    last_failure: RefCell<Option<ObservationFailure>>,
}

impl Lifecycle {
    pub fn bind(declaration: &Declaration) -> tembra::Result<Rc<Self>> {
        let hooks = Hooks::default();
        Ok(Rc::new(Self {
            before_turn_agent: declaration.hook(&hooks.before_turn_agent)?,
            before_turn: declaration.hook(&hooks.before_turn)?,
            prepare_context: declaration.hook(&hooks.prepare_context)?,
            before_tool_call: declaration.hook(&hooks.before_tool_call)?,
            after_tool_call: declaration.hook(&hooks.after_tool_call)?,
            after_turn: declaration.hook(&hooks.after_turn)?,
            after_turn_agent: declaration.hook(&hooks.after_turn_agent)?,
            last_failure: RefCell::new(None),
        }))
    }

    pub fn last_failure(&self) -> Option<ObservationFailure> {
        self.last_failure.borrow().clone()
    }

    fn record(&self, hook: HookName, result: tembra::Result<()>) {
        if let Err(error) = result {
            self.last_failure.replace(Some(ObservationFailure {
                hook,
                cause: Cause::from(&error),
            }));
        }
    }

    pub async fn turn(&self, context: &Context, outcome: TurnOutcome) {
        self.record(
            HookName::AfterTurn,
            self.after_turn.run(context, outcome).await,
        );
    }

    pub async fn finished(&self, context: &Context, outcome: ExecutionOutcome) {
        self.record(
            HookName::AfterTurnAgent,
            self.after_turn_agent.observe(context, outcome).await,
        );
    }
}
