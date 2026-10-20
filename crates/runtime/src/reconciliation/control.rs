use serde_json::Value;
use swarmie_composition::EffectiveInstance;
use swarmie_plugin_contracts::{CONFIGURATION_REGISTRY, PluginConfigurationControlSchema};
use tembra::{ContractToken, Host, Result, RuntimeError, core};

/// Discovers the owning method from Tembra's lifecycle-owned Registry, checking its complete
/// method descriptor. Absence is an unsupported hot change, not a fabricated capability.
pub(super) fn operation(
    host: &Host,
    instance: &EffectiveInstance,
    phase: &str,
) -> Result<Option<core::RegisteredOperation>> {
    let registry = core::Registry::new(CONFIGURATION_REGISTRY, core::ConflictPolicy::Error)?;
    let table = host.core().registry(&registry)?.table(None)?;
    let mut found = None;
    for facet in &instance
        .release
        .as_ref()
        .ok_or_else(|| RuntimeError::invalid("configuration", "release absent"))?
        .facets
    {
        for (_, entry) in table.entries() {
            if entry.data() != &serde_json::json!({"facet_id":facet.id,"phase":phase}) {
                continue;
            }
            if found.is_some() {
                return Err(RuntimeError::invalid(
                    "configuration_control",
                    "ambiguous owning configuration operation",
                ));
            }
            let method = entry
                .operation()
                .ok_or_else(|| {
                    RuntimeError::invalid(
                        "configuration_control",
                        "configuration entry has no method",
                    )
                })?
                .clone();
            let expected = PluginConfigurationControlSchema.descriptor()?;
            if method
                .contract()
                .members("")
                .and_then(|members| members.get(method.method()))
                != expected.members("").and_then(|members| members.get(phase))
            {
                return Err(RuntimeError::invalid(
                    "configuration_control",
                    "configuration operation schema differs from the canonical protocol",
                ));
            }
            found = Some(method);
        }
    }
    Ok(found)
}

pub(super) async fn invoke(
    host: &Host,
    method: core::RegisteredOperation,
    value: Value,
) -> Result<()> {
    host.root(move |context| async move {
        method
            .call_json(context.core(), Some(value))
            .await
            .map(|_| ())
    })
    .await
}
