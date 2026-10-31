//! Native execution ownership supplied by the embedding, independent of Desktop or vault custody.
use std::{any::Any, rc::Rc};
use swarmie_model_provider_ai::ModelProtection;
use swarmie_tools::ToolDefinition;
use tembra::Context;
use uuid::Uuid;

/// Creates one retained logical owner from the authenticated conversation address.
/// Serialized draft fields do not supply project, agent or secret authority.
pub trait ExecutionOwner {
    /// The embedding authenticates the principal before installation and checks it here again.
    fn conversation(
        self: Rc<Self>,
        principal: &str,
        conversation: Uuid,
    ) -> tembra::Result<Rc<dyn ConversationExecution>>;

    /// Reference-only definitions. Their Tembra providers must be installed on the same host.
    fn tools(&self) -> tembra::Result<Vec<ToolDefinition>>;
}

/// One logical main-agent conversation. A child must receive a separate owner.
pub trait ConversationExecution {
    /// Bind a physical turn while retaining logical approvals across turns.
    fn enter(&self, context: &Context) -> tembra::Result<ExecutionScope>;
}

/// Opaque physical ownership retained until all model and tool calls settle.
pub struct ExecutionScope {
    context: Context,
    protection: ModelProtection,
    _lifetime: Box<dyn Any>,
}

impl ExecutionScope {
    /// The native owner supplies its bound context, mandatory guard and nonserializable lifetime.
    pub fn new<T: Any>(context: Context, protection: ModelProtection, lifetime: T) -> Self {
        Self {
            context,
            protection,
            _lifetime: Box::new(lifetime),
        }
    }

    /// Context for the actual engine and all descendant model/tool calls.
    pub fn context(&self) -> &Context {
        &self.context
    }

    /// Guard configuration belonging to the same native execution.
    pub fn protection(&self) -> ModelProtection {
        self.protection.clone()
    }
}
