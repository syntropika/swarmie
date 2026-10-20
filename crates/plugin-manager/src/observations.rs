use swarmie_plugin_contracts::*;
use tembra::core;

use crate::{ManagerError, PluginManager, Result};

impl PluginManager {
    /// Retains an actual Tembra Reference to observe a selected capability's availability.
    /// The caller must acquire the intended Contract; this does not register a provider or
    /// create another dependency graph. All references for one plugin must remain live.
    pub fn observe_reference(&self, plugin_id: &str, reference: core::Reference) -> Result<()> {
        if !self
            .cores
            .borrow()
            .iter()
            .any(|plugin| plugin.plugin_id == plugin_id)
            && self.storage()?.load(plugin_id)?.is_none()
        {
            return Err(ManagerError::NotInstalled(plugin_id.into()));
        }
        if let Some(failures) = self.reference_failures.borrow_mut().get_mut(plugin_id) {
            failures.remove(reference.contract().name());
            failures.remove("");
        }
        let mut references = self.references.borrow_mut();
        let references = references.entry(plugin_id.into()).or_default();
        references.retain(|previous| previous.contract().name() != reference.contract().name());
        references.push(reference);
        Ok(())
    }

    /// Records a failed acquisition when Tembra could not return a Reference to observe.
    /// A successful replacement observation clears this session-local failure.
    pub fn observe_failure(&self, plugin_id: &str, error: String) -> Result<()> {
        if !self
            .cores
            .borrow()
            .iter()
            .any(|plugin| plugin.plugin_id == plugin_id)
            && self.storage()?.load(plugin_id)?.is_none()
        {
            return Err(ManagerError::NotInstalled(plugin_id.into()));
        }
        self.observe_contract_failure(plugin_id, "", error)?;
        Ok(())
    }

    /// Records a failed acquisition for one Contract without masking other capabilities.
    pub fn observe_contract_failure(
        &self,
        plugin_id: &str,
        contract: &str,
        error: String,
    ) -> Result<()> {
        if !self
            .cores
            .borrow()
            .iter()
            .any(|plugin| plugin.plugin_id == plugin_id)
            && self.storage()?.load(plugin_id)?.is_none()
        {
            return Err(ManagerError::NotInstalled(plugin_id.into()));
        }
        self.reference_failures
            .borrow_mut()
            .entry(plugin_id.into())
            .or_default()
            .insert(contract.into(), error);
        Ok(())
    }

    pub(crate) fn reference_observation(&self, id: &str) -> Option<(ObservedState, Vec<String>)> {
        if let Some(failures) = self.reference_failures.borrow().get(id)
            && !failures.is_empty()
        {
            return Some((
                ObservedState::Unavailable,
                failures.values().cloned().collect(),
            ));
        }
        let references = self.references.borrow();
        let references = references.get(id)?;
        let mut state = ObservedState::Active;
        let mut errors = vec![];
        for reference in references {
            match reference.status() {
                core::RefStatus::Live => {}
                core::RefStatus::Loading if state == ObservedState::Active => {
                    state = ObservedState::Blocked
                }
                core::RefStatus::Loading => {}
                core::RefStatus::Gone | core::RefStatus::Released
                    if state != ObservedState::Failed =>
                {
                    state = ObservedState::Unavailable
                }
                core::RefStatus::Gone | core::RefStatus::Released => {}
            }
            if let Some(error) = reference.error() {
                if matches!(
                    error.code,
                    core::ErrorCode::Invalid
                        | core::ErrorCode::ContractMismatch
                        | core::ErrorCode::Forbidden
                        | core::ErrorCode::NotInVariant
                ) {
                    state = ObservedState::Failed;
                }
                errors.push(error.to_string());
            }
        }
        if state != ObservedState::Active && errors.is_empty() {
            errors.push("required capability is not live in Tembra".into());
        }
        Some((state, errors))
    }
}
