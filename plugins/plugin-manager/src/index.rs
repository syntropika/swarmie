use crate::PluginId;
use std::{
    any::Any,
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};
use swarmie_storage::ChangeSource;

type Contributions = BTreeMap<PluginId, Vec<Rc<dyn Any>>>;

/// Observed native contribution metadata. Tembra remains the service registry.
#[derive(Clone, Default)]
pub struct PluginIndex {
    entries: Rc<RefCell<Contributions>>,
    changes: ChangeSource,
    active: Rc<RefCell<BTreeSet<PluginId>>>,
}

impl PluginIndex {
    /// Returns installed contribution metadata, including inactive plugins.
    pub fn contributions<T: Clone + 'static>(&self) -> Vec<T> {
        self.entries
            .borrow()
            .values()
            .flatten()
            .filter_map(|value| value.downcast_ref::<T>().cloned())
            .collect()
    }

    /// Contributions eligible for new work. Disabled or mismatched native plugins
    /// may remain physically mounted until restart, but are absent from this view.
    pub fn active_contributions<T: Clone + 'static>(&self) -> Vec<T> {
        let active = self.active.borrow();
        self.entries
            .borrow()
            .iter()
            .filter(|(id, _)| active.contains(*id))
            .flat_map(|(_, entries)| entries)
            .filter_map(|value| value.downcast_ref::<T>().cloned())
            .collect()
    }

    /// Reports local changes to installed or active plugin membership.
    pub fn changes(&self) -> ChangeSource {
        self.changes.clone()
    }

    pub(crate) fn publish(
        &self,
        entries: BTreeMap<PluginId, Vec<Rc<dyn Any>>>,
        active: BTreeSet<PluginId>,
    ) {
        let changed =
            self.entries.borrow().keys().ne(entries.keys()) || *self.active.borrow() != active;
        *self.entries.borrow_mut() = entries;
        *self.active.borrow_mut() = active;
        if changed {
            self.changes.invalidate();
        }
    }
}

/// Composition-only access to the existing host and observed contribution index.
#[derive(Clone)]
pub struct PluginContext {
    pub host: tembra::Host,
    pub index: PluginIndex,
}
