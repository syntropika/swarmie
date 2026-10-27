use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::{Rc, Weak},
};

#[derive(Default)]
struct Listeners {
    next: Cell<u64>,
    listeners: RefCell<BTreeMap<u64, Rc<dyn Fn()>>>,
}

/// Coalescible local invalidations, not commit receipts or a persisted event log.
#[derive(Clone, Default)]
pub struct ChangeSource(Rc<Listeners>);

impl ChangeSource {
    /// Attaches synchronously. Drop the subscription to detach before owner cleanup.
    pub fn subscribe(&self, listener: impl Fn() + 'static) -> Subscription {
        let id = self.0.next.get();
        self.0
            .next
            .set(id.checked_add(1).expect("subscription identity exhausted"));
        self.0.listeners.borrow_mut().insert(id, Rc::new(listener));
        Subscription {
            owner: Rc::downgrade(&self.0),
            id,
        }
    }

    /// Notify after transaction and mutation gates have been released.
    pub fn invalidate(&self) {
        let listeners: Vec<_> = self.0.listeners.borrow().values().cloned().collect();
        for listener in listeners {
            listener();
        }
    }
}

/// Detaches its listener when dropped.
pub struct Subscription {
    owner: Weak<Listeners>,
    id: u64,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(owner) = self.owner.upgrade() {
            owner.listeners.borrow_mut().remove(&self.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscriptions_detach_and_callbacks_can_reenter_without_a_borrow() {
        let source = ChangeSource::default();
        let count = Rc::new(Cell::new(0));
        let captured = count.clone();
        let nested = source.clone();
        let subscription = source.subscribe(move || {
            captured.set(captured.get() + 1);
            drop(nested.subscribe(|| {}));
        });
        source.invalidate();
        assert_eq!(count.get(), 1);
        drop(subscription);
        source.invalidate();
        assert_eq!(count.get(), 1);
    }
}
