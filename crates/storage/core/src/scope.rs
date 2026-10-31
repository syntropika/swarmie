use crate::StorageOwner;
use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::{Rc, Weak},
};
use tembra::{
    Code, Context, Error,
    core::{ContextKey, OpaqueContextValue},
};

const KEY: ContextKey<OpaqueContextValue> = ContextKey::new("swarmie.storage.write_scope", 1);

#[derive(Default)]
pub(super) struct Scopes {
    next: Cell<u64>,
    active: RefCell<BTreeMap<u64, Weak<Pending>>>,
}

struct Pending {
    token: OpaqueContextValue,
    root: u64,
    principal: String,
    changes: RefCell<Vec<Box<dyn Any>>>,
}

fn refused(code: Code, message: &str) -> Error {
    Error::new(code, "swarmie.storage.write_scope", message)
}

/// A logical preparation scope, never a live backend transaction.
/// Drop aborts and promptly releases payloads even if Tembra retains the derived frame.
/// Nested scopes are refused. Authors must await all participating managed calls.
pub struct WriteScope {
    owner: StorageOwner,
    pending: Rc<Pending>,
    context: Context,
}

/// A sealed, owner-bound batch. Only a backend that understands every change may apply it.
/// Backend adapters must validate ownership before opening their atomic transaction.
pub struct PreparedChanges {
    owner: StorageOwner,
    changes: Vec<Box<dyn Any>>,
}

impl PreparedChanges {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Consumes a batch for its issuing owner. No mutation occurs on refusal.
    pub fn into_changes(self, owner: &StorageOwner) -> tembra::Result<Vec<Box<dyn Any>>> {
        owner
            .check()
            .map_err(|e| refused(Code::Unavailable, &e.to_string()))?;
        if !self.owner.same(owner) {
            return Err(refused(
                Code::Forbidden,
                "batch belongs to another storage owner",
            ));
        }
        Ok(self.changes)
    }
}

impl WriteScope {
    pub fn context(&self) -> &Context {
        &self.context
    }

    /// Seals against late writes. Cancellation or owner uncertainty aborts preparation.
    pub fn seal(self) -> tembra::Result<PreparedChanges> {
        self.context.check()?;
        self.owner
            .check()
            .map_err(|e| refused(Code::Unavailable, &e.to_string()))?;
        self.owner
            .0
            .scopes
            .active
            .borrow_mut()
            .remove(&self.pending.token.id);
        Ok(PreparedChanges {
            owner: self.owner.clone(),
            changes: std::mem::take(&mut *self.pending.changes.borrow_mut()),
        })
    }
}

impl Drop for WriteScope {
    fn drop(&mut self) {
        self.owner
            .0
            .scopes
            .active
            .borrow_mut()
            .remove(&self.pending.token.id);
        self.pending.changes.borrow_mut().clear();
    }
}

impl StorageOwner {
    /// Derives a context carrying an opaque owner-issued scope reference.
    /// The frame retains only the reference, never the staged payloads. Native adapters
    /// must not transplant this reference into a different Tembra host.
    pub fn prepare(&self, context: &Context) -> tembra::Result<WriteScope> {
        context.check()?;
        self.check()
            .map_err(|e| refused(Code::Unavailable, &e.to_string()))?;
        if context.value(&KEY)?.is_some() {
            return Err(refused(
                Code::Conflict,
                "nested write scopes are unsupported",
            ));
        }
        let id = self
            .0
            .scopes
            .next
            .get()
            .checked_add(1)
            .ok_or_else(|| refused(Code::Exhausted, "write scope identity exhausted"))?;
        self.0.scopes.next.set(id);
        let token = OpaqueContextValue::new(id, ());
        let derived = context.with(&KEY, token.clone())?;
        let pending = Rc::new(Pending {
            token,
            root: context.root,
            principal: context.principal()?,
            changes: RefCell::new(vec![]),
        });
        self.0
            .scopes
            .active
            .borrow_mut()
            .insert(id, Rc::downgrade(&pending));
        Ok(WriteScope {
            owner: self.clone(),
            pending,
            context: derived,
        })
    }

    /// Stages a backend-owned command. Success acknowledges preparation only.
    /// A missing, foreign, closed or cancelled scope is refused; there is no implicit
    /// independent commit fallback. Backend commands must contain no external effects.
    pub fn stage<T: Any>(&self, context: &Context, change: T) -> tembra::Result<()> {
        if self.try_stage(context, change)?.is_some() {
            return Err(refused(Code::Invalid, "write scope required"));
        }
        Ok(())
    }

