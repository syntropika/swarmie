use std::time::Duration;

use crate::Result;
use oauth2::TokenResponse;
use reqwest::{Client, RequestBuilder, StatusCode};
use serde_json::Value;
use zeroize::Zeroizing;

use super::{AuthSession, Credential, Provider, invalid, now_ms, unavailable};

const KIMI_CLIENT: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";
const COPILOT_CLIENT: &str = "Iv1.b507a08c87ecfe98";
const XAI_CLIENT: &str = "b1a00492-073a-47ea-816f-4c329264a828";
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";

/// Public upstream client registrations; none is represented as Swarmie's registration.
#[derive(Clone)]
pub struct Endpoints {
    pub chatgpt_token: String,
    pub chatgpt_jwks: String,
    pub kimi_auth: String,
    pub github_auth: String,
    pub copilot_exchange: String,
    pub xai_auth: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            chatgpt_token: crate::chatgpt::TOKEN.into(),
            chatgpt_jwks: crate::chatgpt::JWKS.into(),
            kimi_auth: "https://auth.kimi.ai".into(),
            github_auth: "https://github.com".into(),
            copilot_exchange: "https://api.github.com/copilot_internal/v2/token".into(),
            xai_auth: "https://auth.x.ai".into(),
        }
    }
}

/// A native grant with opaque secrets. It may be cloned without exposing its device code.
#[derive(Clone)]
pub struct Device {
    pub provider: Provider,
    pub(crate) code: Zeroizing<String>,
    pub(crate) user_code: String,
    pub interval_ms: u32,
}

/// One provider poll result. Scheduling and persistence belong to the caller.
pub enum Outcome {
    Pending,
    SlowDown(u32),
    Denied,
    Expired,
    Authorized(Credential),
}

/// Fixed provider protocols over a bounded, redirect-free native HTTP client.
#[derive(Clone)]
pub struct ProviderClient {
    client: Client,
    endpoints: Endpoints,
}

impl ProviderClient {
    pub fn new() -> Result<Self> {
        Self::with_endpoints(Endpoints::default())
    }

