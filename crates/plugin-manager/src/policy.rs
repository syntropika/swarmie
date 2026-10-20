use swarmie_plugin_contracts::AdminProof;

use crate::{ManagerError, PluginManager, Result};

impl PluginManager {
    /// Explicitly trusts a source provider whose acquisition channel the supervisor verified.
    /// The provider owns source authentication. Hashes prove byte integrity, not authorship.
    /// No provider is trusted by default; approval is explicit and session-local.
    /// Remote source approval attests to a verified acquisition channel; it is not a
    /// signature verifier or a claim of publisher authenticity.
    pub fn approve_trusted_source(&self, proof: &AdminProof, provider_id: String) -> Result<()> {
        self.authorize(proof)?;
        if provider_id.is_empty() {
            return Err(ManagerError::InvalidPackage(
                "empty trusted source identity".into(),
            ));
        }
        self.trusted_sources.borrow_mut().insert(provider_id);
        Ok(())
    }
}
