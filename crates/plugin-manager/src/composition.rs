use std::{
    cell::{Cell as Flag, RefCell},
    collections::BTreeSet,
};

use swarmie_plugin_contracts::{AdminProof, PluginManifest};

use crate::{ManagerError, PluginManager, Result};

/// Scoped supervisor permission to reconcile one validated declarative composition.
/// Dropping the guard restores the rejection of direct imperative lifecycle mutations.
pub struct CompositionGuard<'a> {
    reconciling: &'a Flag<bool>,
    selection: &'a RefCell<Option<BTreeSet<String>>>,
    staged: &'a RefCell<BTreeSet<String>>,
}
impl Drop for CompositionGuard<'_> {
    fn drop(&mut self) {
        self.reconciling.set(false);
        self.selection.borrow_mut().take();
        self.staged.borrow_mut().clear();
    }
}

impl PluginManager {
    /// Authorizes applying a validated desired composition using the administrative lifecycle.
    pub fn begin_composition_reconciliation(
        &self,
        proof: &AdminProof,
    ) -> Result<CompositionGuard<'_>> {
        self.authorize(proof)?;
        if self.busy.get() || self.reconciling.replace(true) {
            return Err(ManagerError::Busy);
        }
        Ok(CompositionGuard {
            reconciling: &self.reconciling,
            selection: &self.composition_selection,
            staged: &self.composition_staged,
        })
    }

    /// Scopes fresh-Host admission to this resolved revision's selected dynamic packages.
    /// Cached releases outside the revision retain their bytes but cannot reserve live facet
    /// identities or impose dependencies on the replacement composition. Call before staging.
    pub fn set_composition_selection(
        &self,
        proof: &AdminProof,
        selected: &BTreeSet<String>,
    ) -> Result<()> {
        self.authorize(proof)?;
        if !self.reconciling.get() || !self.fresh_session.get() {
            return Err(ManagerError::CompositionManaged);
        }
        if self.busy.get() || !self.composition_staged.borrow().is_empty() {
            return Err(ManagerError::Busy);
        }
        *self.composition_selection.borrow_mut() = Some(selected.clone());
        Ok(())
    }

    pub(crate) fn composition_candidate_relevant(&self, id: &str) -> bool {
        self.composition_selection
            .borrow()
            .as_ref()
            .is_none_or(|selected| {
                selected.contains(id) && self.composition_staged.borrow().contains(id)
            })
    }

    pub(crate) fn composition_reconciliation_active(&self) -> bool {
        self.reconciling.get()
    }

    pub(crate) fn require_composition_permission(&self) -> Result<()> {
        if self.storage()?.composition()?.is_some() && !self.reconciling.get() {
            return Err(ManagerError::CompositionManaged);
        }
        Ok(())
    }

    /// Persists the validated declared Profile and Bundles as the sole desired composition.
    /// Validation belongs to the composition caller; this layer does not interpret its schema.
    pub fn save_composition(
        &self,
        proof: &AdminProof,
        composition: &serde_json::Value,
    ) -> Result<()> {
        self.authorize(proof)?;
        if !self.reconciling.get() {
            return Err(ManagerError::CompositionManaged);
        }
        self.storage()?.save_composition(composition)
    }

    /// Reads the durable declared composition without synthesizing an effective second copy.
    pub fn composition(&self, proof: &AdminProof) -> Result<Option<serde_json::Value>> {
        self.authorize(proof)?;
        self.storage()?.composition()
    }

    /// Reads admitted immutable package metadata for composition planning before activation.
    pub fn installed_packages(&self, proof: &AdminProof) -> Result<Vec<PluginManifest>> {
        self.authorize(proof)?;
        Ok(self
            .storage()?
            .all()?
            .into_iter()
            .map(|plugin| plugin.package.manifest)
            .collect())
    }
}
