use crate::{Configuration, Connection, Credentials, error::failure};
use genai::{
    Client,
    adapter::AdapterKind,
    resolver::{AuthData, Endpoint, ProviderConfig},
};
use std::{cell::Cell, collections::BTreeMap, rc::Rc, time::Duration};
use swarmie_model_contracts::*;
use tembra::{Result, RuntimeError};

pub(crate) struct Backend {
    pub connections: BTreeMap<String, Rc<ClientConnection>>,
    pub generations: Cell<usize>,
}

pub(crate) struct ClientConnection {
    pub configuration: Connection,
    pub kind: AdapterKind,
    pub client: Client,
    pub discovery: ProviderConfig,
    pub active: Cell<u32>,
}

pub(crate) struct Permit(Rc<ClientConnection>);

impl Drop for Permit {
    fn drop(&mut self) {
        self.0.active.set(self.0.active.get() - 1);
    }
}

impl ClientConnection {
    pub fn permit(self: &Rc<Self>) -> std::result::Result<Permit, ProviderError> {
        if self.active.get() >= self.configuration.max_concurrent {
            return Err(failure(ProviderErrorKind::Busy, Submission::NotSent));
        }
        self.active.set(self.active.get() + 1);
        Ok(Permit(self.clone()))
    }

    pub fn capabilities(&self) -> Capabilities {
        use Support::*;
        let native = matches!(
            self.kind,
            AdapterKind::OpenAI
                | AdapterKind::OpenAIResp
                | AdapterKind::Anthropic
                | AdapterKind::Gemini
        );
        let ollama = matches!(self.kind, AdapterKind::Ollama | AdapterKind::OllamaCloud);
        Capabilities {
            streaming: Supported,
            tools: if native || ollama {
                ModelDependent
            } else {
                Unsupported
            },
            json: if native || ollama {
                ModelDependent
            } else {
                Unsupported
            },
            json_schema: if native { ModelDependent } else { Unsupported },
            reasoning: if native || ollama {
                ModelDependent
            } else {
                Unsupported
            },
            images: if native || ollama {
                ModelDependent
            } else {
                Unsupported
            },
            developer_role: Unsupported,
            model_discovery: ModelDependent,
        }
    }
}

impl Backend {
    pub fn open(configuration: &Configuration, credentials: &dyn Credentials) -> Result<Self> {
        let mut connections = BTreeMap::new();
        for c in &configuration.connections {
            let kind = AdapterKind::from_lower_str(&c.provider)
                .ok_or_else(crate::configuration::invalid)?;
            let auth = match &c.credential_env {
                Some(name) => AuthData::from_single(
                    credentials
                        .resolve(name)
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| {
                            RuntimeError::unavailable(
                                "model_credentials",
                                "configured credential is unavailable",
                            )
                        })?,
                ),
                None => AuthData::None,
            };
            let mut discovery = ProviderConfig::from_auth(auth);
            if let Some(url) = &c.base_url {
                discovery = discovery.with_endpoint(Endpoint::from_owned(if url.ends_with('/') {
                    url.clone()
                } else {
                    format!("{url}/")
                }));
            }
            let http = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .connect_timeout(Duration::from_millis(c.timeout_ms.min(30_000)))
                .build()
                .map_err(|_| {
                    RuntimeError::unavailable("model_client", "HTTP client initialization failed")
                })?;
            let client = Client::builder()
                .with_reqwest(http)
                .with_adapter_kind(kind)
                .append_provider_config(kind, discovery.clone())
                .build()
                .map_err(|_| {
                    RuntimeError::unavailable(
                        "model_client",
                        "provider client initialization failed",
                    )
                })?;
            connections.insert(
                c.id.clone(),
                Rc::new(ClientConnection {
                    configuration: c.clone(),
                    kind,
                    client,
                    discovery,
                    active: Cell::new(0),
                }),
            );
        }
        Ok(Self {
            connections,
            generations: Cell::new(0),
        })
    }

    pub fn connection(
        &self,
        model: &ModelIdentity,
    ) -> std::result::Result<Rc<ClientConnection>, ProviderError> {
        let c = self
            .connections
            .get(&model.connection)
            .ok_or_else(|| failure(ProviderErrorKind::InvalidRequest, Submission::NotSent))?;
        if !c.configuration.models.contains(&model.model) {
            return Err(failure(
                ProviderErrorKind::InvalidRequest,
                Submission::NotSent,
            ));
        }
        Ok(c.clone())
    }
}
