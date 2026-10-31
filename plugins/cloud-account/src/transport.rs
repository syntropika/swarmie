use crate::{Error, Result};
use futures::StreamExt;
use reqwest::{
    Method, Url,
    header::{AUTHORIZATION, CONTENT_TYPE, HeaderValue},
};
use serde::{Serialize, de::DeserializeOwned};
use std::time::Duration;
use zeroize::Zeroizing;

const RESPONSE_LIMIT: usize = 8 * 1024 * 1024;

/// Explicit HTTPS origins supplied by the distribution or trusted native account configuration.
/// Session custody binds both; a new endpoint never receives another endpoint's saved tokens.
#[derive(Clone, Eq, PartialEq, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoints {
    pub api: String,
    /// Allowed browser authorization origin and path prefix; never a bearer-token destination.
    pub authentication: String,
}

impl Endpoints {
    pub fn validate(&self) -> Result<()> {
        for text in [&self.api, &self.authentication] {
            let url = Url::parse(text).map_err(|_| Error::Invalid)?;
            if url.scheme() != "https"
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
                || !url.path().ends_with('/')
            {
                return Err(Error::Invalid);
            }
        }
        Ok(())
    }
}

/// Does not follow redirects, retry requests or discover credentials from the environment.
#[derive(Clone)]
pub struct AccountTransport {
    pub(crate) endpoints: Endpoints,
    pub(crate) client: reqwest::Client,
}

pub(crate) struct Reply {
    pub status: u16,
    pub bytes: Zeroizing<Vec<u8>>,
}
impl Reply {
    pub fn decode<T: DeserializeOwned>(&self) -> Result<T> {
        serde_json::from_slice(&self.bytes).map_err(|_| Error::Integrity)
    }
    pub fn success(&self) -> Result<()> {
        match self.status {
            200..=299 => Ok(()),
            401 => Err(Error::Revoked),
            403 => Err(Error::Forbidden),
            408 | 429 | 500..=599 => Err(Error::Unavailable),
            _ => Err(Error::Refused),
        }
    }
}

impl AccountTransport {
    pub fn new(endpoints: Endpoints, anchor: Option<reqwest::Certificate>) -> Result<Self> {
        endpoints.validate()?;
        let mut builder = reqwest::Client::builder()
            .https_only(true)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(60));
        if let Some(anchor) = anchor {
            builder = builder.tls_certs_only([anchor]);
        }
        Ok(Self {
            endpoints,
            client: builder.build().map_err(|_| Error::Unavailable)?,
        })
    }
    pub fn endpoints(&self) -> &Endpoints {
        &self.endpoints
    }

    pub(crate) async fn exchange(
        &self,
        path: &str,
        method: Method,
        body: Option<&[u8]>,
        bearer: Option<&str>,
    ) -> Result<Reply> {
        if !path.starts_with('/')
            || path.starts_with("//")
            || path.contains(['?', '#'])
            || path.split('/').any(|s| s == "..")
        {
            return Err(Error::Invalid);
        }
        let base = Url::parse(&self.endpoints.api).map_err(|_| Error::Invalid)?;
        let url = base
            .join(path.trim_start_matches('/'))
            .map_err(|_| Error::Invalid)?;
        if url.origin() != base.origin() || !url.path().starts_with(base.path()) {
            return Err(Error::Invalid);
        }
        let mut request = self.client.request(method, url);
        if let Some(body) = body {
            request = request
                .header(CONTENT_TYPE, "application/json")
                .body(body.to_vec());
        }
        if let Some(token) = bearer {
            let mut value = Zeroizing::new(String::from("Bearer "));
            value.push_str(token);
            let mut header = HeaderValue::from_str(&value).map_err(|_| Error::Invalid)?;
            header.set_sensitive(true);
            request = request.header(AUTHORIZATION, header);
        }
        let response = request.send().await.map_err(|_| Error::Unavailable)?;
        let status = response.status().as_u16();
        let mut stream = response.bytes_stream();
        let mut bytes = Zeroizing::new(vec![]);
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| Error::Unavailable)?;
            if bytes.len().saturating_add(chunk.len()) > RESPONSE_LIMIT {
                return Err(Error::Integrity);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(Reply { status, bytes })
    }
}
