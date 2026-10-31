use futures::future::LocalBoxFuture;
use std::rc::Rc;
use tembra::Context;

use crate::{ToolCall, ToolDecision, ToolSpec};

type Admission = dyn for<'a> Fn(
    &'a Context,
    &'a ToolSpec,
    &'a ToolCall,
) -> LocalBoxFuture<'a, tembra::Result<ToolDecision>>;

/// Host admission of the exact final call. This runs again immediately before
/// execution; catalog membership and author annotations cannot substitute for it.
#[derive(Clone)]
pub struct ToolPolicy(Rc<Admission>);

impl ToolPolicy {
    pub fn new(
        check: impl for<'a> Fn(
            &'a Context,
            &'a ToolSpec,
            &'a ToolCall,
        ) -> LocalBoxFuture<'a, tembra::Result<ToolDecision>>
        + 'static,
    ) -> Self {
        Self(Rc::new(check))
    }

    pub(crate) async fn check(
        &self,
        context: &Context,
        spec: &ToolSpec,
        call: &ToolCall,
    ) -> tembra::Result<ToolDecision> {
        (self.0)(context, spec, call).await
    }
}
