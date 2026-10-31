//! Native provider settings: public account metadata on disk, credentials in the OS keyring.
mod catalog;
mod metadata;
mod models;
pub(crate) mod oauth;
mod qualifications;
mod secrets;

use crate::ai::{AiModel, Connection, ConnectionId, Credentials, ModelId, Provider};
use crate::settings::{
    Account, ConfigureProvider, ModelDefaults, ProviderCatalog, ProviderSettings, SetModelDefaults,
};
use futures::future::LocalBoxFuture;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{io::Write, path::PathBuf, rc::Rc, sync::Arc};
use zeroize::Zeroizing;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedAccount {
    account: Account,
    provider: Provider,
    endpoint: String,
    oauth: bool,
    oauth_identity: Option<String>,
    #[serde(default)]
    discovered_limits: Option<metadata::DiscoveredLimits>,
}

/// Principal-bound native account configuration and model construction.
/// Public metadata uses the host-selected file; credentials remain in native custody.
pub struct NativeProviders {
    path: PathBuf,
    node: String,
    principal: String,
    oauth_host: uuid::Uuid,
    renewal: tokio::sync::Mutex<std::collections::BTreeMap<String, crate::auth::Credential>>,
    mutation: tokio::sync::Mutex<()>,
    secrets: Arc<dyn secrets::SecretStore>,
    metadata: metadata::ModelMetadata,
    #[cfg(test)]
    oauth_options: Option<(crate::auth::ChatgptBrowserOptions, String)>,
}

pub(super) fn failure(message: &str) -> tembra::Error {
    tembra::Error::new(tembra::Code::Unavailable, "providers.settings", message)
}

impl NativeProviders {
    /// Declare the provider settings and authorization services for this bound owner.
    pub fn orbs(self: Rc<Self>) -> Vec<tembra::Orb> {
        vec![
            crate::settings::settings_orb(self.clone()),
            oauth::orb(self),
        ]
    }

    pub fn node_id(&self) -> String {
        self.node.clone()
    }

    /// Bind settings to a trusted host identity, principal and profile file.
    /// Construction does not authenticate a transport or grant filesystem access.
    pub fn new(path: PathBuf, node: String, principal: String, oauth_host: uuid::Uuid) -> Self {
        let secrets = Arc::new(secrets::ChunkedStore::new(Arc::new(
            secrets::KeyringStore {
                namespace: format!("{node}/{principal}"),
            },
        )));
        Self {
            path,
            node,
            principal,
            oauth_host,
            renewal: tokio::sync::Mutex::new(Default::default()),
            mutation: tokio::sync::Mutex::new(()),
            secrets,
            metadata: metadata::ModelMetadata::default(),
            #[cfg(test)]
            oauth_options: None,
        }
    }

    fn authorize(&self, principal: &str) -> tembra::Result<()> {
        if principal != self.principal {
            return Err(tembra::Error::new(
                tembra::Code::Forbidden,
                "providers.principal",
                "provider access refused",
            ));
        }
        Ok(())
    }

