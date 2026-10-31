//! Concrete discovery and authorization protocols. Account association, custody,
//! persistence, pending results and cancellation fences belong to Accounts.

mod recovery;

use super::{StoredConfiguration, catalog, discovery};
use crate::Provider;
use futures::future::LocalBoxFuture;
use sha2::{Digest, Sha256};
use swarmie_provider_auth::{BrowserLogin, ChatgptBrowserOptions, Credential};
use swarmie_providers::{
    AccountRecord, CredentialRequirement,
    accounts::{AccountCandidate, AccountDriver, AuthorizationFlow, AuthorizationInstructions},
    settings::{Account, ConfigureProvider, ModelDefaults},
};
use tembra::{Code, Error};
use uuid::Uuid;
use zeroize::Zeroizing;

pub(super) fn failure(message: &str) -> Error {
    Error::new(Code::Unavailable, "providers.settings", message)
}

fn invalid_credential() -> Error {
    Error::new(
        Code::Invalid,
        "providers.credential_invalid",
        "the account credential is invalid",
    )
}

pub(super) fn validate_api_key(value: &str) -> tembra::Result<()> {
    if value.trim().is_empty()
        || value.len() > 8192
        || value.chars().any(char::is_control)
        || !matches!(swarmie_provider_auth::decode(value), Ok(None))
    {
        return Err(invalid_credential());
    }
    Ok(())
}

fn association(credential: &Credential) -> tembra::Result<String> {
    let identity = &credential
        .chatgpt
        .as_ref()
        .ok_or_else(invalid_credential)?
        .identity;
    Ok(format!(
        "{:x}",
        Sha256::digest(format!("{}\0{}", identity.issuer, identity.subject).as_bytes())
    ))
}

fn record(account: Account, configuration: StoredConfiguration) -> tembra::Result<AccountRecord> {
    let private = serde_json::to_value(configuration)
        .map_err(|_| failure("the provider configuration could not be encoded"))?;
    let serde_json::Value::Object(private) = private else {
        return Err(failure("the provider configuration is invalid"));
    };
    AccountRecord::new(account, private)
}

/// Concrete protocols for the existing product choices. The registration scope
/// is the already persisted native UUID. Construction has no filesystem, custody
/// or network I/O. This driver contains no account store or renewal cache.
pub struct AiAccountDriver {
    registration_scope: Uuid,
    chatgpt: ChatgptBrowserOptions,
    catalog_endpoint: Option<String>,
}

impl AiAccountDriver {
    pub fn new(registration_scope: Uuid) -> tembra::Result<Self> {
        Self::with_options(registration_scope, ChatgptBrowserOptions::default(), None)
    }

    /// Trusted native protocol options, following provider-auth's existing fixture
    /// seam. A catalog override is limited to loopback fixtures and never grants
    /// production metadata qualification. These options are not RPC inputs.
    pub fn with_options(
        registration_scope: Uuid,
        chatgpt: ChatgptBrowserOptions,
        catalog_endpoint: Option<String>,
    ) -> tembra::Result<Self> {
        if registration_scope.is_nil() {
            return Err(Error::invalid(
                "providers.registration",
                "a persisted registration scope is required",
            ));
        }
        if let Some(endpoint) = &catalog_endpoint {
            let parsed = reqwest::Url::parse(endpoint)
                .map_err(|_| failure("the fixture catalog endpoint is invalid"))?;
            let loopback = parsed.host_str().is_some_and(|host| {
                host == "localhost"
                    || host
                        .trim_matches(['[', ']'])
                        .parse::<std::net::IpAddr>()
                        .is_ok_and(|ip| ip.is_loopback())
            });
            if !loopback
                || !matches!(parsed.scheme(), "http" | "https")
                || !parsed.username().is_empty()
                || parsed.password().is_some()
                || parsed.query().is_some()
                || parsed.fragment().is_some()
            {
                return Err(failure(
                    "the fixture catalog endpoint must be an HTTP loopback URL",
                ));
            }
        }
        Ok(Self {
            registration_scope,
            chatgpt,
            catalog_endpoint,
        })
    }

    fn decode(&self, encoded: &str) -> tembra::Result<Credential> {
        let credential = swarmie_provider_auth::decode(encoded)
            .map_err(|_| invalid_credential())?
            .ok_or_else(invalid_credential)?;
        if credential.provider != swarmie_provider_auth::Provider::Chatgpt
            || credential.chatgpt.as_ref().is_none_or(|registration| {
                registration.host_id != format!("urn:uuid:{}", self.registration_scope)
            })
        {
            return Err(invalid_credential());
        }
        Ok(credential)
    }

