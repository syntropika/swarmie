use crate::{Approval, Error, Result, SecretMetadata, SecretReference};
use serde::{Deserialize, Serialize};
pub use swarmie_permissions::{
    Authority, CloudOwnerIdentity, CorePlugin, ExecutionBinding, ExecutionIdentity, HumanDecision,
    HumanSession, OwnerBinding, Permission, PluginGrant, ShellIdentity,
};
use uuid::Uuid;

impl From<swarmie_permissions::Error> for Error {
    fn from(e: swarmie_permissions::Error) -> Self {
        match e {
            swarmie_permissions::Error::Forbidden => Self::Forbidden,
            swarmie_permissions::Error::Revoked => Self::Revoked,
            swarmie_permissions::Error::Invalid => Self::Invalid,
        }
    }
}

pub trait VaultEligibility {
    fn eligible(&self, reference: SecretReference, metadata: &SecretMetadata) -> Result<()>;
}

impl VaultEligibility for ExecutionBinding {
    fn eligible(&self, reference: SecretReference, metadata: &SecretMetadata) -> Result<()> {
        if !self.is_live()
            || self.identity().account != Some(reference.account)
            || !metadata.projects.allows(self.identity().project)
            || !metadata.agents.allows(self.identity().agent)
        {
            return Err(Error::Forbidden);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UseIntent {
    pub reference: SecretReference,
    pub metadata: SecretMetadata,
    pub operation: Uuid,
    pub attempt: Uuid,
    pub provider: String,
    pub destination: String,
    pub execution: ExecutionIdentity,
    pub policy_generation: Uuid,
    pub permission_generation: Uuid,
}

pub(crate) struct UseGrant {
    pub shell: OwnerBinding,
    pub execution: ExecutionIdentity,
    pub reference: SecretReference,
    pub policy: Uuid,
    pub provider: String,
    pub permission: Uuid,
    pub conversation: bool,
    pub intent: Option<UseIntent>,
}

impl UseGrant {
    pub fn permits(
        &self,
        binding: &ExecutionBinding,
        intent: &UseIntent,
        policy: Approval,
    ) -> bool {
        binding.is_live()
            && self.shell.is_live()
            && self.shell.same_owner(binding.shell())
            && self.reference == intent.reference
            && self.policy == intent.policy_generation
            && self.provider == intent.provider
            && self.permission == intent.permission_generation
            && self.execution == *binding.identity()
            && if self.conversation {
                policy == Approval::Conversation
                    && binding.identity().main_agent
                    && self.execution.conversation.is_some()
            } else {
                self.intent.as_ref() == Some(intent)
            }
    }
}