    async fn read(&self) -> tembra::Result<Vec<SavedAccount>> {
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| {
                failure("provider settings could not be read; existing data was preserved")
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
            Err(_) => Err(failure(
                "provider settings could not be read; existing data was preserved",
            )),
        })
        .await
        .map_err(|_| failure("provider settings could not be read"))?
    }

    async fn save(&self, accounts: &[SavedAccount]) -> tembra::Result<()> {
        let path = self.path.clone();
        let accounts = accounts.to_vec();
        tokio::task::spawn_blocking(move || {
            let parent = path
                .parent()
                .ok_or_else(|| failure("provider settings path is unavailable"))?;
            let mut temporary = tempfile::NamedTempFile::new_in(parent)
                .map_err(|_| failure("provider settings could not be saved"))?;
            serde_json::to_writer(&mut temporary, &accounts)
                .map_err(|_| failure("provider settings could not be encoded"))?;
            temporary
                .flush()
                .and_then(|()| temporary.as_file().sync_all())
                .map_err(|_| failure("provider settings could not be saved"))?;
            temporary
                .persist(&path)
                .map_err(|_| failure("provider settings could not be saved"))?;
            Ok(())
        })
        .await
        .map_err(|_| failure("provider settings could not be saved"))?
    }

    /// Construct a configured model for the principal and retained protection owner.
    /// Missing accounts, models, metadata or authorization refuse construction.
    pub async fn model_with_protection(
        &self,
        principal: &str,
        account_id: &str,
        model_id: &str,
        effort: &str,
        protection: crate::ai::ModelProtection,
    ) -> tembra::Result<AiModel> {
        self.authorize(principal)?;
        let saved = self
            .read()
            .await?
            .into_iter()
            .find(|saved| saved.account.id == account_id)
            .ok_or_else(|| failure("the provider account is not configured"))?;
        if !saved
            .account
            .models
            .iter()
            .any(|model| model.id == model_id)
        {
            return Err(failure("the model is not configured for this account"));
        }
        if !effort.is_empty()
            && !saved
                .account
                .models
                .iter()
                .find(|model| model.id == model_id)
                .is_some_and(|model| {
                    model
                        .efforts
                        .iter()
                        .any(|allowed| allowed.eq_ignore_ascii_case(effort))
                })
        {
            return Err(failure(
                "the reasoning effort is not supported by this model",
            ));
        }
        let mut connection = Connection::new(
            ConnectionId::new(account_id)?,
            saved.provider,
            [ModelId::new(model_id)?],
        );
        connection.endpoint = Some(saved.endpoint.clone());
        if saved.account.provider_id != "ollama" {
            connection.credential_key = Some(account_id.into());
        }
        let snapshot = self.metadata.prepare(
            &saved,
            &connection,
            &ModelId::new(model_id)?,
            effort,
            crate::auth::now_ms(),
        )?;
        if saved.oauth {
            return Err(failure("the ChatGPT inference profile is not qualified"));
        }
        let model = AiModel::new(
            connection,
            ModelId::new(model_id)?,
            Rc::new(LazyCredential {
                store: self.secrets.clone(),
                account: account_id.into(),
            }),
            protection,
        )?;
        let model = if effort.is_empty() {
            model
        } else {
            model.with_reasoning(&effort.to_ascii_lowercase())?
        };
        model.with_metadata(snapshot)
    }

    #[cfg(test)]
    async fn model(
        &self,
        principal: &str,
        account: &str,
        model: &str,
        effort: &str,
    ) -> tembra::Result<AiModel> {
        self.model_with_protection(
            principal,
            account,
            model,
            effort,
            crate::ai::ModelProtection::Unprotected,
        )
        .await
    }

    fn oauth_identity(identity: &crate::auth::VerifiedIdentity) -> String {
        format!(
            "{:x}",
            Sha256::digest(format!("{}\0{}", identity.issuer, identity.subject).as_bytes())
        )
    }

    fn decode_oauth(
        &self,
        stored: &str,
        expected_identity: Option<&str>,
    ) -> Option<crate::auth::Credential> {
        let credential = crate::auth::decode(stored).ok().flatten()?;
        if credential.provider != crate::auth::Provider::Chatgpt {
            return None;
        }
        let registration = credential.chatgpt.as_ref()?;
        if registration.host_id != format!("urn:uuid:{}", self.oauth_host)
            || expected_identity
                .is_some_and(|identity| identity != Self::oauth_identity(&registration.identity))
        {
            return None;
        }
        Some(credential)
    }

    async fn returning_credential(
        &self,
        id: &str,
    ) -> tembra::Result<Option<crate::auth::Credential>> {
        let saved = self
            .read()
            .await?
            .into_iter()
            .find(|saved| saved.account.id == id && saved.oauth)
            .ok_or_else(|| failure("the provider account is not configured for authorization"))?;
        let Ok(stored) = secrets::get(self.secrets.clone(), saved.account.id).await else {
            return Ok(None);
        };
        Ok(self.decode_oauth(&stored, saved.oauth_identity.as_deref()))
    }

    async fn begin_authorization(
        &self,
        returning: Option<&str>,
    ) -> tembra::Result<crate::auth::BrowserLogin> {
        let credential = match returning {
            Some(id) => self.returning_credential(id).await?,
            None => None,
        };
        #[cfg(test)]
        if let Some((options, _)) = &self.oauth_options {
            return crate::auth::begin_chatgpt_browser_with_options(
                self.oauth_host,
                credential.as_ref(),
                options.clone(),
            )
            .await
            .map_err(|_| {
                failure("provider authorization could not start; check the local callback listener")
            });
        }
        crate::auth::begin_chatgpt_browser(self.oauth_host, credential.as_ref())
            .await
            .map_err(|_| {
                failure("provider authorization could not start; check the local callback listener")
            })
    }

    async fn save_oauth(
        &self,
        context: &tembra::Context,
        principal: &str,
        credential: &crate::auth::Credential,
        cancelled: &std::cell::Cell<bool>,
    ) -> tembra::Result<Account> {
        self.authorize(principal)?;
        let endpoint = &credential.endpoint;
        #[cfg(test)]
        let endpoint = self
            .oauth_options
            .as_ref()
            .map_or(endpoint, |(_, endpoint)| endpoint);
        let mut models = models::oauth_models(credential, endpoint).await?;
        models.limits.endpoint = credential.endpoint.clone();
        context.check()?;
        if cancelled.get() {
            return Err(failure("provider authorization was cancelled"));
        }
        let registration = credential
            .chatgpt
            .as_ref()
            .ok_or_else(|| failure("the ChatGPT registration is unavailable"))?;
        let label = models::public_label(
            registration.identity.email.as_deref().unwrap_or(""),
            "ChatGPT account",
        );
        let identity = Self::oauth_identity(&registration.identity);
        let mut renewal = self.renewal.lock().await;
        let _mutation = self.mutation.lock().await;
        context.check()?;
        let mut accounts = self.read().await?;
        let existing = accounts
            .iter()
            .find(|saved| saved.oauth_identity.as_deref() == Some(&identity));
        let account = Account {
            id: existing.map_or_else(
                || uuid::Uuid::new_v4().to_string(),
                |saved| saved.account.id.clone(),
            ),
            provider_id: "openai".into(),
            provider_label: "OpenAI".into(),
            defaults: existing.map_or_else(ModelDefaults::default, |saved| {
                saved.account.defaults.clone()
            }),
            authorization_method: Some("oauth-browser".into()),
            label,
            models: models.models,
            available: true,
            models_truncated: false,
        };
        let encoded = crate::auth::encode(credential)
            .map_err(|_| failure("the authorized credential is invalid"))?;
        context.check()?;
        let previous = if existing.is_some() {
            match secrets::get(self.secrets.clone(), account.id.clone()).await {
                Ok(stored) => self.decode_oauth(&stored, Some(&identity)).map(|_| stored),
                Err(error) if secrets::has_no_restore_point(&error) => None,
                Err(error) => return Err(error),
            }
        } else {
            None
        };
        context.check()?;
        if cancelled.get() {
            return Err(failure("provider authorization was cancelled"));
        }
        secrets::set(self.secrets.clone(), account.id.clone(), encoded).await?;
        accounts.retain(|saved| saved.account.id != account.id);
        accounts.push(SavedAccount {
            account: account.clone(),
            provider: Provider::OpenAiResponses,
            endpoint: credential.endpoint.clone(),
            oauth: true,
            oauth_identity: Some(identity),
            discovered_limits: Some(models.limits),
        });
        if let Err(error) = self.save(&accounts).await {
            if let Some(previous) = previous {
                let _ = secrets::set(self.secrets.clone(), account.id.clone(), previous).await;
            } else {
                let _ = secrets::delete(self.secrets.clone(), account.id.clone()).await;
            }
            return Err(error);
        }

        renewal.remove(&account.id);
        Ok(first_chunk(account))
    }
}

