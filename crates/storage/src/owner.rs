use crate::{ChangeSource, scope};
use std::{cell::Cell, rc::Rc};

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum OwnerError {
    #[error("storage owner requires authoritative recovery")]
    RecoveryRequired,
    #[error("storage owner is closed")]
    Closed,
}

#[derive(Default)]
pub(super) struct State {
    uncertain: Cell<bool>,
    closed: Cell<bool>,
    changes: ChangeSource,
    pub(super) scopes: scope::Scopes,
}

/// Identity and uncertainty gate shared by all capabilities of one storage owner.
#[derive(Clone, Default)]
pub struct StorageOwner(pub(super) Rc<State>);

impl StorageOwner {
    /// Checks whether both capabilities share the same owner instance.
    pub fn same(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    /// Refuses use after closure or an unknown commit outcome.
    pub fn check(&self) -> Result<(), OwnerError> {
        if self.0.closed.get() {
            Err(OwnerError::Closed)
        } else if self.0.uncertain.get() {
            Err(OwnerError::RecoveryRequired)
        } else {
            Ok(())
        }
    }

    /// Freezes all domain capabilities after an unknown acknowledgement.
    pub fn require_recovery(&self) {
        if !self.0.uncertain.replace(true) {
            self.0.changes.invalidate();
        }
    }

    /// Backends call this only after every capability lease and physical write settles.
    pub fn closed(&self) {
        if !self.0.closed.replace(true) {
            self.0.changes.invalidate();
        }
    }

    /// Returns local notifications for changes to owner health.
    pub fn changes(&self) -> ChangeSource {
        self.0.changes.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uncertainty_is_shared_sticky_and_emits_one_invalidation() {
        let owner = StorageOwner::default();
        let other_capability = owner.clone();
        let count = Rc::new(Cell::new(0));
        let captured = count.clone();
        let _subscription = owner
            .changes()
            .subscribe(move || captured.set(captured.get() + 1));
        other_capability.require_recovery();
        owner.require_recovery();
        assert_eq!(count.get(), 1);
        assert!(owner.same(&other_capability));
        assert!(!owner.same(&StorageOwner::default()));
        assert_eq!(owner.check(), Err(OwnerError::RecoveryRequired));
        owner.closed();
        assert_eq!(other_capability.check(), Err(OwnerError::Closed));
        assert_eq!(count.get(), 2);
    }
}
