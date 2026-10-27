use crate::OperationId;
use serde::{Deserialize, Serialize};
use std::rc::Rc;
use tembra::{Context, Declaration};

/// A replaceable preview, never evidence of a settled model operation.
/// Tool arguments are deliberately absent: only the final response can authorize calls.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelProgress {
    pub operation: OperationId,
    pub content: String,
}

type Receiver = dyn Fn(&Context, ModelProgress);

/// Synchronous, best-effort observation. Receivers must return promptly; they
/// cannot fail the provider operation. No event queue or background task is owned here.
#[derive(Clone)]
pub struct Progress(Rc<Receiver>);

impl Progress {
    pub fn new(receive: impl Fn(&Context, ModelProgress) + 'static) -> Self {
        Self(Rc::new(receive))
    }

    /// Publish the latest preview through this object's ordinary Tembra State.
    /// Read it with `reference.state::<Option<ModelProgress>>()`. One latest value
    /// is retained per object, not a log. Concurrent operations carry distinct IDs;
    /// subscriptions see each update synchronously and can select an operation.
    /// The provider owns its output budget. Cancellation can suppress a publication.
    /// An object can declare only one State; custom compositions may use `new` instead.
    pub fn bind(declaration: &Declaration) -> tembra::Result<Self> {
        let state = declaration.state(None::<ModelProgress>, 1, |_| Ok(()))?;
        Ok(Self::new(move |context, value| {
            // Progress is transient. A closed/cancelled observer cannot replace
            // the provider's authoritative result or grant recovery authority.
            let _ = state.update(context, |_| Ok(Some(value)));
        }))
    }

    pub fn publish(&self, context: &Context, value: ModelProgress) {
        if context.check().is_ok() {
            (self.0)(context, value);
        }
    }
}
