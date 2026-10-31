//! Concrete account configuration capture. The provider account owner controls
//! persistence, selection, source resolution and credential lifecycle.
mod catalog;
mod discovery;
mod driver;
mod metadata;
mod qualifications;

use crate::ai::{
    AiModel, Connection, ConnectionId, Credentials, ModelId, ModelProtection, Provider,
};
use crate::{
    AccountRecord, CapturedModel, CredentialRequirement, Model, ModelConstruction, ModelFactory,
    ModelSelection,
};
use futures::future::LocalBoxFuture;
use serde::{Deserialize, Serialize};
use std::{rc::Rc, sync::Arc};
use swarmie_ai::catalog::Snapshot;
use tembra::{Code, Context, Error};
use zeroize::Zeroizing;

/// The current flat adapter fields. Unknown fields remain in AccountRecord and
/// are not discarded by reading this typed view. No credential is stored here.
#[derive(Deserialize, Serialize)]
struct StoredConfiguration {
    provider: Provider,
    endpoint: String,
    oauth: bool,
    #[serde(default)]
    oauth_identity: Option<String>,
    #[serde(default)]
    discovered_limits: Option<metadata::DiscoveredLimits>,
}

impl StoredConfiguration {
    fn from_record(record: &AccountRecord) -> tembra::Result<Self> {
        serde_json::from_value(serde_json::Value::Object(
            record.private_configuration().clone(),
        ))
        .map_err(|_| failure("the provider account configuration is invalid"))
    }

    fn credential_requirement(&self, account: &crate::settings::Account) -> CredentialRequirement {
        if !self.oauth && self.provider == Provider::Ollama && account.provider_id == "ollama" {
            CredentialRequirement::None
        } else {
            CredentialRequirement::Account
        }
    }
}

pub use driver::AiAccountDriver;

fn failure(message: &str) -> Error {
    Error::new(Code::Unavailable, "providers.model_profile", message)
}

/// Native factory with an explicit protection policy. Construction performs no
/// I/O. The execution owner must retain the matching Context and physical scope.
pub struct AiModelFactory {
    protection: ModelProtection,
}

impl AiModelFactory {
    pub fn new(protection: ModelProtection) -> Self {
        Self { protection }
    }
}

impl ModelFactory for AiModelFactory {
    fn capture(
        &self,
        record: &AccountRecord,
        selection: &ModelSelection,
    ) -> tembra::Result<CapturedModel> {
        let account = &record.account;
        if account.id != selection.account_id {
            return Err(failure("the selected account does not match its capture"));
        }
        let selected = account
            .models
            .iter()
            .find(|model| model.id == selection.model_id)
            .ok_or_else(|| failure("the model is not configured for this account"))?;
        let reasoning = selection
            .reasoning
            .as_deref()
            .filter(|value| !value.is_empty())
            .map(str::to_ascii_lowercase);
        if reasoning.as_ref().is_some_and(|effort| {
            !selected
                .efforts
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(effort))
        }) {
            return Err(failure(
                "the reasoning effort is not supported by this model",
            ));
        }
        let saved = StoredConfiguration::from_record(record)?;
        if saved.oauth {
            return Err(failure(
                "the ChatGPT-plan authorization profile has not been qualified",
            ));
        }
        let model = ModelId::new(&selection.model_id)?;
        let mut connection = Connection::new(
            ConnectionId::new(&account.id)?,
            saved.provider,
            [model.clone()],
        );
        connection.endpoint = Some(saved.endpoint.clone());
        let credential = saved.credential_requirement(account);
        if credential == CredentialRequirement::Account {
            connection.credential_key = Some(account.id.clone());
        }
        connection.validate()?;
        let metadata =
            metadata::capture(&saved, account, &connection, &model, reasoning.as_deref())?;
        CapturedModel::new(
            metadata,
            credential,
            Box::new(Construction {
                connection,
                model,
                reasoning,
                protection: self.protection.clone(),
            }),
        )
    }
}

struct Construction {
    connection: Connection,
    model: ModelId,
    reasoning: Option<String>,
    protection: ModelProtection,
}

impl ModelConstruction for Construction {
    fn construct(
        self: Box<Self>,
        snapshot: Arc<Snapshot>,
        credential: Option<Zeroizing<String>>,
    ) -> tembra::Result<Rc<dyn Model>> {
        let credential = credential.unwrap_or_default();
        if self.connection.credential_key.is_some() {
            driver::validate_api_key(&credential)?;
        }
        let mut model = AiModel::new(
            self.connection,
            self.model,
            Rc::new(AccountCredential(credential)),
            self.protection,
        )?;
        if let Some(reasoning) = self.reasoning {
            model = model.with_reasoning(&reasoning)?;
        }
        Ok(Rc::new(model.with_metadata(snapshot)?))
    }
}

struct AccountCredential(Zeroizing<String>);

impl Credentials for AccountCredential {
    fn acquire<'a>(
        &'a self,
        context: &'a Context,
        _admitted: &'a crate::ai::AdmittedAttempt,
        _key: &'a str,
    ) -> LocalBoxFuture<'a, tembra::Result<crate::ai::ModelCredential>> {
        Box::pin(async move {
            context.check()?;
            crate::ai::ModelCredential::new(self.0.to_string(), "[secret]".into())
        })
    }
}
