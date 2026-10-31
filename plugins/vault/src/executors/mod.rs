//! Qualified native delivery channels. No plaintext argument expansion or general Bash sandbox.
mod bash;
mod capture;
mod http;
pub use capture::*;

pub use bash::*;
pub use http::*;

use crate::{
    ExecutionBinding, HumanDecision, PluginGrant, Result, SecretLease, SecretReference,
    SecretUseRequest, UseIntent, VaultNode, VaultSession,
};
use futures::future::LocalBoxFuture;
use uuid::Uuid;

/// Private UI decision owner. Implementations must return actual user actions, never model text.
pub trait UseDecisions {
    fn request<'a>(
        &'a self,
        execution: &'a ExecutionBinding,
        intent: &'a UseIntent,
    ) -> LocalBoxFuture<'a, Result<(HumanDecision, bool)>>;
}

/// Fixed physical attempt issued by the actual tool/execution owner.
pub struct ExecutionAttempt<'a> {
    pub node: &'a VaultNode,
    pub session: &'a VaultSession,
    pub execution: &'a ExecutionBinding,
    pub plugin: &'a PluginGrant,
    pub decisions: &'a dyn UseDecisions,
    pub operation: Uuid,
    pub attempt: Uuid,
}

impl ExecutionAttempt<'_> {
    async fn approvals(
        &self,
        references: &[SecretReference],
        destination: &str,
    ) -> Result<Vec<UseIntent>> {
        self.node.refresh_current(self.session).await?;
        let mut intents = vec![];
        let unique: std::collections::BTreeSet<_> = references.iter().copied().collect();
        for reference in unique {
            let intent = self.node.prepare_use(
                self.session,
                self.execution,
                self.plugin,
                SecretUseRequest {
                    reference,
                    operation: self.operation,
                    attempt: self.attempt,
                    destination: destination.into(),
                },
            )?;
            // Ask before acquiring *any* plaintext lease, so another approval cannot pin old values.
            if self
                .node
                .requires_approval(self.session, self.execution, self.plugin, &intent)?
            {
                let (decision, conversation) =
                    self.decisions.request(self.execution, &intent).await?;
                self.node.approve_use(
                    self.session,
                    self.execution,
                    self.plugin,
                    intent.clone(),
                    conversation,
                    decision,
                )?;
            }
            intents.push(intent);
        }
        self.node.refresh_current(self.session).await?;
        Ok(intents)
    }
    fn leases(&self, intents: &[UseIntent]) -> Result<Vec<SecretLease>> {
        intents
            .iter()
            .map(|intent| {
                self.node
                    .lease(self.session, self.execution, self.plugin, intent)
            })
            .collect()
    }
}
