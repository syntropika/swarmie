use super::ExecutionAttempt;
use crate::{Error, OutputMask, Result, SecretReference};
use futures::StreamExt;
use reqwest::{
    Method, Url,
    header::{HeaderName, HeaderValue},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, time::Duration};
use zeroize::Zeroizing;

const RESPONSE_LIMIT: usize = 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeaderCredential {
    pub reference: SecretReference,
    pub prefix: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JsonCredential {
    pub pointer: String,
    pub reference: SecretReference,
}

/// Public reference-only request. Body credentials replace one exact reserved reference leaf.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpRequest {
    pub url: String,
    pub method: String,
    pub body: Option<Value>,
    pub credentials: BTreeMap<String, HeaderCredential>,
    pub fields: Vec<JsonCredential>,
    pub timeout_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HttpReceipt {
    pub status: u16,
    pub body: String,
    pub truncated: bool,
}

/// Owns redirect/retry policy. A caller cannot inject a client that follows credential redirects.
pub struct HttpExecutor {
    client: reqwest::Client,
}

impl HttpExecutor {
    /// Optional trust anchor is supplied only by trusted embedding configuration, never a tool.
    pub fn new(anchor: Option<reqwest::Certificate>) -> Result<Self> {
        let mut client = reqwest::Client::builder()
            .https_only(true)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(30));
        if let Some(anchor) = anchor {
            client = client.tls_certs_only([anchor]);
        }
        Ok(Self {
            client: client.build().map_err(|_| Error::Unavailable)?,
        })
    }

    pub async fn execute(
        &self,
        attempt: &ExecutionAttempt<'_>,
        request: HttpRequest,
    ) -> Result<HttpReceipt> {
        match self.run(attempt, request, false, None).await? {
            HttpResult::Visible(receipt) => Ok(receipt),
            HttpResult::Captured(_) => Err(Error::Integrity),
        }
    }
    pub(super) async fn run(
        &self,
        attempt: &ExecutionAttempt<'_>,
        request: HttpRequest,
        capture: bool,
        approved: Option<Vec<crate::UseIntent>>,
    ) -> Result<HttpResult> {
        let (url, method) = validate(&request)?;
        let references: Vec<_> = request
            .credentials
            .values()
            .map(|v| v.reference)
            .chain(request.fields.iter().map(|v| v.reference))
            .collect();
        // Show canonical HTTPS destination plus the exact reference-only effective request.
        let destination = format!(
            "{} {} {}",
            method,
            url,
            String::from_utf8(crate::canonical(&request)?).map_err(|_| Error::Invalid)?
        );
        let intents = if let Some(approved) = approved {
            approved
        } else {
            attempt.approvals(&references, &destination).await?
        };
        let leases = attempt.leases(&intents)?;
        let mut builder = self
            .client
            .request(method, url)
            .timeout(Duration::from_millis(request.timeout_ms));
        for (name, binding) in &request.credentials {
            let lease = leases
                .iter()
                .find(|l| l.reference == binding.reference)
                .ok_or(Error::Invalid)?;
            let value = lease.deliver(|secret| {
                let mut bytes = Zeroizing::new(binding.prefix.as_bytes().to_vec());
                bytes.extend_from_slice(secret);
                let mut header = HeaderValue::from_bytes(&bytes).map_err(|_| Error::Unsupported)?;
                header.set_sensitive(true);
                Ok(header)
            })?;
            builder = builder.header(
                HeaderName::from_bytes(name.as_bytes()).map_err(|_| Error::Invalid)?,
                value,
            );
        }
        let mut body = PrivateJson(request.body);
        for field in &request.fields {
            let target = body
                .0
                .as_mut()
                .and_then(|v| v.pointer_mut(&field.pointer))
                .ok_or(Error::Invalid)?;
            let lease = leases
                .iter()
                .find(|l| l.reference == field.reference)
                .ok_or(Error::Invalid)?;
            *target = lease.deliver(|secret| {
                Ok(Value::String(
                    std::str::from_utf8(secret)
                        .map_err(|_| Error::Unsupported)?
                        .to_owned(),
                ))
            })?;
        }
        if let Some(value) = &body.0 {
            let bytes = Zeroizing::new(serde_json::to_vec(value).map_err(|_| Error::Invalid)?);
            builder = builder
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(bytes.to_vec());
        }
        let mut mask = OutputMask::new(leases)?;
        // No await after lease acquisition until the single physical HTTP attempt starts here.
        let response = builder.send().await.map_err(|_| Error::Unknown)?;
        drop(body);
        let status = response.status().as_u16();
        let mut stream = response.bytes_stream();
        let mut safe = Zeroizing::new(vec![]);
        let mut truncated = false;
        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(_) if capture => return Err(Error::Unknown),
                Err(_) => {
                    return Ok(HttpResult::Visible(HttpReceipt {
                        status,
                        body: "[response withheld: interrupted output]".into(),
                        truncated: true,
                    }));
                }
            };
            if capture {
                safe.extend_from_slice(&chunk);
            } else {
                safe.extend(mask.push(&chunk));
            }
            if safe.len() > RESPONSE_LIMIT {
                truncated = true;
                break;
            }
        }
        drop(stream);
        if capture {
            if truncated || !(200..300).contains(&status) {
                return Err(Error::Unknown);
            }
            return Ok(HttpResult::Captured(safe));
        }
        if truncated && mask.has_secrets() {
            return Ok(HttpResult::Visible(HttpReceipt {
                status,
                body: "[response withheld: output limit exceeded]".into(),
                truncated: true,
            }));
        }
        safe.extend(mask.finish_stream());
        let body = if let Ok(value) = serde_json::from_slice::<Value>(&safe) {
            let mut value = PrivateJson(Some(value));
            mask.mask_json(value.0.as_mut().ok_or(Error::Invalid)?);
            serde_json::to_string(value.0.as_ref().ok_or(Error::Unknown)?)
                .unwrap_or_else(|_| "[response withheld: unsupported JSON output]".into())
        } else if mask.has_secrets() && safe.contains(&b'\\') {
            "[response withheld: unsupported escaped output]".into()
        } else {
            std::str::from_utf8(&safe)
                .unwrap_or("[response withheld: non-text output]")
                .to_owned()
        };
        Ok(HttpResult::Visible(HttpReceipt {
            status,
            body,
            truncated,
        }))
    }
}

