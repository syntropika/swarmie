use crate::{PluginContext, PluginId, PluginIndex};
use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU32,
    rc::Rc,
};
use tembra::Orb;

/// Optional composition metadata around an independently usable orb factory.
#[derive(Clone)]
pub struct CorePlugin {
    pub(crate) id: PluginId,
    pub(crate) version: NonZeroU32,
    pub(crate) dependencies: Vec<PluginId>,
    pub(crate) contributions: Vec<Rc<dyn std::any::Any>>,
    pub(crate) create: Rc<dyn Fn(PluginContext) -> tembra::Result<Vec<Orb>>>,
}

impl CorePlugin {
    /// Receives the composition's Tembra host and live contribution index at installation.
    /// Factories declare capabilities; resource opening remains managed by Tembra.
    pub fn new(
        id: PluginId,
        version: NonZeroU32,
        create: impl Fn(PluginContext) -> tembra::Result<Vec<Orb>> + 'static,
    ) -> Self {
        Self {
            id,
            version,
            dependencies: Vec::new(),
            create: Rc::new(create),
            contributions: Vec::new(),
        }
    }

    /// Adds typed native contribution metadata without coupling the capability orb.
    pub fn contributes<T: 'static>(mut self, contribution: T) -> Self {
        self.contributions.push(Rc::new(contribution));
        self
    }

    /// Declares plugin-level startup ordering; Tembra still validates orb contracts.
    pub fn requires(mut self, dependencies: impl IntoIterator<Item = PluginId>) -> Self {
        self.dependencies = dependencies.into_iter().collect();
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum CatalogError {
    #[error("duplicate compiled core plugin: {0}")]
    Duplicate(PluginId),
    #[error("required compiled plugin is missing: {0}")]
    RequiredMissing(PluginId),
}

/// Compiled factories available to this binary, not a second service registry.
#[derive(Clone, Default)]
pub struct Catalog(
    pub(crate) BTreeMap<PluginId, CorePlugin>,
    pub(crate) PluginIndex,
    pub(crate) BTreeSet<PluginId>,
);

impl Catalog {
    /// Returns the shared contribution index for this composition.
    pub fn index(&self) -> PluginIndex {
        self.1.clone()
    }

    /// Rejects duplicate identities instead of silently replacing compiled definitions.
    pub fn new(plugins: impl IntoIterator<Item = CorePlugin>) -> Result<Self, CatalogError> {
        let mut catalog = BTreeMap::new();
        for plugin in plugins {
            if catalog.contains_key(&plugin.id) {
                return Err(CatalogError::Duplicate(plugin.id));
            }
            catalog.insert(plugin.id.clone(), plugin);
        }
        Ok(Self(catalog, PluginIndex::default(), BTreeSet::new()))
    }

    /// Select mandatory plugins and their dependencies before opening a runtime.
    /// The policy is host-owned and cannot be disabled through persisted inventory.
    pub fn require(
        mut self,
        plugins: impl IntoIterator<Item = PluginId>,
    ) -> Result<Self, CatalogError> {
        let mut pending: Vec<_> = plugins.into_iter().collect();
        let mut required = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !required.insert(id.clone()) {
                continue;
            }
            let definition = self
                .0
                .get(&id)
                .ok_or_else(|| CatalogError::RequiredMissing(id.clone()))?;
            pending.extend(definition.dependencies.iter().cloned());
        }
        self.1 = PluginIndex::with_required(required.clone());
        self.2 = required;
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_core_definitions_do_not_silently_replace_each_other() {
        let plugin = CorePlugin::new(
            PluginId::new("core.counter").unwrap(),
            NonZeroU32::new(1).unwrap(),
            |_| Ok(vec![]),
        );
        assert!(matches!(
            Catalog::new([plugin.clone(), plugin]),
            Err(CatalogError::Duplicate(_))
        ));
    }
}
