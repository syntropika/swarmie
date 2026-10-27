use crate::invalid;
use rmcp::{ClientLifecycleMode, model::ProtocolVersion};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf, time::Duration};

/// Stable configuration identity, independent of a server's display name.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ServerId(String);

impl ServerId {
    pub fn new(value: impl Into<String>) -> tembra::Result<Self> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 64
            || !value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        {
            return Err(invalid(
                "server identity must contain 1 to 64 ASCII letters, digits, underscores or hyphens",
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ServerId {
    type Error = tembra::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ServerId> for String {
    fn from(value: ServerId) -> Self {
        value.0
    }
}

/// Deliberate lifecycle selection. There is no implicit version downgrade.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Protocol {
    #[serde(rename = "2026-07-28")]
    Current,
    #[serde(rename = "2025-11-25")]
    Previous,
}

impl Protocol {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Current => "2026-07-28",
            Self::Previous => "2025-11-25",
        }
    }

    pub(crate) fn version(self) -> ProtocolVersion {
        match self {
            Self::Current => ProtocolVersion::V_2026_07_28,
            Self::Previous => ProtocolVersion::V_2025_11_25,
        }
    }

    pub(crate) fn lifecycle(self) -> ClientLifecycleMode {
        match self {
            Self::Current => ClientLifecycleMode::Discover {
                preferred_versions: vec![self.version()],
            },
            Self::Previous => ClientLifecycleMode::Initialize,
        }
    }
}

/// Configuration only. Constructing or deserializing an endpoint performs no I/O.
/// Command arguments and URLs must not contain authentication secrets.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Endpoint {
    Local {
        program: PathBuf,
        arguments: Vec<String>,
        directory: Option<PathBuf>,
    },
    Http {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
    },
}

/// Host-selected bounds. These are qualification defaults, not product quotas.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub max_catalog_pages: usize,
    pub max_tools: usize,
    pub max_operations: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(15),
            request_timeout: Duration::from_secs(30),
            max_catalog_pages: 32,
            max_tools: 1024,
            max_operations: 4096,
        }
    }
}

/// One endpoint and protocol profile. User/project persistence belongs to the host.
#[derive(Clone)]
pub struct Configuration {
    pub id: ServerId,
    pub endpoint: Endpoint,
    pub protocol: Protocol,
    pub limits: Limits,
}

impl Configuration {
    pub fn validate(&self) -> tembra::Result<()> {
        if self.limits.connect_timeout.is_zero()
            || self.limits.request_timeout.is_zero()
            || self.limits.max_catalog_pages == 0
            || self.limits.max_tools == 0
            || self.limits.max_operations == 0
        {
            return Err(invalid("MCP timeouts and catalog limits must be positive"));
        }
        match &self.endpoint {
            Endpoint::Local { program, .. } if program.as_os_str().is_empty() => {
                Err(invalid("local MCP command must not be empty"))
            }
            Endpoint::Http { url, headers } => {
                validate_headers(headers)?;
                let url = reqwest::Url::parse(url).map_err(|_| invalid("invalid MCP URL"))?;
                if !matches!(url.scheme(), "http" | "https")
                    || url.host_str().is_none()
                    || !url.username().is_empty()
                    || url.password().is_some()
                    || url.fragment().is_some()
                {
                    return Err(invalid(
                        "MCP requires an HTTP(S) URL without user information or fragment",
                    ));
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

pub(crate) fn validate_headers(
    headers: &BTreeMap<String, String>,
) -> tembra::Result<reqwest::header::HeaderMap> {
    if headers.len() > 32 {
        return Err(invalid("use at most 32 HTTP headers"));
    }
    let mut result = reqwest::header::HeaderMap::new();
    for (name, value) in headers {
        let parsed = reqwest::header::HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| invalid("invalid HTTP header name"))?;
        if matches!(
            parsed.as_str(),
            "authorization"
                | "proxy-authorization"
                | "cookie"
                | "x-api-key"
                | "host"
                | "content-length"
                | "connection"
                | "transfer-encoding"
                | "mcp-session-id"
                | "mcp-protocol-version"
                | "accept"
                | "content-type"
        ) {
            return Err(invalid(
                "authentication and transport-owned headers cannot be stored as custom headers",
            ));
        }
        if value.len() > 4096 {
            return Err(invalid("HTTP header value is too long"));
        }
        let value = reqwest::header::HeaderValue::from_str(value)
            .map_err(|_| invalid("invalid HTTP header value"))?;
        if result.insert(parsed, value).is_some() {
            return Err(invalid("duplicate HTTP header name"));
        }
    }
    Ok(result)
}
