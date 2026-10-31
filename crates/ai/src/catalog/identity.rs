use crate::catalog::ResolutionError;
use serde::{Deserialize, Serialize};

/// Exact operational route. Model names never select an endpoint or an adapter.
/// The host supplies a non-secret deployment identifier when the endpoint alone
/// does not identify the deployment. An absent revision means unreported revision.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelIdentity {
    pub provider: String,
    pub adapter: String,
    pub endpoint: String,
    pub deployment: Option<String>,
    pub model: String,
    pub revision: Option<String>,
}

impl ModelIdentity {
    /// Validate exact identifiers and reject credentials in the public endpoint.
    /// No case folding, prefix removal or model alias resolution is performed.
    pub fn validate(&self) -> Result<(), ResolutionError> {
        for value in [&self.provider, &self.adapter, &self.model]
            .into_iter()
            .chain(self.deployment.as_ref())
            .chain(self.revision.as_ref())
        {
            if value.trim().is_empty() || value.len() > 1024 || value.chars().any(char::is_control)
            {
                return Err(ResolutionError::InvalidIdentity);
            }
        }
        let endpoint =
            url::Url::parse(&self.endpoint).map_err(|_| ResolutionError::InvalidIdentity)?;
        if !matches!(endpoint.scheme(), "http" | "https")
            || endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(ResolutionError::InvalidIdentity);
        }
        Ok(())
    }
}
