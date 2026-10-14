// Copyright 2026 Syntropika
// SPDX-License-Identifier: Apache-2.0
//! Bounded first-party HTTP exchanges; request proofs and bearer values never enter diagnostics.
use crate::{Error, Result};
use reqwest::{Client, Method, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{Duration, SystemTime};

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
                let seconds = retry_after(
                    response
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|value| value.to_str().ok()),
                    SystemTime::now(),
                );
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
/// Accept HTTP delay-seconds and dates, rounding up so a fractional wait never retries early.
fn retry_after(value: Option<&str>, now: SystemTime) -> u64 {
    let parsed = value.and_then(|value| {
        let value = value.trim();
        if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Some(value.parse::<u64>().unwrap_or(u64::MAX));
        }
        let wait = httpdate::parse_http_date(value)
            .ok()?
            .duration_since(now)
            .unwrap_or_default();
        Some(
            wait.as_secs()
                .saturating_add(u64::from(wait.subsec_nanos() > 0)),
        )
    });
    parsed.map(|seconds| seconds.min(86_400)).unwrap_or(60)
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retry_delays_accept_zero_and_bound_large_valid_values() {
        let now = SystemTime::UNIX_EPOCH;
        for (input, expected) in [
            ("0", 0),
            ("120", 120),
            (" 120 ", 120),
            ("86400", 86400),
            ("86401", 86400),
            ("184467440737095516160", 86400),
        ] {
            assert_eq!(retry_after(Some(input), now), expected);
        }
        for invalid in [
            None,
            Some(""),
            Some("-1"),
            Some("+1"),
            Some("1.5"),
            Some("invalid date"),
        ] {
            assert_eq!(retry_after(invalid, now), 60);
        }
    }
    #[test]
    fn retry_dates_accept_all_http_formats_without_shortening_the_wait() {
        let instant = httpdate::parse_http_date("Sun, 06 Nov 1994 08:49:37 GMT").unwrap();
        let now = instant - Duration::from_millis(2250);
        for date in [
            "Sun, 06 Nov 1994 08:49:37 GMT",
            "Sunday, 06-Nov-94 08:49:37 GMT",
            "Sun Nov  6 08:49:37 1994",
        ] {
            assert_eq!(retry_after(Some(date), now), 3);
            assert_eq!(retry_after(Some(date), instant), 0);
            assert_eq!(retry_after(Some(date), instant + Duration::from_secs(1)), 0);
            assert_eq!(
                retry_after(Some(date), instant - Duration::from_secs(172800)),
                86400
            );
        }
    }
}
