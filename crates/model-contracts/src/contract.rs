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
    #[portable]
    async fn capabilities(&self, model: ModelIdentity) -> Result<Capabilities>;

    #[portable]
    async fn models(&self, connection: String) -> Result<ModelList>;

    /// Cancellation of this invocation aborts local I/O, without asserting remote rollback.
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
    #[portable]
    async fn start(&self) -> Result<()>;

    /// Single-consumer acknowledgement, batched by State publication rather than per token.
    #[portable]
    async fn acknowledge(&self, through: u64) -> Result<()>;

    /// Requests cancellation. Observe physically_settled before treating local work as stopped.
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
