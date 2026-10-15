use crate::*;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

/// Persisted connection settings contain environment references, never credentials.
#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpDefinition {
    pub transport: String,
    #[serde(default)]
    pub command: Option<PathBuf>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub env_vars: Vec<String>,
    #[serde(default)]
    pub header_env_vars: BTreeMap<String, String>,
    #[serde(default)]
    pub bearer_token_env_var: Option<String>,
}
impl McpDefinition {
    pub fn validate(&self) -> Result<()> {
        if !matches!(self.transport.as_str(), "stdio" | "http" | "sse") {
            return Err(Error::Invalid(
                "Choose STDIO or an HTTP MCP transport".into(),
            ));
        }
        if self.transport == "stdio" {
            if self
                .command
                .as_ref()
                .is_none_or(|path| path.as_os_str().is_empty())
            {
                return Err(Error::Invalid("MCP command is required".into()));
            }
            if self.url.is_some()
                || !self.header_env_vars.is_empty()
                || self.bearer_token_env_var.is_some()
            {
                return Err(Error::Invalid("STDIO cannot include HTTP settings".into()));
            }
        } else {
            let url = url::Url::parse(self.url.as_deref().unwrap_or_default())
                .map_err(|_| Error::Invalid("MCP URL must be HTTP or HTTPS".into()))?;
            if !matches!(url.scheme(), "http" | "https")
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query_pairs().any(|(key, _)| {
                    matches!(
                        key.to_ascii_lowercase().as_str(),
                        "token"
                            | "access_token"
                            | "api_key"
                            | "secret"
                            | "password"
                            | "authorization"
                    )
                })
            {
                return Err(Error::Invalid(
                    "Use an HTTP URL and environment references for credentials".into(),
                ));
            }
            if self.command.is_some() || !self.args.is_empty() || !self.env_vars.is_empty() {
                return Err(Error::Invalid(
                    "HTTP cannot include process settings".into(),
                ));
            }
        }
        for name in self
            .env_vars
            .iter()
            .chain(self.header_env_vars.values())
            .chain(self.bearer_token_env_var.iter())
        {
            if name.is_empty()
                || name.as_bytes()[0].is_ascii_digit()
                || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            {
                return Err(Error::Invalid(
                    "Environment names must contain letters, digits or underscores".into(),
                ));
            }
        }
        if self.header_env_vars.keys().any(|key| {
            key.is_empty()
                || !key
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
        }) {
            return Err(Error::Invalid(
                "HTTP header names must be valid tokens".into(),
            ));
        }
        if self.args.len() > 128
            || self.env_vars.len() > 128
            || self.header_env_vars.len() > 64
            || serde_json::to_vec(self)?.len() > 64 * 1024
        {
            return Err(Error::Invalid(
                "MCP settings exceed the supported size".into(),
            ));
        }
        Ok(())
    }
    pub(crate) fn resolve(
        &self,
        name: String,
        execution: &SessionExecutionOptions,
    ) -> Result<McpServer> {
        self.validate()?;
        let read = |key: &str| {
            execution
                .env
                .get(key)
                .cloned()
                .or_else(|| std::env::var(key).ok())
                .ok_or_else(|| {
                    Error::Invalid(format!(
                        "MCP credential environment variable {key} is unavailable on this node"
                    ))
                })
        };
        let transport = if self.transport == "stdio" {
            let env = self
                .env_vars
                .iter()
                .map(|key| Ok((key.clone(), read(key)?)))
                .collect::<Result<_>>()?;
            McpTransport::Stdio {
                command: self.command.clone().unwrap(),
                args: self.args.clone(),
                env,
            }
        } else {
            let mut headers: BTreeMap<String, String> = self
                .header_env_vars
                .iter()
                .map(|(key, value)| Ok((key.clone(), read(value)?)))
                .collect::<Result<_>>()?;
            if let Some(key) = &self.bearer_token_env_var {
                headers.insert("Authorization".into(), format!("Bearer {}", read(key)?));
            }
            if self.transport == "http" {
                McpTransport::Http {
                    url: self.url.clone().unwrap(),
                    headers,
                }
            } else {
                McpTransport::Sse {
                    url: self.url.clone().unwrap(),
                    headers,
                }
            }
        };
        Ok(McpServer { name, transport })
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpQuery {
    #[serde(skip)]
    pub externally_managed: bool,
    #[serde(skip)]
    pub owner_home: Option<PathBuf>,
    #[serde(skip)]
    pub owner_env: BTreeMap<String, String>,
    pub harness: Option<String>,
    pub cwd: Option<PathBuf>,
    pub session_id: Option<String>,
    pub config_home: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct McpEntry {
    pub id: String,
    pub revision: String,
    pub name: String,
    pub native_name: String,
    pub origin: String,
    pub harness: Option<String>,
    pub scope: String,
    pub source: Option<PathBuf>,
    pub project_path: Option<PathBuf>,
    pub enabled: bool,
    pub definition: McpDefinition,
    pub actions: Vec<String>,
    pub credential_fields: Vec<String>,
    pub read_only_reason: Option<String>,
    pub runtime_status: String,
    pub auth_status: String,
    pub tools: Vec<String>,
    pub requires_restart: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct McpCatalog {
    pub entries: Vec<McpEntry>,
    pub limitations: Vec<String>,
    pub restart_sessions: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct McpMutation {
    #[serde(default)]
    pub query: McpQuery,
    pub action: String,
    pub id: Option<String>,
    pub expected_revision: Option<String>,
    pub origin: Option<String>,
    pub name: Option<String>,
    pub scope: Option<String>,
    pub definition: Option<McpDefinition>,
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct McpReceipt {
    pub action: String,
    pub entry: Option<McpEntry>,
    pub restart_sessions: Vec<String>,
    pub authorization_url: Option<String>,
}

pub fn validate_mcp_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 128
        || name == "swarmie_host"
        || name.starts_with("swarmie_managed_")
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        return Err(Error::Invalid(
            "Use a unique MCP name containing letters, digits, hyphens or underscores".into(),
        ));
    }
    Ok(())
}
