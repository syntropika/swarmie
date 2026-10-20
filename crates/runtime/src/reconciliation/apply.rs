use std::{
    cell::Cell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
};

use swarmie_composition::{EffectiveInstance, PluginKind};
use swarmie_plugin_contracts::{DeactivateRequest, DesiredState, InstallRequest, UpdateRequest};
use swarmie_plugin_manager::ManagerError;

use super::{
    ChangeKind, InstanceChange, ReconciliationError, ReconciliationReport,
    TransitionOutcome as Outcome, TransitionResult, control,
};
use crate::{Application, Runtime};

struct InFlight(Rc<Cell<bool>>);
impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.set(true);
    }
}

fn result(outcome: Outcome, diagnostic: impl Into<String>) -> TransitionResult {
    TransitionResult {
        outcome,
        diagnostic: diagnostic.into(),
    }
}
fn native(instance: &Option<EffectiveInstance>) -> bool {
    instance
        .as_ref()
        .and_then(|instance| instance.release.as_ref())
        .is_some_and(|release| release.kind == PluginKind::Core)
}
fn request(instance: &EffectiveInstance) -> InstallRequest {
    InstallRequest {
        provider_id: instance.selection.provider.clone().unwrap_or_default(),
        plugin_id: instance.selection.plugin.clone(),
        version: instance.selection.version.clone(),
        confirmed: true,
    }
}
fn needs_retirement(change: &InstanceChange) -> bool {
    change.before.is_some()
        && change
            .before
            .as_ref()
            .map(|instance| &instance.selection.plugin)
            != change
                .after
                .as_ref()
                .map(|instance| &instance.selection.plugin)
}