struct LazyCredential {
    store: Arc<dyn secrets::SecretStore>,
    account: String,
}
impl Credentials for LazyCredential {
    fn acquire<'a>(
        &'a self,
        context: &'a tembra::Context,
        _admitted: &'a crate::ai::AdmittedAttempt,
        key: &'a str,
    ) -> LocalBoxFuture<'a, tembra::Result<crate::ai::ModelCredential>> {
        Box::pin(async move {
            context.check()?;
            if key != self.account {
                return Err(failure("the credential belongs to another account"));
            }
            let secret = secrets::get(self.store.clone(), self.account.clone()).await?;
            context.check()?;
            crate::ai::ModelCredential::new(secret.to_string(), "[secret]".into())
        })
    }
}

impl ProviderSettings for NativeProviders {
    fn set_defaults(
        &self,
        principal: String,
        input: SetModelDefaults,
    ) -> LocalBoxFuture<'_, tembra::Result<ModelDefaults>> {
        Box::pin(async move {
            self.authorize(&principal)?;
            let _mutation = self.mutation.lock().await;
            let mut accounts = self.read().await?;
            let saved = accounts
                .iter_mut()
                .find(|saved| saved.account.id == input.account_id)
                .ok_or_else(|| failure("the provider account is not configured"))?;
            input.defaults.validate(&saved.account.models)?;
            saved.account.defaults = input.defaults.clone();
            self.save(&accounts).await?;
            Ok(input.defaults)
        })
    }

    fn list(
        &self,
        principal: String,
        cursor: Option<String>,
    ) -> LocalBoxFuture<'_, tembra::Result<ProviderCatalog>> {
        Box::pin(async move {
            self.authorize(&principal)?;
            let saved = self.read().await?;
            let revision = format!(
                "{:x}",
                Sha256::digest(
                    serde_json::to_vec(&saved)
                        .map_err(|_| failure("provider settings could not be encoded"))?
                )
            );
            let offset = match cursor {
                None => 0,
                Some(cursor) => {
                    let (version, offset) = cursor
                        .split_once(':')
                        .ok_or_else(|| failure("invalid provider catalog cursor"))?;
                    if version != revision {
                        return Err(tembra::Error::new(
                            tembra::Code::Conflict,
                            "providers.catalog_changed",
                            "the provider catalog changed; load it again",
                        ));
                    }
                    offset
                        .parse::<usize>()
                        .map_err(|_| failure("invalid provider catalog cursor"))?
                }
            };
            let chunks: Vec<_> = saved
                .into_iter()
                .flat_map(|saved| account_chunks(saved.account))
                .collect();
            if offset > chunks.len() {
                return Err(failure("invalid provider catalog cursor"));
            }
            let mut page = ProviderCatalog {
                choices: catalog::choices(),
                accounts: Vec::new(),
                next_cursor: None,
            };
            for mut account in chunks.iter().skip(offset).cloned() {
                account.available = account.provider_id == "ollama"
                    || secrets::get(self.secrets.clone(), account.id.clone())
                        .await
                        .is_ok();
                page.accounts.push(account);
                if serde_json::to_vec(&page)
                    .map_err(|_| failure("provider catalog could not be encoded"))?
                    .len()
                    > 40 * 1024
                {
                    page.accounts.pop();
                    break;
                }
            }
            let next = offset + page.accounts.len();
            if next < chunks.len() {
                page.next_cursor = Some(format!("{revision}:{next}"));
            }
            Ok(page)
        })
    }

    fn configure(
        &self,
        principal: String,
        input: ConfigureProvider,
    ) -> LocalBoxFuture<'_, tembra::Result<Account>> {
        Box::pin(async move {
            self.authorize(&principal)?;
            let choice = catalog::choices()
                .into_iter()
                .find(|choice| choice.id == input.provider_id)
                .ok_or_else(|| failure("this provider is not available for configuration"))?;
            let spec = catalog::specifications()
                .into_iter()
                .find(|spec| spec.id == input.provider_id)
                .ok_or_else(|| failure("this provider is not supported"))?;
            let local = spec.id == "ollama";
            if input.method != if local { "local" } else { "api-key" } {
                return Err(failure("the connection method is not supported"));
            }
            let credential = Zeroizing::new(input.credential.unwrap_or_default());
            if !local
                && (credential.trim().is_empty()
                    || credential.len() > 8192
                    || credential.chars().any(char::is_control))
            {
                return Err(failure("provide a valid API key"));
            }
            let endpoint = if local {
                input.url.as_deref().unwrap_or(spec.endpoint)
            } else {
                spec.endpoint
            };
            let url =
                reqwest::Url::parse(endpoint).map_err(|_| failure("provide a valid server URL"))?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(failure(
                    "use an HTTP server URL without credentials, query or fragment",
                ));
            }
            let models =
                models::list(&spec, endpoint, (!local).then_some(credential.as_str())).await?;
            let account = Account {
                id: uuid::Uuid::new_v4().to_string(),
                defaults: ModelDefaults::default(),
                provider_id: spec.id.into(),
                provider_label: choice.label.clone(),
                authorization_method: Some(if local { "local" } else { "api-key" }.into()),
                label: format!("{} account", choice.label),
                models: models.models,
                available: true,
                models_truncated: false,
            };
            let _mutation = self.mutation.lock().await;
            let mut accounts = self.read().await?;
            if !local {
                secrets::set(self.secrets.clone(), account.id.clone(), credential).await?;
            }
            accounts.push(SavedAccount {
                account: account.clone(),
                provider: spec.provider,
                endpoint: endpoint.into(),
                oauth: false,
                oauth_identity: None,
                discovered_limits: Some(models.limits),
            });
            if let Err(error) = self.save(&accounts).await {
                if !local {
                    let _ = secrets::delete(self.secrets.clone(), account.id.clone()).await;
                }
                return Err(error);
            }
            Ok(first_chunk(account))
        })
    }
}

