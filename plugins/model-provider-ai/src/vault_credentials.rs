use crate::{AdmittedAttempt, Credentials, ModelCredential};
use futures::future::LocalBoxFuture;
use std::{collections::BTreeMap, rc::Rc};
use swarmie_permissions::PluginGrant;
use swarmie_vault::{
    SecretReference, SecretUseRequest, VaultNode, VaultSession, executors::UseDecisions,
};
use tembra::{Code, Context, Error};

/// Private native provider credential binding. Public model configuration contains reference keys only.
pub struct VaultCredentials {
    node: VaultNode,
    session: VaultSession,
    plugins: BTreeMap<String, PluginGrant>,
    decisions: Rc<dyn UseDecisions>,
    references: BTreeMap<String, SecretReference>,
}

impl VaultCredentials {
    pub fn new(
        node: VaultNode,
        session: VaultSession,
        plugins: BTreeMap<String, PluginGrant>,
        decisions: Rc<dyn UseDecisions>,
        references: BTreeMap<String, SecretReference>,
    ) -> Self {
        Self {
            node,
            session,
            plugins,
            decisions,
            references,
        }
    }
}

fn denied(_: swarmie_vault::Error) -> Error {
    Error::new(
        Code::Forbidden,
        "swarmie.models.credentials",
        "private credential admission failed",
    )
}

impl Credentials for VaultCredentials {
    fn acquire<'a>(
        &'a self,
        context: &'a Context,
        admitted: &'a AdmittedAttempt,
        key: &'a str,
    ) -> LocalBoxFuture<'a, tembra::Result<ModelCredential>> {
        Box::pin(async move {
            context.check()?;
            if reqwest::Url::parse(admitted.destination())
                .map_err(|_| denied(swarmie_vault::Error::Invalid))?
                .scheme()
                != "https"
            {
                return Err(denied(swarmie_vault::Error::Unsupported));
            }
            let execution = admitted
                .execution()
                .ok_or_else(|| denied(swarmie_vault::Error::Forbidden))?;
            let reference = *self
                .references
                .get(key)
                .ok_or_else(|| denied(swarmie_vault::Error::NotFound))?;
            let plugin = self
                .plugins
                .get(admitted.provider())
                .ok_or_else(|| denied(swarmie_vault::Error::Forbidden))?;
            if plugin.provider() != format!("swarmie.models.ai:{}", admitted.provider()) {
                return Err(denied(swarmie_vault::Error::Forbidden));
            }
            self.node
                .refresh_current(&self.session)
                .await
                .map_err(denied)?;
            let intent = self
                .node
                .prepare_use(
                    &self.session,
                    execution,
                    plugin,
                    SecretUseRequest {
                        reference,
                        operation: admitted.operation().as_uuid(),
                        attempt: admitted.attempt(),
                        destination: admitted.destination().into(),
                    },
                )
                .map_err(denied)?;
            if self
                .node
                .requires_approval(&self.session, execution, plugin, &intent)
                .map_err(denied)?
            {
                let (decision, conversation) = self
                    .decisions
                    .request(execution, &intent)
                    .await
                    .map_err(denied)?;
                self.node
                    .approve_use(
                        &self.session,
                        execution,
                        plugin,
                        intent.clone(),
                        conversation,
                        decision,
                    )
                    .map_err(denied)?;
            }
            self.node
                .refresh_current(&self.session)
                .await
                .map_err(denied)?;
            context.check()?;
            // No await between the latest confirmed lease and returning to final HTTP dispatch.
            let lease = self
                .node
                .lease(&self.session, execution, plugin, &intent)
                .map_err(denied)?;
            let value = lease
                .deliver(|value| {
                    Ok(std::str::from_utf8(value)
                        .map_err(|_| swarmie_vault::Error::Invalid)?
                        .to_owned())
                })
                .map_err(denied)?;
            ModelCredential::new(value, reference.to_string())
        })
    }
}
