//! Local wake-ups for callers awaiting terminal records. A notification carries no task state:
//! every woken waiter rereads TaskStorage before it reports an outcome.

use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use crate::scheduler::Wake;

/// Registered waiters by task identity. Entries live only as long as their [`Waiter`] guard.
#[derive(Default)]
pub(crate) struct Waiters {
    entries: RefCell<BTreeMap<String, Vec<Rc<Wake>>>>,
}

impl Waiters {
    /// Registers interest before the caller's first read, so a commit that lands during that
    /// read still wakes the caller afterwards.
    pub fn register(&self, id: &str) -> Waiter<'_> {
        let wake = Rc::new(Wake::default());
        self.entries
            .borrow_mut()
            .entry(id.to_owned())
            .or_default()
            .push(wake.clone());
        Waiter {
            waiters: self,
            id: id.to_owned(),
            wake,
        }
    }

    /// Wakes every caller awaiting this task identity.
    pub fn notify(&self, id: &str) {
        let woken = self.entries.borrow().get(id).cloned().unwrap_or_default();
        for wake in woken {
            wake.notify();
        }
    }

    /// Wakes every registered caller, for commits that this engine did not perform itself.
    pub fn notify_all(&self) {
        let woken: Vec<_> = self.entries.borrow().values().flatten().cloned().collect();
        for wake in woken {
            wake.notify();
        }
    }
}

/// One caller's registration; dropping it removes the entry, including on cancellation.
pub(crate) struct Waiter<'a> {
    waiters: &'a Waiters,
    id: String,
    wake: Rc<Wake>,
}

impl Waiter<'_> {
    /// Completes once after a notification that arrived since the previous completion.
    pub fn notified(&self) -> impl Future<Output = ()> + '_ {
        self.wake.wait()
    }
}

impl Drop for Waiter<'_> {
    fn drop(&mut self) {
        let mut entries = self.waiters.entries.borrow_mut();
        if let Some(registered) = entries.get_mut(&self.id) {
            registered.retain(|wake| !Rc::ptr_eq(wake, &self.wake));
            if registered.is_empty() {
                entries.remove(&self.id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::task::{Context, Poll};

    use super::*;

    fn ready(future: impl Future<Output = ()>) -> bool {
        let mut future = std::pin::pin!(future);
        let mut context = Context::from_waker(std::task::Waker::noop());
        future.as_mut().poll(&mut context) == Poll::Ready(())
    }

    #[test]
    fn notifications_reach_only_registered_identities_and_guards_unregister() {
        let waiters = Waiters::default();
        let first = waiters.register("first");
        let second = waiters.register("second");
        assert!(!ready(first.notified()));
        waiters.notify("first");
        assert!(ready(first.notified()));
        assert!(
            !ready(first.notified()),
            "a notification completes one wait"
        );
        assert!(!ready(second.notified()));
        waiters.notify_all();
        assert!(ready(first.notified()));
        assert!(ready(second.notified()));
        drop(first);
        drop(second);
        assert!(waiters.entries.borrow().is_empty());
    }

    #[test]
    fn a_notification_before_the_wait_is_retained() {
        let waiters = Waiters::default();
        let waiter = waiters.register("task");
        let other = waiters.register("task");
        waiters.notify("task");
        assert!(ready(waiter.notified()));
        assert!(ready(other.notified()));
        drop(waiter);
        assert_eq!(waiters.entries.borrow()["task"].len(), 1);
    }
}