pub(super) enum HttpResult {
    Visible(HttpReceipt),
    Captured(Zeroizing<Vec<u8>>),
}

pub(super) fn validate(request: &HttpRequest) -> Result<(Url, Method)> {
    if !(1..=300_000).contains(&request.timeout_ms)
        || request.credentials.len() + request.fields.len() > 16
        || crate::canonical(request)?.len() > 262_144
    {
        return Err(Error::Invalid);
    }
    let url = Url::parse(&request.url).map_err(|_| Error::Invalid)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || request.url.contains("{{secret:")
    {
        return Err(Error::Unsupported);
    }
    let method = match request.method.as_str() {
        "GET" => Method::GET,
        "POST" => Method::POST,
        "PUT" => Method::PUT,
        "PATCH" => Method::PATCH,
        "DELETE" => Method::DELETE,
        _ => return Err(Error::Unsupported),
    };
    for (name, binding) in &request.credentials {
        if matches!(
            name.to_ascii_lowercase().as_str(),
            "host" | "connection" | "proxy-authorization" | "content-length" | "transfer-encoding"
        ) || HeaderName::from_bytes(name.as_bytes()).is_err()
            || binding.prefix.len() > 256
            || binding.prefix.contains(['\r', '\n', '\0'])
        {
            return Err(Error::Invalid);
        }
    }
    for field in &request.fields {
        if field.pointer.is_empty()
            || field.pointer.len() > 1024
            || request
                .body
                .as_ref()
                .and_then(|b| b.pointer(&field.pointer))
                .and_then(Value::as_str)
                != Some(&field.reference.to_string())
        {
            return Err(Error::Invalid);
        }
    }
    Ok((url, method))
}

pub(super) struct PrivateJson(pub(super) Option<Value>);
impl Drop for PrivateJson {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        fn clear(value: Value) {
            match value {
                Value::String(mut s) => s.zeroize(),
                Value::Array(values) => values.into_iter().for_each(clear),
                Value::Object(values) => {
                    for (mut key, value) in values {
                        key.zeroize();
                        clear(value);
                    }
                }
                _ => {}
            }
        }
        if let Some(value) = self.0.take() {
            clear(value);
        }
    }
}