    fn credential(
        &self,
        account: &AccountRecord,
        encoded: &str,
    ) -> tembra::Result<Option<Credential>> {
        let saved = StoredConfiguration::from_record(account)?;
        if saved.credential_requirement(&account.account) == CredentialRequirement::None {
            return Err(invalid_credential());
        }
        if !saved.oauth {
            validate_api_key(encoded)?;
            return Ok(None);
        }
        let credential = self.decode(encoded)?;
        if account.account.provider_id != "openai"
            || saved.provider != Provider::OpenAiResponses
            || saved.endpoint.trim_end_matches('/') != credential.endpoint.trim_end_matches('/')
            || saved.oauth_identity.as_deref() != Some(association(&credential)?.as_str())
        {
            return Err(invalid_credential());
        }
        Ok(Some(credential))
    }
}

impl AccountDriver for AiAccountDriver {
    fn protocol_binding(&self) -> tembra::Result<swarmie_providers::ProtocolBinding> {
        AiAccountDriver::protocol_binding(self)
    }

    fn needs_renewal(
        &self,
        account: &AccountRecord,
        credential: &Zeroizing<String>,
        now_ms: u64,
    ) -> tembra::Result<bool> {
        AiAccountDriver::needs_renewal(self, account, credential, now_ms)
    }