impl Runtime {
    /// Applies supported transitions while retaining distinct desired and acknowledged inputs.
    ///
    /// All native factory validation, dynamic artifact preflight and available read-only plugin
    /// validation precede desired persistence. Native changes and code+configuration changes
    /// require restart. Same-release Dynamic reconfiguration uses an explicit owning plugin
    /// protocol through actual Tembra operations/State; absence requires restart. This is not
    /// atomic across plugins or Hosts. An interrupted admitted transition requires recovery.
    /// Cross-package replacement of one configured instance uses one Tembra reload. Tembra's
    /// actual object identity and compatibility checks determine Reference and State retention.
    ///
    /// # Errors
    /// Refuses invalid metadata/configuration and administrative workflow/persistence errors.
    /// Per-plugin preflight and runtime refusals are returned in the report. A persisted desired
    /// revision can differ from acknowledged input; neither proves live object readiness.
    pub async fn apply_application(
        &mut self,
        application: Application,
    ) -> Result<ReconciliationReport, ReconciliationError> {
        let plan = self.inspect_application(&application)?;
        let mut report = ReconciliationReport {
            results: plan
                .changes
                .iter()
                .map(|change| {
                    (
                        change.instance_id.clone(),
                        result(Outcome::Unchanged, "no runtime transition acknowledged"),
                    )
                })
                .collect(),
            plan,
            desired_persisted: false,
            recovery_required: false,
        };
        let changes = report.plan.changes.clone();
        if self.manager.requires_recovery()
            || self.reconciliation_uncertain.get()
            || self
                .recovery_error
                .as_ref()
                .is_some_and(ManagerError::requires_restart)
        {
            report.recovery_required = true;
            for change in &changes {
                report.results.insert(
                    change.instance_id.clone(),
                    result(
                        Outcome::RecoveryRequired,
                        "prior startup or interrupted transition requires a fresh Host",
                    ),
                );
            }
            return Ok(report);
        }
        let manager = self.manager.clone();
        let _permit = manager.begin_composition_reconciliation()?;
        let mut restart = BTreeSet::new();
        let mut restart_facets = BTreeSet::new();
        let mut controls = BTreeMap::new();
        for change in &changes {
            if change.kind == ChangeKind::Unchanged {
                continue;
            }
            if native(&change.before)
                || native(&change.after)
                || (change.kind == ChangeKind::Replaced
                    && change
                        .before
                        .as_ref()
                        .zip(change.after.as_ref())
                        .is_some_and(|(before, after)| {
                            before.selection.config != after.selection.config
                        }))
            {
                restart.insert(change.instance_id.clone());
                for instance in change.before.iter().chain(change.after.iter()) {
                    restart_facets.extend(
                        instance.release.iter().flat_map(|release| {
                            release.facets.iter().map(|facet| facet.id.clone())
                        }),
                    );
                }
            }
        }
        // A retained native owner cannot be displaced by adding a different instance label.
        for change in &changes {
            if change.kind != ChangeKind::Unchanged
                && change.after.as_ref().is_some_and(|instance| {
                    instance.release.as_ref().is_some_and(|release| {
                        release
                            .facets
                            .iter()
                            .any(|facet| restart_facets.contains(&facet.id))
                    })
                })
            {
                restart.insert(change.instance_id.clone());
            }
        }
        let mut preflight_failure = None;
        for change in &changes {
            if change.kind == ChangeKind::Unchanged {
                continue;
            }
            if let Some(after) = &change.after {
                if after
                    .release
                    .as_ref()
                    .is_some_and(|release| release.kind == PluginKind::Dynamic)
                    && matches!(change.kind, ChangeKind::Added | ChangeKind::Replaced)
                {
                    match manager.validate_release(request(after)).await {
                        Ok(manifest) => {
                            if let Err(error) = crate::deployment::check_manifest(
                                after.release.as_ref().expect("resolved release"),
                                &manifest,
                            ) {
                                preflight_failure = Some(error.to_string());
                                break;
                            }
                        }
                        Err(error) => {
                            preflight_failure = Some(error.to_string());
                            break;
                        }
                    }
                }
                if change.kind == ChangeKind::Reconfigured && !restart.contains(&change.instance_id)
                {
                    let validate = control::operation(&self.host, after, "validate");
                    let apply = control::operation(&self.host, after, "apply");
                    match (validate, apply) {
                        (Ok(Some(validate)), Ok(Some(apply))) => {
                            if let Err(error) = control::invoke(
                                &self.host,
                                validate,
                                after.selection.config.clone(),
                            )
                            .await
                            {
                                preflight_failure = Some(error.to_string());
                                break;
                            }
                            controls.insert(change.instance_id.clone(), apply);
                        }
                        (Ok(_), Ok(_)) => {
                            restart.insert(change.instance_id.clone());
                        }
                        (Err(error), _) | (_, Err(error)) => {
                            preflight_failure = Some(error.to_string());
                            break;
                        }
                    }
                }
            }
        }
        if let Some(error) = preflight_failure {
            for change in &changes {
                if change.kind != ChangeKind::Unchanged {
                    report.results.insert(
                        change.instance_id.clone(),
                        result(
                            Outcome::Failed,
                            format!("revision refused before desired publication: {error}"),
                        ),
                    );
                }
            }
            return Ok(report);
        }
        let selection = report
            .plan
            .desired
            .instances
            .values()
            .filter(|instance| {
                instance.state == swarmie_composition::SelectionState::Enabled
                    && instance
                        .release
                        .as_ref()
                        .is_some_and(|release| release.kind == PluginKind::Dynamic)
            })
            .map(|instance| instance.selection.plugin.clone())
            .collect();
        manager.set_live_composition_selection(&selection)?;
        manager.save_composition(
            &serde_json::to_value(&application)
                .map_err(|error| ReconciliationError::Validation(error.into()))?,
        )?;
        self.desired = application;
        self.effective = report.plan.desired.clone();
        report.desired_persisted = true;
        let in_flight = InFlight(self.reconciliation_uncertain.clone());
        let mut failed = BTreeSet::new();
        for change in &changes {
            if change.kind == ChangeKind::Unchanged {
                if let Some(after) = &change.after {
                    self.applied
                        .insert(change.instance_id.clone(), after.clone());
                }
                report.results.insert(
                    change.instance_id.clone(),
                    result(
                        Outcome::Unchanged,
                        "selection and acknowledged configuration are unchanged",
                    ),
                );
            } else if restart.contains(&change.instance_id) {
                report.results.insert(change.instance_id.clone(), result(Outcome::RestartRequired, "transition needs a fresh Host or an owning configuration protocol; acknowledged selection retained"));
            } else if needs_retirement(change) && change.after.is_none() {
                let before = change.before.as_ref().expect("retirement input");
                match manager
                    .deactivate(DeactivateRequest {
                        plugin_id: before.selection.plugin.clone(),
                    })
                    .await
                {
                    Ok(_) => {
                        self.applied.remove(&change.instance_id);
                        self.configuration_values.remove(&change.instance_id);
                        report.results.insert(change.instance_id.clone(), result(Outcome::Applied, "server Facets logically retired; persistent application data retained; browser retirement requires Host observation"));
                    }
                    Err(error) => {
                        let recovery = error.requires_restart() || manager.requires_recovery();
                        failed.insert(change.instance_id.clone());
                        report.recovery_required |= recovery;
                        report.results.insert(
                            change.instance_id.clone(),
                            result(
                                if recovery {
                                    Outcome::RecoveryRequired
                                } else {
                                    Outcome::Failed
                                },
                                error.to_string(),
                            ),
                        );
                    }
                }
            }
        }
        let mut additions = Vec::new();
        for change in &changes {
            if change.kind == ChangeKind::Unchanged
                || restart.contains(&change.instance_id)
                || failed.contains(&change.instance_id)
            {
                continue;
            }
            let Some(after) = &change.after else {
                continue;
            };
            if report.recovery_required {
                report.results.insert(
                    change.instance_id.clone(),
                    result(
                        Outcome::RecoveryRequired,
                        "earlier transition requires recovery; no further mutation attempted",
                    ),
                );
                continue;
            }
            if change.kind == ChangeKind::Replaced {
                let before = change.before.as_ref().expect("replacement input");
                // Input is unchanged; both fixed Facet identities need it while Tembra
                // prepares and adopts the replacement. No configuration epoch is created.
                self.configuration_values
                    .allow_replacement_facets(before, after);
            }
            if needs_retirement(change) {
                let before = change.before.as_ref().expect("replacement input");
                let transition = async {
                    let _inputs = crate::configuration::hold_inputs(&self.host, after).await?;
                    manager
                        .replace_dynamic(&before.selection.plugin, request(after))
                        .await
                }
                .await;
                match transition {
                    Ok(_) => self.acknowledge(after, &mut report),
                    Err(error) => {
                        let recovery = error.requires_restart() || manager.requires_recovery();
                        if !recovery {
                            self.configuration_values.publish(before);
                        }
                        report.recovery_required |= recovery;
                        report.results.insert(
                            change.instance_id.clone(),
                            result(
                                if recovery {
                                    Outcome::RecoveryRequired
                                } else {
                                    Outcome::Failed
                                },
                                error.to_string(),
                            ),
                        );
                    }
                }
                continue;
            }
            if change.kind == ChangeKind::Reconfigured {
                match control::invoke(
                    &self.host,
                    controls[&change.instance_id].clone(),
                    after.selection.config.clone(),
                )
                .await
                {
                    Ok(()) => self.acknowledge(after, &mut report),
                    Err(error) => {
                        report.recovery_required = true;
                        report.results.insert(change.instance_id.clone(), result(Outcome::RecoveryRequired, format!("owning configuration application failed; effects are unconfirmed: {error}")));
                    }
                }
                continue;
            }
            let publication = change.before.is_none();
            if publication {
                self.configuration_values.publish(after);
            }
            let transition = async {
                let _inputs = if change.kind == ChangeKind::Replaced {
                    crate::configuration::hold_inputs(&self.host, after).await?
                } else {
                    Vec::new()
                };
                let inventory = manager.inventory()?;
                let installed = inventory
                    .plugins
                    .iter()
                    .find(|plugin| plugin.plugin_id == after.selection.plugin);
                let mut admitted_by_update = false;
                match installed {
                    None => {
                        manager.install(request(after)).await?;
                    }
                    Some(plugin) if plugin.version != after.selection.version => {
                        let active_intent = plugin.desired == DesiredState::Active;
                        manager
                            .update(UpdateRequest {
                                provider_id: after.selection.provider.clone().unwrap_or_default(),
                                plugin_id: after.selection.plugin.clone(),
                                version: after.selection.version.clone(),
                                confirmed: true,
                            })
                            .await?;
                        admitted_by_update = active_intent;
                    }
                    Some(_) if !publication => {
                        manager.reload_cached(&after.selection.plugin).await?;
                    }
                    _ => {}
                }
                Ok::<_, ManagerError>(admitted_by_update)
            }
            .await;
            match transition {
                Ok(false) if publication => additions.push(after.clone()),
                Ok(_) => self.acknowledge(after, &mut report),
                Err(error) => {
                    let recovery = error.requires_restart() || manager.requires_recovery();
                    if publication && !recovery {
                        self.configuration_values.remove(&after.selection.id);
                    } else if change.kind == ChangeKind::Replaced && !recovery {
                        self.configuration_values
                            .publish(change.before.as_ref().expect("replacement input"));
                    }
                    report.recovery_required |= recovery;
                    report.results.insert(
                        change.instance_id.clone(),
                        result(
                            if recovery {
                                Outcome::RecoveryRequired
                            } else {
                                Outcome::Failed
                            },
                            error.to_string(),
                        ),
                    );
                }
            }
        }
        if !additions.is_empty() && !report.recovery_required {
            let ids = additions
                .iter()
                .map(|instance| instance.selection.plugin.clone())
                .collect::<Vec<_>>();
            match manager.activate_batch(&ids).await {
                Ok(_) => {
                    for instance in additions {
                        self.acknowledge(&instance, &mut report);
                    }
                }
                Err(error) => {
                    let recovery = error.requires_restart() || manager.requires_recovery();
                    report.recovery_required |= recovery;
                    for instance in additions {
                        if !recovery {
                            self.configuration_values.remove(&instance.selection.id);
                        }
                        report.results.insert(
                            instance.selection.id.clone(),
                            result(
                                if recovery {
                                    Outcome::RecoveryRequired
                                } else {
                                    Outcome::Failed
                                },
                                error.to_string(),
                            ),
                        );
                    }
                }
            }
        } else if !additions.is_empty() {
            for instance in additions {
                self.configuration_values.remove(&instance.selection.id);
                report.results.insert(instance.selection.id.clone(), result(Outcome::RecoveryRequired, "cached addition was not activated because an earlier transition requires recovery"));
            }
        }
        report.recovery_required |= manager.requires_recovery();
        drop(in_flight);
        self.reconciliation_uncertain.set(report.recovery_required);
        Ok(report)
    }

    fn acknowledge(&mut self, instance: &EffectiveInstance, report: &mut ReconciliationReport) {
        self.configuration_values.publish(instance);
        self.applied
            .insert(instance.selection.id.clone(), instance.clone());
        self.deployment_errors.remove(&instance.selection.id);
        report.results.insert(instance.selection.id.clone(), result(Outcome::Applied, "owning Tembra transition acknowledged; live readiness remains observable through References"));
    }
}