    /// Stages under an existing scope, or returns the command unchanged when no
    /// scope was supplied. Backend capabilities may then explicitly commit it.
    /// A present but foreign, expired or invalid scope always fails closed.
    pub fn try_stage<T: Any>(&self, context: &Context, change: T) -> tembra::Result<Option<T>> {
        context.check()?;
        self.check()
            .map_err(|e| refused(Code::Unavailable, &e.to_string()))?;
        let Some(token) = context.value(&KEY)? else {
            return Ok(Some(change));
        };
        let pending = self
            .0
            .scopes
            .active
            .borrow()
            .get(&token.id)
            .and_then(Weak::upgrade)
            .ok_or_else(|| refused(Code::Forbidden, "write scope is foreign or closed"))?;
        if pending.token != token
            || pending.root != context.root
            || pending.principal != context.principal()?
        {
            return Err(refused(Code::Forbidden, "write scope identity mismatch"));
        }
        pending.changes.borrow_mut().push(Box::new(change));
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{FutureExt, executor::LocalPool};

    struct Payload(Rc<Cell<u32>>);
    impl Drop for Payload {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    #[test]
    fn sealing_releases_payloads_and_refuses_late_foreign_and_nested_scopes() {
        let mut pool = LocalPool::new();
        let host = tembra::Host::new(pool.spawner());
        let drops = Rc::new(Cell::new(0));
        let observed = drops.clone();
        pool.run_until(host.root("alice", move |ctx| {
            async move {
                let owner = StorageOwner::default();
                let foreign = StorageOwner::default();
                let scope = owner.prepare(&ctx)?;
                let derived = scope.context().clone();
                owner.stage(&derived, Payload(drops.clone()))?;
                assert!(owner.prepare(&derived).is_err());
                // Both owners issue local identity 1; token identity still distinguishes them.
                let other = foreign.prepare(&ctx)?;
                assert!(foreign.stage(&derived, 1_u64).is_err());
                assert!(owner.stage(other.context(), 1_u64).is_err());
                let batch = scope.seal()?;
                assert!(owner.stage(&derived, 1_u64).is_err());
                assert_eq!(drops.get(), 0);
                drop(batch.into_changes(&owner)?);
                assert_eq!(drops.get(), 1, "release before physical root ends");
                let aborted = owner.prepare(&ctx)?;
                owner.stage(aborted.context(), Payload(drops.clone()))?;
                drop(aborted);
                assert_eq!(drops.get(), 2);
                let scope = owner.prepare(&ctx)?;
                owner.stage(scope.context(), Payload(drops.clone()))?;
                assert!(scope.seal()?.into_changes(&foreign).is_err());
                assert_eq!(drops.get(), 3);
                assert!(owner.0.scopes.active.borrow().is_empty());
                Ok(())
            }
            .boxed_local()
        }))
        .unwrap();
        assert_eq!(observed.get(), 3);
        pool.run_until(host.close()).unwrap();
    }

    #[test]
    fn cancellation_and_uncertainty_discard_without_sealing() {
        let mut pool = LocalPool::new();
        let host = tembra::Host::new(pool.spawner());
        pool.run_until(host.root("alice", move |ctx| {
            async move {
                let owner = StorageOwner::default();
                let scope = owner.prepare(&ctx)?;
                let derived = scope.context().clone();
                owner.require_recovery();
                assert!(owner.stage(&derived, 1_u64).is_err());
                assert!(scope.seal().is_err());
                let owner = StorageOwner::default();
                let scope = owner.prepare(&ctx)?;
                ctx.cancel()?;
                assert!(owner.stage(scope.context(), 1_u64).is_err());
                assert!(scope.seal().is_err());
                Ok(())
            }
            .boxed_local()
        }))
        .unwrap();
        pool.run_until(host.close()).unwrap();
    }

    #[test]
    fn root_owned_preparations_remain_subject_to_the_live_frame_limit() {
        let mut pool = LocalPool::new();
        let host = tembra::Host::new(pool.spawner());
        let kernel = host.kernel();
        let inspected = kernel.clone();
        pool.run_until(host.root("alice", move |ctx| {
            async move {
                let owner = StorageOwner::default();
                let drops = Rc::new(Cell::new(0));
                for _ in 0..4096 {
                    let scope = owner.prepare(&ctx)?;
                    owner.stage(scope.context(), Payload(drops.clone()))?;
                    drop(scope.seal()?.into_changes(&owner)?);
                }
                assert_eq!(drops.get(), 4096);
                assert!(owner.0.scopes.active.borrow().is_empty());
                assert_eq!(inspected.context_stats().0, 4096);
                let error = match owner.prepare(&ctx) {
                    Ok(_) => panic!("root-owned frame limit must be visible"),
                    Err(error) => error,
                };
                assert_eq!(error.code, Code::Exhausted);
                assert!(owner.check().is_ok());
                Ok(())
            }
            .boxed_local()
        }))
        .unwrap();
        assert_eq!(kernel.context_stats().0, 0);
        pool.run_until(host.close()).unwrap();
    }
}
