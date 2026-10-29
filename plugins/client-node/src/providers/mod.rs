//! Native provider settings: public account metadata on disk, credentials in the OS keyring.
mod catalog;
mod models;
pub(crate) mod oauth;
mod secrets;

use futures::future::LocalBoxFuture;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{io::Write, path::PathBuf, sync::Arc};
use swarmie_model_provider_genai::{
    Connection, ConnectionId, Credentials, GenaiModel, ModelId, Provider,
};
use swarmie_providers::settings::{
    Account, ConfigureProvider, ModelDefaults, ProviderCatalog, ProviderSettings, SetModelDefaults,
};
use zeroize::Zeroizing;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedAccount {
    account: Account,
    provider: Provider,
    endpoint: String,
    oauth: bool,
    oauth_identity: Option<String>,
}

pub(crate) struct NativeProviders {
    path: PathBuf,
    node: String,
    principal: String,
    oauth_host: uuid::Uuid,
    renewal:
        tokio::sync::Mutex<std::collections::BTreeMap<String, swarmie_provider_auth::Credential>>,
    mutation: tokio::sync::Mutex<()>,
    secrets: Arc<dyn secrets::SecretStore>,
    #[cfg(test)]
    oauth_options: Option<(swarmie_provider_auth::ChatgptBrowserOptions, String)>,
}

pub(super) fn failure(message: &str) -> tembra::Error {
    tembra::Error::new(tembra::Code::Unavailable, "providers.settings", message)
}

impl NativeProviders {
    pub(crate) fn node_id(&self) -> String {
        self.node.clone()
    }
    pub(crate) fn new(
        path: PathBuf,
        node: String,
        principal: String,
        oauth_host: uuid::Uuid,
    ) -> Self {
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

    pub(crate) async fn model(
        &self,
        principal: &str,
        account_id: &str,
        model_id: &str,
        effort: &str,
    ) -> tembra::Result<GenaiModel> {
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
        connection.endpoint = Some(saved.endpoint);
        if saved.account.provider_id != "ollama" {
            connection.credential_key = Some(account_id.into());
        }
        let mut renewal = self.renewal.lock().await;
        let credentials = if saved.account.provider_id == "ollama" {
            None
        } else if saved.oauth {
            let mut credential = if let Some(pending) = renewal.get(account_id) {
                pending.clone()
            } else {
                let stored = secrets::get(self.secrets.clone(), account_id.into()).await?;
                swarmie_provider_auth::decode(&stored)
                    .map_err(|_| failure("the account credential is invalid"))?
                    .ok_or_else(|| failure("the account credential is invalid"))?
            };
            if renewal.contains_key(account_id) {
                let encoded = swarmie_provider_auth::encode(&credential)
                    .map_err(|_| failure("the renewed credential is invalid"))?;
                secrets::set(self.secrets.clone(), account_id.into(), encoded).await?;
                renewal.remove(account_id);
            }
            if credential.expires_at_ms <= swarmie_provider_auth::now_ms().saturating_add(60_000) {
                credential = self.refresh_credential(&credential).await.map_err(|_| {
                    failure("the account authorization could not be renewed; sign in again")
                })?;
                renewal.insert(account_id.into(), credential.clone());
                let encoded = swarmie_provider_auth::encode(&credential)
                    .map_err(|_| failure("the renewed credential is invalid"))?;
                secrets::set(self.secrets.clone(), account_id.into(), encoded).await?;
                renewal.remove(account_id);
            }
            Some(AccessToken(credential.access_token))
        } else {
            Some(AccessToken(
                secrets::get(self.secrets.clone(), account_id.into()).await?,
            ))
        };
        let empty = AccessToken(Zeroizing::new(String::new()));
        let model = GenaiModel::new(
            connection,
            ModelId::new(model_id)?,
            credentials.as_ref().unwrap_or(&empty),
        )?;
        if effort.is_empty() {
            Ok(model)
        } else {
            model.with_reasoning(&effort.to_ascii_lowercase())
        }
    }

    async fn refresh_credential(
        &self,
        credential: &swarmie_provider_auth::Credential,
    ) -> swarmie_provider_auth::Result<swarmie_provider_auth::Credential> {
        #[cfg(test)]
        if let Some((options, _)) = &self.oauth_options {
            return swarmie_provider_auth::ProviderClient::with_endpoints(
                swarmie_provider_auth::Endpoints {
                    chatgpt_token: options.browser.token_url.clone(),
                    chatgpt_jwks: options.jwks_url.clone(),
                    ..Default::default()
                },
            )?
            .refresh(credential)
            .await;
        }
        swarmie_provider_auth::refreshed(credential).await
    }

    fn oauth_identity(identity: &swarmie_provider_auth::VerifiedIdentity) -> String {
        format!(
            "{:x}",
            Sha256::digest(format!("{}\0{}", identity.issuer, identity.subject).as_bytes())
        )
    }

    fn decode_oauth(
        &self,
        stored: &str,
        expected_identity: Option<&str>,
    ) -> Option<swarmie_provider_auth::Credential> {
        let credential = swarmie_provider_auth::decode(stored).ok().flatten()?;
        if credential.provider != swarmie_provider_auth::Provider::Chatgpt {
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
    ) -> tembra::Result<Option<swarmie_provider_auth::Credential>> {
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
    ) -> tembra::Result<swarmie_provider_auth::BrowserLogin> {
        let credential = match returning {
            Some(id) => self.returning_credential(id).await?,
            None => None,
        };
        #[cfg(test)]
        if let Some((options, _)) = &self.oauth_options {
            return swarmie_provider_auth::begin_chatgpt_browser_with_options(
                self.oauth_host,
                credential.as_ref(),
                options.clone(),
            )
            .await
            .map_err(|_| {
                failure("provider authorization could not start; check the local callback listener")
            });
        }
        swarmie_provider_auth::begin_chatgpt_browser(self.oauth_host, credential.as_ref())
            .await
            .map_err(|_| {
                failure("provider authorization could not start; check the local callback listener")
            })
    }

    async fn save_oauth(
        &self,
        context: &tembra::Context,
        principal: &str,
        credential: &swarmie_provider_auth::Credential,
        cancelled: &std::cell::Cell<bool>,
    ) -> tembra::Result<Account> {
        self.authorize(principal)?;
        let endpoint = &credential.endpoint;
        #[cfg(test)]
        let endpoint = self
            .oauth_options
            .as_ref()
            .map_or(endpoint, |(_, endpoint)| endpoint);
        let models = models::oauth_models(credential, endpoint).await?;
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
            models,
            available: true,
            models_truncated: false,
        };
        let encoded = swarmie_provider_auth::encode(credential)
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

struct AccessToken(Zeroizing<String>);
impl Credentials for AccessToken {
    fn resolve(&self, _key: &str) -> Option<String> {
        Some(self.0.to_string())
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
                models,
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
    use swarmie_providers::settings::ModelInfo;

    fn account(id: &str, count: usize) -> SavedAccount {
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