/// RPC account chunks fit both state and method-result frame budgets.
fn account_chunks(mut account: Account) -> Vec<Account> {
    let models = std::mem::take(&mut account.models);
    let mut chunks = Vec::new();
    let mut chunk = account;
    let base = serde_json::to_vec(&chunk).map_or(4096, |bytes| bytes.len()) + 64;
    let mut size = base;
    for model in models {
        let added = serde_json::to_vec(&model).map_or(2048, |bytes| bytes.len()) + 1;
        if !chunk.models.is_empty() && (chunk.models.len() >= 256 || size + added > 20 * 1024) {
            let models = std::mem::take(&mut chunk.models);
            chunks.push(Account {
                models,
                models_truncated: true,
                ..chunk.clone()
            });
            size = base;
        }
        size += added;
        chunk.models.push(model);
    }
    chunk.models_truncated = !chunks.is_empty();
    chunks.push(chunk);
    chunks
}

fn first_chunk(account: Account) -> Account {
    account_chunks(account)
        .into_iter()
        .next()
        .expect("each account has a chunk")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::ModelInfo;

    pub(super) fn account(id: &str, count: usize) -> SavedAccount {
        SavedAccount {
            account: Account {
                id: id.into(),
                defaults: ModelDefaults::default(),
                provider_id: "ollama".into(),
                provider_label: "Ollama".into(),
                authorization_method: Some("local".into()),
                label: "Local account".into(),
                models: (0..count)
                    .map(|index| ModelInfo {
                        id: format!("model-{index}"),
                        label: "m".repeat(512),
                        efforts: vec![],
                    })
                    .collect(),
                available: true,
                models_truncated: false,
            },
            provider: Provider::Ollama,
            endpoint: "http://localhost:11434".into(),
            oauth: false,
            oauth_identity: None,
            discovered_limits: None,
        }
    }

    #[tokio::test]
    async fn provider_catalog_pages_every_model_within_rpc_budgets_and_rejects_stale_cursors() {
        let directory = tempfile::tempdir().unwrap();
        let source = NativeProviders::new(
            directory.path().join("providers.json"),
            "fixture-node".into(),
            "fixture".into(),
            uuid::Uuid::new_v4(),
        );
        let accounts = vec![
            account("first", 700),
            account("second", 700),
            account("third", 700),
        ];
        source.save(&accounts).await.unwrap();
        let first = source.list("fixture".into(), None).await.unwrap();
        let stale = first.next_cursor.clone().unwrap();
        let mut cursor = None;
        let mut collected =
            std::collections::BTreeMap::<String, std::collections::BTreeSet<String>>::new();
        loop {
            let page = source.list("fixture".into(), cursor).await.unwrap();
            assert!(serde_json::to_vec(&page).unwrap().len() <= 40 * 1024 + 128);
            for account in page.accounts {
                assert!(account.models.len() <= 256);
                assert!(account.models_truncated);
                for model in account.models {
                    assert!(
                        collected
                            .entry(account.id.clone())
                            .or_default()
                            .insert(model.id)
                    );
                }
            }
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(collected.len(), 3);
        assert!(
            collected.values().all(|models| models.len() == 700),
            "paging must not silently discard models"
        );
        let mut updated = accounts;
        updated.push(account("new", 1));
        source.save(&updated).await.unwrap();
        assert!(source.list("fixture".into(), Some(stale)).await.is_err());
        assert!(
            serde_json::to_vec(&first_chunk(updated.remove(0).account))
                .unwrap()
                .len()
                <= 20 * 1024
        );
    }

    #[test]
    fn public_labels_are_nonblank_and_use_the_browser_utf8_budget() {
        assert_eq!(models::public_label("  ", "model-id"), "model-id");
        assert_eq!(models::public_label("\u{feff}", "model-id"), "model-id");
        assert_eq!(
            models::public_label("\u{feff} \u{00a0}Model\u{feff}", "model-id"),
            "Model"
        );
        assert_eq!(models::public_label("bad\nlabel", "model-id"), "model-id");
        let label = models::public_label(&"🙂".repeat(200), "model-id");
        assert_eq!(label.len(), 512);
        assert_eq!(label.chars().count(), 128);
    }

    #[tokio::test]
    async fn model_defaults_persist_and_invalid_or_foreign_updates_preserve_the_saved_account() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("providers.json");
        let source = NativeProviders::new(
            path.clone(),
            "fixture-node".into(),
            "fixture".into(),
            uuid::Uuid::new_v4(),
        );
        let mut saved = account("primary", 2);
        saved.account.models[1].efforts = vec!["balanced".into(), "deep".into()];
        source.save(&[saved]).await.unwrap();
        let defaults = ModelDefaults {
            model: Some("model-1".into()),
            effort: Some("deep".into()),
            auxiliary_model: Some("model-0".into()),
        };
        let input = SetModelDefaults {
            account_id: "primary".into(),
            defaults: defaults.clone(),
        };
        assert_eq!(
            source
                .set_defaults("fixture".into(), input.clone())
                .await
                .unwrap(),
            defaults
        );
        let bytes = std::fs::read(&path).unwrap();
        assert!(
            source
                .set_defaults("other".into(), input.clone())
                .await
                .is_err()
        );
        for invalid in [
            ModelDefaults {
                model: Some("missing".into()),
                ..defaults.clone()
            },
            ModelDefaults {
                effort: Some("high".into()),
                ..defaults.clone()
            },
            ModelDefaults {
                auxiliary_model: Some("missing".into()),
                ..defaults.clone()
            },
        ] {
            assert!(
                source
                    .set_defaults(
                        "fixture".into(),
                        SetModelDefaults {
                            account_id: "primary".into(),
                            defaults: invalid
                        }
                    )
                    .await
                    .is_err()
            );
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        let reopened = NativeProviders::new(
            path,
            "fixture-node".into(),
            "fixture".into(),
            uuid::Uuid::new_v4(),
        );
        let page = reopened.list("fixture".into(), None).await.unwrap();
        assert_eq!(page.accounts[0].defaults, defaults);
        assert_eq!(page.accounts[0].models.len(), 2);
        reopened
            .set_defaults(
                "fixture".into(),
                SetModelDefaults {
                    account_id: "primary".into(),
                    defaults: ModelDefaults::default(),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            reopened.read().await.unwrap()[0].account.defaults,
            ModelDefaults::default()
        );
    }
}
