use crate::PluginId;
use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};
use swarmie_storage_core::ChangeSource;

type Contributions = BTreeMap<PluginId, Vec<Rc<dyn Any>>>;

/// Observed native contribution metadata. Tembra remains the service registry.
#[derive(Clone)]
pub struct PluginIndex {
    entries: Rc<RefCell<Contributions>>,
    changes: ChangeSource,
    active: Rc<RefCell<BTreeSet<PluginId>>>,
    required: Rc<BTreeSet<PluginId>>,
    eligible: Rc<Cell<bool>>,
}

impl Default for PluginIndex {
    fn default() -> Self {
        Self {
            entries: Default::default(),
            changes: Default::default(),
            active: Default::default(),
            required: Default::default(),
            eligible: Rc::new(Cell::new(true)),
        }
    }
}

impl PluginIndex {
    pub(crate) fn with_required(required: BTreeSet<PluginId>) -> Self {
        Self {
            eligible: Rc::new(Cell::new(required.is_empty())),
            required: Rc::new(required),
            ..Self::default()
        }
    }

    /// Cached eligibility of every mandatory plugin, without inventory I/O.
    /// Mounted declarations do not prove readiness of an individual service object.
    pub fn required_active(&self) -> bool {
        self.eligible.get()
    }

    pub(crate) fn invalidate(&self) {
        let entries = self.entries.borrow().clone();
        self.publish(entries, BTreeSet::new());
    }

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
        let eligible = self.required.is_subset(&active);
        let changed = self.entries.borrow().keys().ne(entries.keys())
            || *self.active.borrow() != active
            || self.eligible.get() != eligible;
        *self.entries.borrow_mut() = entries;
        *self.active.borrow_mut() = active;
        self.eligible.set(eligible);
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