    fn configure(
        &self,
        input: ConfigureProvider,
    ) -> LocalBoxFuture<'_, tembra::Result<AccountCandidate>> {
        Box::pin(async move {
            let credential = Zeroizing::new(input.credential.unwrap_or_default());
            let choice = swarmie_providers::settings::provider_catalog()
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
            if !local {
                validate_api_key(&credential)?;
            }
            let endpoint = if local {
                input.url.as_deref().unwrap_or(spec.endpoint)
            } else {
                spec.endpoint
            };
            let parsed =
                reqwest::Url::parse(endpoint).map_err(|_| failure("provide a valid server URL"))?;
            if !matches!(parsed.scheme(), "http" | "https")
                || parsed.host_str().is_none()
                || !parsed.username().is_empty()
                || parsed.password().is_some()
                || parsed.query().is_some()
                || parsed.fragment().is_some()
            {
                return Err(failure(
                    "use an HTTP server URL without credentials, query or fragment",
                ));
            }
            let models =
                discovery::list(&spec, endpoint, (!local).then_some(credential.as_str())).await?;
            Ok(AccountCandidate {
                record: record(
                    Account {
                        id: String::new(),
                        provider_id: spec.id.into(),
                        provider_label: choice.label.clone(),
                        label: format!("{} account", choice.label),
                        models: models.models,
                        available: true,
                        defaults: ModelDefaults::default(),
                        authorization_method: Some(if local { "local" } else { "api-key" }.into()),
                        models_truncated: false,
                    },
                    StoredConfiguration {
                        provider: spec.provider,
                        endpoint: endpoint.into(),
                        oauth: false,
                        oauth_identity: None,
                        discovered_limits: Some(models.limits),
                    },
                )?,
                credential: (!local).then_some(credential),
                association_key: None,
            })
        })
    }

    fn begin_authorization<'a>(
        &'a self,
        provider: &'a str,
        returning: Option<&'a Zeroizing<String>>,
    ) -> LocalBoxFuture<'a, tembra::Result<Box<dyn AuthorizationFlow>>> {
        Box::pin(async move {
            if provider != "openai" {
                return Err(failure(
                    "this provider does not support browser authorization",
                ));
            }
            let returning = returning.map(|encoded| self.decode(encoded)).transpose()?;
            let login = swarmie_provider_auth::begin_chatgpt_browser_with_options(
                self.registration_scope,
                returning.as_ref(),
                self.chatgpt.clone(),
            )
            .await
            .map_err(|error| failure(&error.to_string()))?;
            Ok(Box::new(Flow(login)) as Box<dyn AuthorizationFlow>)
        })
    }

    fn accept_authorization<'a>(
        &'a self,
        provider: &'a str,
        encoded: &'a Zeroizing<String>,
    ) -> LocalBoxFuture<'a, tembra::Result<AccountCandidate>> {
        Box::pin(async move {
            if provider != "openai" {
                return Err(failure(
                    "this provider does not support browser authorization",
                ));
            }
            let credential = self.decode(encoded)?;
            let endpoint = self
                .catalog_endpoint
                .as_deref()
                .unwrap_or(&credential.endpoint);
            let models = discovery::oauth_models(&credential, endpoint).await?;
            let association = association(&credential)?;
            let registration = credential.chatgpt.as_ref().ok_or_else(invalid_credential)?;
            Ok(AccountCandidate {
                record: record(
                    Account {
                        id: String::new(),
                        provider_id: "openai".into(),
                        provider_label: "OpenAI".into(),
                        label: discovery::public_label(
                            registration.identity.email.as_deref().unwrap_or(""),
                            "ChatGPT account",
                        ),
                        models: models.models,
                        available: true,
                        defaults: ModelDefaults::default(),
                        authorization_method: Some("oauth-browser".into()),
                        models_truncated: false,
                    },
                    StoredConfiguration {
                        provider: Provider::OpenAiResponses,
                        endpoint: credential.endpoint,
                        oauth: true,
                        oauth_identity: Some(association.clone()),
                        discovered_limits: Some(models.limits),
                    },
                )?,
                credential: Some(encoded.clone()),
                association_key: Some(association),
            })
        })
    }

    fn renew<'a>(
        &'a self,
        account: &'a AccountRecord,
        encoded: &'a Zeroizing<String>,
        now_ms: u64,
    ) -> LocalBoxFuture<'a, tembra::Result<Option<Zeroizing<String>>>> {
        Box::pin(async move {
            let Some(credential) = self.renewal_credential(account, encoded, now_ms)? else {
                return Ok(None);
            };
            let client = swarmie_provider_auth::ProviderClient::with_endpoints(
                swarmie_provider_auth::Endpoints {
                    chatgpt_token: self.chatgpt.browser.token_url.clone(),
                    chatgpt_jwks: self.chatgpt.jwks_url.clone(),
                    ..Default::default()
                },
            )
            .map_err(|error| failure(&error.to_string()))?;
            let replacement = client.refresh(&credential).await.map_err(|_| {
                failure("the account authorization could not be renewed; sign in again")
            })?;
            let encoded =
                swarmie_provider_auth::encode(&replacement).map_err(|_| invalid_credential())?;
            self.credential(account, &encoded)?;
            Ok(Some(encoded))
        })
    }

    fn validate_credential(&self, account: &AccountRecord, credential: &str) -> tembra::Result<()> {
        self.credential(account, credential).map(|_| ())
    }

    fn association_key(&self, account: &AccountRecord) -> tembra::Result<Option<String>> {
        let saved = StoredConfiguration::from_record(account)?;
        if !saved.oauth {
            return Ok(None);
        }
        if account.account.provider_id != "openai" || saved.provider != Provider::OpenAiResponses {
            return Err(failure("the account registration metadata is invalid"));
        }
        let key = saved
            .oauth_identity
            .filter(|key| key.len() == 64 && key.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .ok_or_else(|| failure("the account registration metadata is invalid"))?;
        Ok(Some(key))
    }

    fn credential_requirement(
        &self,
        account: &AccountRecord,
    ) -> tembra::Result<CredentialRequirement> {
        Ok(StoredConfiguration::from_record(account)?.credential_requirement(&account.account))
    }
}

struct Flow(BrowserLogin);

impl AuthorizationFlow for Flow {
    fn instructions(&self) -> AuthorizationInstructions {
        let instructions = self.0.instructions();
        AuthorizationInstructions {
            authorization_url: instructions.authorization_url.clone(),
            user_code: None,
            callback_uri: Some(instructions.callback_url.clone()),
        }
    }

    fn poll(&self) -> LocalBoxFuture<'_, tembra::Result<Option<Zeroizing<String>>>> {
        Box::pin(async move {
            self.0
                .poll()
                .await
                .map_err(|error| failure(&error.to_string()))?
                .map(|credential| {
                    swarmie_provider_auth::encode(&credential).map_err(|_| invalid_credential())
                })
                .transpose()
        })
    }

    fn submit_redirect<'a>(
        &'a self,
        callback: &'a str,
    ) -> LocalBoxFuture<'a, tembra::Result<Zeroizing<String>>> {
        Box::pin(async move {
            let credential = self
                .0
                .submit_redirect(callback)
                .await
                .map_err(|error| failure(&error.to_string()))?;
            swarmie_provider_auth::encode(&credential).map_err(|_| invalid_credential())
        })
    }

    fn cancel(&self) {
        self.0.cancel();
    }

    fn close(&self) -> LocalBoxFuture<'_, ()> {
        Box::pin(self.0.close())
    }
}
