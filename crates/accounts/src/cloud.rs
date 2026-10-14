// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
//! Bounded first-party HTTP exchanges; request proofs and bearer values never enter diagnostics.
use crate::{Error, Result};
use reqwest::{Client, Method, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Credentials {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub profile: Option<crate::AccountProfile>,
}
#[derive(Clone)]
pub(crate) struct Cloud {
    origin: Url,
    client: Client,
}
impl Cloud {
    pub fn new(origin: &str) -> Result<Self> {
        let origin = validate_origin(origin)?;
        let client = Client::builder()
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .build()
            .map_err(|_| Error::Unavailable)?;
        Ok(Self { origin, client })
    }
    pub fn authorization_url(&self, value: &str) -> Result<String> {
        let parsed = Url::parse(value).map_err(|_| Error::Invalid)?;
        if parsed.origin() != self.origin.origin()
            || parsed.path() != "/auth/native/authorize"
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.fragment().is_some()
        {
            return Err(Error::Invalid);
        }
        Ok(parsed.to_string())
    }
    pub async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        access: Option<&str>,
    ) -> Result<Value> {
        let url = self.origin.join(path).map_err(|_| Error::Invalid)?;
        let mut request = self.client.request(method, url);
        if let Some(body) = body {
            request = request.json(&body);
        }
        if let Some(token) = access {
            request = request.bearer_auth(token);
        }
        let mut response = request.send().await.map_err(|_| Error::Unavailable)?;
        match response.status().as_u16() {
            429 => {
                let seconds = response
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<u64>().ok())
                    .filter(|value| (1..=86_400).contains(value))
                    .unwrap_or(60);
                return Err(Error::RateLimited(seconds));
            }
            400 => return Err(Error::Invalid),
            401 | 403 => return Err(Error::Unauthenticated),
            409 => return Err(Error::Conflict),
            200..=299 => {}
            _ => return Err(Error::Unavailable),
        }
        if response.status() == reqwest::StatusCode::NO_CONTENT {
            return Ok(Value::Null);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Error::Unavailable)? {
            if bytes.len() + chunk.len() > 256 * 1024 {
                return Err(Error::Invalid);
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| Error::Invalid)
    }
}
pub fn validate_origin(value: &str) -> Result<Url> {
    let url = Url::parse(value).map_err(|_| Error::Invalid)?;
    if !(url.scheme() == "https" || (url.scheme() == "http" && url.host_str() == Some("127.0.0.1")))
        || !url.username().is_empty()
        || url.password().is_some()
        || !matches!(url.path(), "/" | "")
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::Invalid);
    }
    Ok(url)
}
