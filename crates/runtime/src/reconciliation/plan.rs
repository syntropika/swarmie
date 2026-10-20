use std::collections::BTreeSet;

use swarmie_composition::SelectionState;
use swarmie_plugin_manager::ManagerError;

use super::{ChangeKind, InstanceChange, ReconciliationPlan};
use crate::{Application, Runtime};

/// Refusal before applying a revision, or failure to persist its desired intent.
#[derive(Debug, thiserror::Error)]
pub enum ReconciliationError {
    #[error("invalid application revision: {0}")]
    Validation(#[source] Box<dyn std::error::Error>),
    #[error("administrative composition workflow: {0}")]
    Administration(#[from] ManagerError),
}

impl Runtime {
    /// Inspects a replacement relative to acknowledged selections without mutating a Host.
    /// Native factories validate configuration but must not open resources during preparation.
    /// Package availability and opt-in Dynamic configuration validation are checked by apply.
    /// # Errors
    /// Refuses invalid metadata, unavailable release facts and owning native configuration.
    pub fn inspect_application(
        &self,
        application: &Application,
    ) -> Result<ReconciliationPlan, ReconciliationError> {
        let prepared = crate::preparation::prepare(application, &self.compiled)
            .map_err(ReconciliationError::Validation)?;
        if prepared
            .effective
            .instances
            .values()
            .any(|instance| matches!(instance.state, SelectionState::UnavailableRelease { .. }))
        {
            return Err(ReconciliationError::Validation(
                "requested release metadata is unavailable".into(),
            ));
        }
        let selected = prepared
            .effective
            .instances
            .iter()
            .filter(|(_, instance)| instance.state == SelectionState::Enabled)
            .map(|(id, _)| id.clone())
            .collect::<BTreeSet<_>>();
        let ids = selected
            .union(&self.applied.keys().cloned().collect())
            .cloned()
            .collect::<Vec<_>>();
        let changes = ids
            .into_iter()
            .map(|instance_id| {
                let before = self.applied.get(&instance_id).cloned();
                let after = selected
                    .contains(&instance_id)
                    .then(|| prepared.effective.instances[&instance_id].clone());
                let kind = match (&before, &after) {
                    (None, Some(_)) => ChangeKind::Added,
                    (Some(_), None) => ChangeKind::Removed,
                    (Some(before), Some(after))
                        if before.release != after.release
                            || before.selection.plugin != after.selection.plugin
                            || before.selection.provider != after.selection.provider =>
                    {
                        ChangeKind::Replaced
                    }
                    (Some(before), Some(after))
                        if before.selection.config != after.selection.config =>
                    {
                        ChangeKind::Reconfigured
                    }
                    _ => ChangeKind::Unchanged,
                };
                InstanceChange {
                    instance_id,
                    kind,
                    before,
                    after,
                }
            })
            .collect();
        Ok(ReconciliationPlan {
            desired: prepared.effective,
            changes,
        })
    }

    /// Returns the last durably accepted desired document, which may require restart to apply.
    pub fn desired_application(&self) -> &Application {
        &self.desired
    }
}
