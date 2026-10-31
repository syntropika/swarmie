use crate::{Authority, ExecutionBinding};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::{Rc, Weak},
};
use tembra::{
    Code, Context, Error,
    core::{ContextKey, OpaqueContextValue},
};

const KEY: ContextKey<OpaqueContextValue> = ContextKey::new("swarmie.execution.owner", 1);

struct Bound {
    token: OpaqueContextValue,
    binding: ExecutionBinding,
    root: u64,
    principal: String,
}

struct State {
    authority: Authority,
    next: Cell<u64>,
    bindings: RefCell<BTreeMap<u64, Weak<Bound>>>,
}

/// Private mapping supplied by the actual logical execution owner, not a principal-name heuristic.
/// Issuers must replace the binding for children and recovered/adopted executions explicitly.
#[derive(Clone)]
pub struct ExecutionContexts(Rc<State>);

/// Retained by the execution owner until its physical consumers settle. Drop fences future use.
pub struct BoundExecutionContext {
    owner: ExecutionContexts,
    bound: Rc<Bound>,
    context: Context,
}

impl BoundExecutionContext {
    pub fn context(&self) -> &Context {
        &self.context
    }
}

impl Drop for BoundExecutionContext {
    fn drop(&mut self) {
        self.bound.binding.close();
        self.owner
            .0
            .bindings
            .borrow_mut()
            .remove(&self.bound.token.id);
    }
}

impl ExecutionContexts {
    pub fn new(authority: Authority) -> Self {
        Self(Rc::new(State {
            authority,
            next: Cell::new(0),
            bindings: RefCell::new(BTreeMap::new()),
        }))
    }

    /// Native embedding entry. No serialized caller can mint or transplant its opaque token.
    pub fn bind(
        &self,
        context: &Context,
        binding: ExecutionBinding,
    ) -> tembra::Result<BoundExecutionContext> {
        context.check()?;
        if !binding.is_live() {
            return Err(denied());
        }
        let binding = binding.scoped();
        self.0
            .authority
            .check_binding(binding.shell())
            .map_err(|_| denied())?;
        let id = self.0.next.get().checked_add(1).ok_or_else(denied)?;
        self.0.next.set(id);
        let token = OpaqueContextValue::new(id, ());
        let derived = context.with(&KEY, token.clone())?;
        let bound = Rc::new(Bound {
            token,
            binding,
            root: context.root,
            principal: context.principal()?,
        });
        self.0
            .bindings
            .borrow_mut()
            .insert(id, Rc::downgrade(&bound));
        Ok(BoundExecutionContext {
            owner: self.clone(),
            bound,
            context: derived,
        })
    }

    pub fn resolve(&self, context: &Context) -> tembra::Result<ExecutionBinding> {
        context.check()?;
        let token = context.value(&KEY)?.ok_or_else(denied)?;
        let bound = self
            .0
            .bindings
            .borrow()
            .get(&token.id)
            .and_then(Weak::upgrade)
            .ok_or_else(denied)?;
        if token != bound.token
            || context.root != bound.root
            || context.principal()? != bound.principal
            || !bound.binding.is_live()
        {
            return Err(denied());
        }
        Ok(bound.binding.clone())
    }
}

fn denied() -> Error {
    Error::new(
        Code::Forbidden,
        "swarmie.execution.owner",
        "authenticated execution ownership is unavailable",
    )
}
