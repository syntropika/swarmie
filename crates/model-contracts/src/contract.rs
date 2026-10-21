use crate::*;
use tembra::{core, prelude::*};

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct GenerationInput {
    pub request: ModelRequest,
}

/// Canonical schema implemented at independently named provider endpoints.
#[tembra::contract(local, name = "swarmie.model-provider-schema", version = 1)]
pub trait ModelProviderSchema {
    /// Describes implemented paths; model-dependent support is not a remote capability probe.
    /// # Errors
    /// Returns a Tembra error for an unknown configured identity or unavailable endpoint.
    #[portable]
    async fn capabilities(&self, model: ModelIdentity) -> Result<Capabilities>;

    /// Discovers candidates without changing the configured model admission policy.
    /// # Errors
    /// Provider failures are returned in ModelList; Tembra lifecycle/dispatch can fail separately.
    #[portable]
    async fn models(&self, connection: String) -> Result<ModelList>;

    /// Executes one inference. Cancellation aborts local I/O without asserting remote rollback.
    /// # Guarantees
    /// NotSent errors identify local refusal; other outcomes never promise remote nonprocessing.
    /// # Errors
    /// Provider failures are typed results. Tembra scope/dispatch failures can prevent delivery
    /// of that result after remote effects, so callers must retain their own attempt identity.
    #[portable]
    async fn generate(&self, request: ModelRequest) -> Result<GenerateResult>;
}

/// Managed generation schema. Acquisition prepares only. Keep the Reference alive from
/// start through physical settlement. Its last claim releasing cancels the managed task.
#[tembra::contract(local, name = "swarmie.model-generation-schema", version = 1, input = GenerationInput)]
pub trait ModelGenerationSchema {
    #[state]
    fn progress(&self) -> GenerationState;

    /// Idempotent within this live object. An acknowledged start does not prove remote admission.
    /// # Errors
    /// Returns Tembra lifecycle/publication errors; a refused start does not restart inference.
    #[portable]
    async fn start(&self) -> Result<()>;

    /// Single-consumer acknowledgement, batched by State publication rather than per token.
    /// # Errors
    /// Refuses future cursors without mutation, or reports a Tembra lifecycle/publication error.
    #[portable]
    async fn acknowledge(&self, through: u64) -> Result<()>;

    /// Reads bounded retained completion even when terminal State publication was rejected.
    /// # Guarantees
    /// Does not submit or restart inference. After local I/O settlement this may be ahead of
    /// progress State. Keep the Reference alive; losing the object loses this volatile result.
    /// # Errors
    /// Returns Tembra admission/serialization errors if the object is no longer callable.
    #[portable]
    async fn recover(&self) -> Result<GenerationState>;

    /// Requests cancellation. Observe physically_settled before treating local work as stopped.
    /// # Errors
    /// Returns Tembra admission errors when the Reference is no longer callable. Cancellation
    /// cannot undo a remote submission or guarantee that the caller receives a terminal State.
    #[portable]
    async fn cancel(&self) -> Result<()>;
}

/// Registry entry resolves independently named provider and generation Contract identities.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "tembra::serde", deny_unknown_fields)]
#[schemars(crate = "tembra::schemars")]
pub struct ProviderRegistration {
    pub provider: String,
    pub version: u32,
    pub provider_contract: String,
    pub generation_contract: String,
    pub connections: Vec<String>,
}

pub fn model_providers() -> Result<core::Registry> {
    core::Registry::new("swarmie.model-providers", core::ConflictPolicy::Error)
}
