use crate::UiRegistryMetadata;

/// Browser-facet contribution metadata tied to the provider object's lifecycle.
#[tembra::contract(local, name = "swarmie.ui-contributions", version = 1)]
pub trait UiContributions {
    #[state]
    fn contributions(&self) -> Vec<UiRegistryMetadata>;
}

/// Binds one fixed executable Facet to its single configured application instance.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    tembra::serde::Serialize,
    tembra::serde::Deserialize,
    tembra::schemars::JsonSchema,
)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct PluginConfigurationInput {
    pub facet_id: String,
}

/// Configuration input delivered to one selected instance. Initial admission exposes the
/// candidate for plugin-owned validation; the supervisor records acknowledged inputs
/// separately from desired intent and withdraws inputs after failed admission.
#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    tembra::serde::Serialize,
    tembra::serde::Deserialize,
    tembra::schemars::JsonSchema,
)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ConfiguredValue {
    pub instance_id: String,
    pub configuration: tembra::serde_json::Value,
}

/// Read-only configuration delivery. Each owning plugin validates its own schema.
/// Hot changes require an explicit plugin-owned validation/application protocol; Tembra
/// State and Resources are not automatically reinitialized by compatible code replacement.
#[tembra::contract(local, name = "swarmie.plugin-configuration", version = 1, input = PluginConfigurationInput)]
pub trait PluginConfiguration {
    #[portable]
    async fn read(&self) -> Result<ConfiguredValue>;
}

/// Lifecycle-owned, opt-in configuration control operations keyed by `<facet>:<phase>`.
/// The owning operation must validate without mutation and apply through its own Tembra
/// state/lifecycle. Registry metadata routes trusted code; it is not caller authorization.
pub const CONFIGURATION_REGISTRY: &str = "swarmie.configuration-controls";

/// Method-schema vocabulary for optional lifecycle-owned configuration operations.
/// Implementations register their own Contracts' methods in CONFIGURATION_REGISTRY;
/// this schema does not create another provider or make registration mandatory.
#[tembra::contract(local, name = "swarmie.configuration-control-schema", version = 1)]
pub trait PluginConfigurationControlSchema {
    /// Validates candidate input without mutating configuration or external state.
    #[portable]
    async fn validate(&self, configuration: tembra::serde_json::Value) -> Result<()>;

    /// Applies validated input through the owning plugin's actual Tembra state/lifecycle.
    /// Failure or cancellation may follow effects; the supervisor must retain uncertainty.
    #[portable]
    async fn apply(&self, configuration: tembra::serde_json::Value) -> Result<()>;
}
