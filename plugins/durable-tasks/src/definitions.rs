use crate::{
    TaskDefinition,
    authority::{Reason, error},
};
use std::{cell::RefCell, collections::BTreeSet, rc::Rc};
use swarmie_storage::{ChangeSource, Subscription};
use tembra::Code;

type Reader = Rc<dyn Fn() -> Vec<TaskDefinition>>;

/// Live typed contribution index. It contains metadata; Tembra resolves actual services.
#[derive(Clone)]
pub struct Definitions {
    entries: Rc<RefCell<tembra::Result<BTreeSet<TaskDefinition>>>>,
    changes: ChangeSource,
    source: Option<(Reader, ChangeSource)>,
}

impl Default for Definitions {
    fn default() -> Self {
        Self::new([]).expect("empty definitions")
    }
}

impl Definitions {
    /// Rejects invalid and duplicate contributions before adopting a candidate index.
    ///
    /// # Errors
    /// Returns Invalid for invalid names and Conflict for duplicate definitions.
    pub fn new(definitions: impl IntoIterator<Item = TaskDefinition>) -> tembra::Result<Self> {
        Ok(Self {
            entries: Rc::new(RefCell::new(Ok(validate(definitions)?))),
            changes: ChangeSource::default(),
            source: None,
        })
    }

    /// Atomically replaces standalone contribution metadata and requests reconciliation.
    pub fn replace(
        &self,
        definitions: impl IntoIterator<Item = TaskDefinition>,
    ) -> tembra::Result<()> {
        let next = validate(definitions)?;
        *self.entries.borrow_mut() = Ok(next);
        self.changes.invalidate();
        Ok(())
    }

    pub(crate) fn contains(&self, definition: &TaskDefinition) -> tembra::Result<bool> {
        self.entries
            .borrow()
            .as_ref()
            .map(|v| v.contains(definition))
            .map_err(Clone::clone)
    }
    pub(crate) fn changes(&self) -> ChangeSource {
        self.changes.clone()
    }

    pub(crate) fn attach(&self) -> Option<Subscription> {
        self.source.as_ref().map(|(reader, source)| {
            let entries = Rc::downgrade(&self.entries);
            let changes = self.changes.clone();
            let read = reader.clone();
            let update = move || {
                if let Some(entries) = entries.upgrade() {
                    *entries.borrow_mut() = validate(read());
                    changes.invalidate();
                }
            };
            let subscription = source.subscribe(update);
            *self.entries.borrow_mut() = validate(reader());
            subscription
        })
    }

    /// Optional composition bridge. Capability orbs do not import PluginManager.
    #[cfg(feature = "plugin-manager")]
    pub fn from_plugins(index: swarmie_plugin_manager::PluginIndex) -> Self {
        let source = index.changes();
        let reader: Reader = Rc::new(move || index.contributions::<TaskDefinition>());
        Self {
            entries: Rc::new(RefCell::new(Ok(BTreeSet::new()))),
            changes: ChangeSource::default(),
            source: Some((reader, source)),
        }
    }
}

fn validate(
    definitions: impl IntoIterator<Item = TaskDefinition>,
) -> tembra::Result<BTreeSet<TaskDefinition>> {
    let mut result = BTreeSet::new();
    for definition in definitions {
        definition
            .validate()
            .map_err(|e| error(Code::Invalid, Reason::Definition, e))?;
        if !result.insert(definition) {
            return Err(error(
                Code::Conflict,
                Reason::Definition,
                "duplicate task definition",
            ));
        }
    }
    Ok(result)
}