    /// Overrides native protocol endpoints for trusted adapters and deterministic fixtures.
    /// The caller controls these URLs; never expose them as an untrusted browser parameter.
    pub fn with_endpoints(endpoints: Endpoints) -> Result<Self> {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .user_agent(concat!("Swarmie/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| unavailable())?;
        Ok(Self { client, endpoints })
    }

    /// Begins a device grant without signing in or authorizing an account.
    pub async fn begin(&self, provider: Provider, name: String) -> Result<(Device, AuthSession)> {
        let value: Value = if provider == Provider::Copilot {
            serde_json::to_value(
                super::standard::github_device(
                    self.client.clone(),
                    COPILOT_CLIENT,
                    format!("{}/login/device/code", self.endpoints.github_auth),
                )
                .await?,
            )
            .map_err(|_| invalid())?
        } else if provider == Provider::Xai {
            serde_json::to_value(
                super::standard::xai_device(
                    self.client.clone(),
                    XAI_CLIENT,
                    format!("{}/oauth2/device/code", self.endpoints.xai_auth),
                )
                .await?,
            )
            .map_err(|_| invalid())?
        } else {
            let request = match provider {
                Provider::Chatgpt => return Err(invalid()),
                Provider::Kimi => self.form(
                    format!(
                        "{}/api/oauth/device_authorization",
                        self.endpoints.kimi_auth
                    ),
                    &[("client_id", KIMI_CLIENT)],
                ),
                Provider::Copilot | Provider::OpenRouter | Provider::Xai => return Err(invalid()),
            };
            let (status, body) = read(identity(request, provider)).await?;
            if !status.is_success() {
                return Err(unavailable());
            }
            serde_json::from_slice(&body).map_err(|_| invalid())?
        };
        let code = text(&value, "device_code")?;
        let user_code = value
            .get("user_code")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty() && s.len() <= 256)
            .ok_or_else(invalid)?
            .to_owned();
        let verification_uri = {
            value
                .get("verification_uri")
                .and_then(Value::as_str)
                .filter(|uri| !uri.is_empty())
                .or_else(|| {
                    value
                        .get("verification_uri_complete")
                        .and_then(Value::as_str)
                })
                .ok_or_else(invalid)?
                .to_owned()
        };
        validate_verification(provider, &verification_uri)?;
        let complete = value
            .get("verification_uri_complete")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if let Some(uri) = &complete {
            validate_verification(provider, uri)?;
        }
        if provider == Provider::Xai
            && number(&value, "expires_in").is_none_or(|expiry| expiry == 0)
        {
            return Err(invalid());
        }
        let interval_ms = number(&value, "interval")
            .filter(|interval| provider != Provider::Xai || *interval > 0)
            .unwrap_or(5)
            .clamp(1, 900) as u32
            * 1000;
        let expires_at_ms = now_ms()
            .saturating_add(number(&value, "expires_in").unwrap_or(900).clamp(1, 900) * 1000);
        Ok((
            Device {
                provider,
                code: Zeroizing::new(code),
                user_code: user_code.clone(),
                interval_ms,
            },
            AuthSession {
                id: uuid::Uuid::new_v4().to_string(),
                provider: name,
                verification_uri,
                verification_uri_complete: complete,
                user_code,
                expires_at_ms,
                interval_ms,
            },
        ))
    }

    fn form(&self, url: String, fields: &[(&str, &str)]) -> RequestBuilder {
        let mut encoded = reqwest::Url::parse("https://localhost/").expect("constant URL");
        encoded
            .query_pairs_mut()
            .extend_pairs(fields.iter().copied());
        self.client
            .post(url)
            .header("Accept", "application/json")
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(Zeroizing::new(encoded.query().unwrap_or_default().to_owned()).to_string())
    }

    /// Makes one upstream poll. The caller must honor the advertised interval and expiry.
    pub async fn poll(&self, device: &Device) -> Result<Outcome> {
        let request = match device.provider {
            Provider::Chatgpt => return Err(invalid()),
            Provider::OpenRouter => return Err(invalid()),
            Provider::Kimi => self.form(
                format!("{}/api/oauth/token", self.endpoints.kimi_auth),
                &[
                    ("client_id", KIMI_CLIENT),
                    ("device_code", &device.code),
                    ("grant_type", DEVICE_GRANT),
                ],
            ),
            Provider::Xai => self.form(
                format!("{}/oauth2/token", self.endpoints.xai_auth),
                &[
                    ("client_id", XAI_CLIENT),
                    ("device_code", &device.code),
                    ("grant_type", DEVICE_GRANT),
                ],
            ),
            Provider::Copilot => self.form(
                format!("{}/login/oauth/access_token", self.endpoints.github_auth),
                &[
                    ("client_id", COPILOT_CLIENT),
                    ("device_code", &device.code),
                    ("grant_type", DEVICE_GRANT),
                ],
            ),
        };
        let (status, body) = read(identity(request, device.provider)).await?;
        if status == StatusCode::TOO_MANY_REQUESTS {
            let interval = serde_json::from_slice::<Value>(&body)
                .ok()
                .and_then(|value| number(&value, "interval"))
                .unwrap_or(0)
                .min(900) as u32
                * 1000;
            return Ok(Outcome::SlowDown(interval));
        }
        let value: Value = serde_json::from_slice(&body).map_err(|_| invalid())?;
        if let Some(error) = value.get("error").and_then(Value::as_str) {
            return match error {
                "authorization_pending" => Ok(Outcome::Pending),
                "slow_down" => Ok(Outcome::SlowDown(
                    number(&value, "interval").unwrap_or(0).min(900) as u32 * 1000,
                )),
                "expired_token" | "expired_device_code" => Ok(Outcome::Expired),
                "access_denied"
                | "authorization_denied"
                | "authorization_declined"
                | "incorrect_device_code" => Ok(Outcome::Denied),
                _ => Err(unavailable()),
            };
        }
        if !status.is_success() {
            return Err(unavailable());
        }
        let credential = match device.provider {
            Provider::Chatgpt => return Err(invalid()),
            Provider::OpenRouter => return Err(invalid()),
            Provider::Kimi | Provider::Xai => parse_token(&value, device.provider, None)?,
            Provider::Copilot => {
                self.copilot(&Zeroizing::new(text(&value, "access_token")?))
                    .await?
            }
        };
        Ok(Outcome::Authorized(credential))
    }

    pub async fn refresh(&self, credential: &Credential) -> Result<Credential> {
        if credential.provider == Provider::Chatgpt {
            return crate::chatgpt::refresh(
                self.client.clone(),
                self.endpoints.chatgpt_token.clone(),
                &self.endpoints.chatgpt_jwks,
                credential,
            )
            .await;
        }
        if credential.provider == Provider::OpenRouter {
            return Ok(credential.clone());
        }
        if credential.provider == Provider::Copilot {
            return self.copilot(&credential.refresh_token).await;
        }
        let (url, client_id) = match credential.provider {
            Provider::Chatgpt => return Err(invalid()),
            Provider::Kimi => (
                format!("{}/api/oauth/token", self.endpoints.kimi_auth),
                KIMI_CLIENT,
            ),
            Provider::Xai => (
                format!("{}/oauth2/token", self.endpoints.xai_auth),
                XAI_CLIENT,
            ),
            Provider::Copilot | Provider::OpenRouter => unreachable!(),
        };
        let token = super::standard::refresh(
            self.client.clone(),
            credential.provider,
            client_id,
            url,
            &credential.refresh_token,
            None,
        )
        .await?;
        credential_from_token(&token, credential.provider, Some(credential))
    }

    async fn copilot(&self, source: &Zeroizing<String>) -> Result<Credential> {
        let request = self
            .client
            .get(&self.endpoints.copilot_exchange)
            .bearer_auth(source.as_str())
            .header("Accept", "application/json")
            .header(
                "Editor-Version",
                concat!("swarmie/", env!("CARGO_PKG_VERSION")),
            )
            .header(
                "Editor-Plugin-Version",
                concat!("swarmie/", env!("CARGO_PKG_VERSION")),
            )
            .header("Copilot-Integration-Id", "swarmie");
        let (status, body) = read(request).await?;
        if !status.is_success() {
            return Err(unavailable());
        }
        let value: Value = serde_json::from_slice(&body).map_err(|_| invalid())?;
        let access_token = Zeroizing::new(text(&value, "token")?);
        let endpoint = if let Some(endpoint) = value
            .get("endpoints")
            .and_then(|v| v.get("api"))
            .and_then(Value::as_str)
        {
            trusted_copilot_endpoint(endpoint)?
        } else {
            let proxy = access_token
                .split(';')
                .find_map(|entry| entry.strip_prefix("proxy-ep="));
            if let Some(proxy) = proxy {
                trusted_copilot_endpoint(&format!(
                    "https://{}",
                    proxy.replacen("proxy.", "api.", 1)
                ))?
            } else {
                "https://api.individual.githubcopilot.com".into()
            }
        };
        let expires_at_ms = number(&value, "expires_at")
            .ok_or_else(invalid)?
            .saturating_mul(1000);
        if expires_at_ms <= now_ms() {
            return Err(invalid());
        }
        Ok(Credential {
            provider: Provider::Copilot,
            access_token,
            refresh_token: source.clone(),
            expires_at_ms,
            chatgpt: None,
            endpoint,
        })
    }
}

pub(crate) fn identity(request: RequestBuilder, provider: Provider) -> RequestBuilder {
    if provider == Provider::Kimi {
        request
            .header("X-Msh-Platform", "swarmie")
            .header("X-Msh-Version", env!("CARGO_PKG_VERSION"))
            .header("X-Msh-Device-Name", "Swarmie")
            .header(
                "X-Msh-Device-Model",
                format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
            )
    } else if provider == Provider::Xai {
        request.header("x-grok-client-surface", "ui")
    } else {
        request
    }
}

pub(crate) fn parse_token(
    value: &Value,
    provider: Provider,
    previous: Option<&Credential>,
) -> Result<Credential> {
    let token = super::standard::parse_token(value.clone())?;
    credential_from_token(&token, provider, previous)
}

pub(crate) fn credential_from_token(
    token: &super::standard::Token,
    provider: Provider,
    previous: Option<&Credential>,
) -> Result<Credential> {
    let access_token = Zeroizing::new(token.access_token().secret().to_owned());
    if access_token.is_empty()
        || access_token.chars().any(char::is_control)
        || token.token_type() != &oauth2::basic::BasicTokenType::Bearer
    {
        return Err(invalid());
    }
    let refresh_token = token
        .refresh_token()
        .filter(|t| !t.secret().is_empty())
        .map(|t| Zeroizing::new(t.secret().to_owned()))
        .or_else(|| previous.map(|c| c.refresh_token.clone()))
        .ok_or_else(invalid)?;
    if refresh_token.chars().any(char::is_control) {
        return Err(invalid());
    }
    let expires_at_ms = now_ms().saturating_add(
        token
            .expires_in()
            .or_else(|| (provider == Provider::Xai).then_some(Duration::from_secs(3600)))
            .filter(|d| !d.is_zero())
            .ok_or_else(invalid)?
            .as_secs()
            .min(31_536_000)
            * 1000,
    );
    if provider == Provider::Chatgpt {
        return Err(invalid());
    }
    Ok(Credential {
        provider,
        access_token,
        refresh_token,
        expires_at_ms,
        chatgpt: None,
        endpoint: match provider {
            Provider::Chatgpt => return Err(invalid()),
            Provider::Kimi => "https://api.kimi.ai/coding/v1/",
            Provider::Xai => "https://api.x.ai/v1/",
            Provider::Copilot | Provider::OpenRouter => return Err(invalid()),
        }
        .into(),
    })
}

pub fn trusted_copilot_endpoint(raw: &str) -> Result<String> {
    let url = reqwest::Url::parse(raw).map_err(|_| invalid())?;
    let host = url.host_str().ok_or_else(invalid)?;
    if url.scheme() != "https"
        || !(host == "api.githubcopilot.com" || host.ends_with(".githubcopilot.com"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err(invalid());
    }
    Ok(raw.trim_end_matches('/').to_owned())
}

fn validate_verification(provider: Provider, raw: &str) -> Result<()> {
    let url = reqwest::Url::parse(raw).map_err(|_| invalid())?;
    let allowed = match provider {
        Provider::Chatgpt => return Err(invalid()),
        Provider::Kimi => &[
            "auth.kimi.ai",
            "www.kimi.ai",
            "kimi.ai",
            "www.kimi.com",
            "kimi.com",
        ][..],
        Provider::Copilot => &["github.com"][..],
        Provider::OpenRouter => &["openrouter.ai"][..],
        Provider::Xai => &["auth.x.ai", "accounts.x.ai", "x.ai"][..],
    };
    if url.scheme() != "https"
        || !allowed.contains(&url.host_str().unwrap_or_default())
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || raw.len() > 2048
    {
        return Err(invalid());
    }
    Ok(())
}

fn text(value: &Value, key: &str) -> Result<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 16_384 && !s.chars().any(char::is_control))
        .map(str::to_owned)
        .ok_or_else(invalid)
}
fn number(value: &Value, key: &str) -> Option<u64> {
    value
        .get(key)
        .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
}

pub(crate) async fn read(request: RequestBuilder) -> Result<(StatusCode, Zeroizing<Vec<u8>>)> {
    let mut response = request.send().await.map_err(|_| unavailable())?;
    let status = response.status();
    if response.content_length().is_some_and(|n| n > 65536) {
        return Err(invalid());
    }
    let mut body = Zeroizing::new(Vec::new());
    while let Some(chunk) = response.chunk().await.map_err(|_| unavailable())? {
        if body.len() + chunk.len() > 65536 {
            return Err(invalid());
        }
        body.extend_from_slice(&chunk);
    }
    Ok((status, body))
}
