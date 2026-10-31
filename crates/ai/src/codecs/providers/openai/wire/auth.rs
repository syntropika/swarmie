use serde::{Deserialize, Serialize};

use crate::codecs::wire::Secret;

use super::{Dialect, OpenAIConfig};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Auth {
    Bearer,
    OptionalBearer,
    ApiKeyHeader,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuthAlternative {
    pub api_key_env: &'static str,
    pub auth: Auth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    pub originator: &'static str,
    pub originator_env: &'static str,
    pub user_agent_env: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallerIdentity {
    pub originator: String,
    pub user_agent: String,
}

pub(super) fn default_user_agent(originator: &str) -> String {
    format!(
        "swarmie/{} ({} {}; {originator})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

impl OpenAIConfig {
    pub fn with_alternate_key(dialect: &Dialect, api_key: impl Into<Secret>) -> Self {
        let auth = dialect
            .alternate_auth
            .map_or(dialect.quirks.auth, |alternative| alternative.auth);
        Self {
            auth,
            ..Self::with_key(dialect, api_key)
        }
    }

    pub fn with_auth(mut self, auth: Auth) -> Self {
        self.auth = auth;
        self
    }

    pub fn with_account_id(mut self, account_id: impl Into<String>) -> Self {
        self.account_id = Some(account_id.into());
        self
    }

    pub(crate) fn authenticate(&self, builder: http::request::Builder) -> http::request::Builder {
        match self.auth {
            Auth::Bearer => {
                builder.header("Authorization", format!("Bearer {}", self.api_key.expose()))
            }
            Auth::OptionalBearer if self.api_key.is_empty() => builder,
            Auth::OptionalBearer => {
                builder.header("Authorization", format!("Bearer {}", self.api_key.expose()))
            }
            Auth::ApiKeyHeader => builder.header("api-key", self.api_key.expose()),
        }
    }

    pub(crate) fn headers_in_session(
        &self,
        builder: http::request::Builder,
        session: Option<&str>,
    ) -> http::request::Builder {
        let mut builder = self.authenticate(builder);
        if let Some(identity) = &self.identity {
            builder = builder
                .header("originator", &identity.originator)
                .header(http::header::USER_AGENT, &identity.user_agent);
        }
        if let Some(name) = self.dialect.quirks.session_header {
            builder = builder.header(
                name,
                crate::codecs::providers::internal::session_id(session),
            );
        }
        if let Some(account_id) = &self.account_id {
            builder = builder.header("ChatGPT-Account-Id", account_id);
        }
        builder
    }
}
