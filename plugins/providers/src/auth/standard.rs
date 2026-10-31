//! Standard authorization-code, PKCE, refresh and device-request handling belongs to oauth2.
//! The transport adapter shares Swarmie's bounded reqwest client and never formats library errors.

use std::{future::Future, pin::Pin};

use crate::auth::Result;
use oauth2::{
    AuthType, AuthorizationCode, Client, ClientId, DeviceAuthorizationUrl, EndpointNotSet,
    EndpointSet, HttpRequest, HttpResponse, PkceCodeVerifier, RedirectUrl, RefreshToken, Scope,
    StandardDeviceAuthorizationResponse, StandardRevocableToken, StandardTokenResponse, TokenUrl,
    basic::{
        BasicClient, BasicErrorResponse, BasicRevocationErrorResponse,
        BasicTokenIntrospectionResponse, BasicTokenType,
    },
};
use serde::{Deserialize, Serialize};

use super::{Provider, invalid, protocol, unavailable};

#[derive(Serialize, Deserialize)]
pub(crate) struct Extras {
    #[serde(default)]
    pub id_token: Option<oauth2::AccessToken>,
}
impl std::fmt::Debug for Extras {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OAuthTokenExtras { .. }")
    }
}
impl oauth2::ExtraTokenFields for Extras {}

pub(crate) type Token = StandardTokenResponse<Extras, BasicTokenType>;

// These provider flows accept an omitted token_type and use Bearer, as in their upstream clients.
// Keep that interoperability rule here while oauth2 owns the standard token fields and errors.
fn compatible_token(mut value: serde_json::Value) -> serde_json::Value {
    if let Some(fields) = value.as_object_mut()
        && fields
            .get("access_token")
            .is_some_and(serde_json::Value::is_string)
        && !fields.contains_key("token_type")
    {
        fields.insert(
            "token_type".into(),
            serde_json::Value::String("Bearer".into()),
        );
    }
    value
}

pub(crate) fn parse_token(value: serde_json::Value) -> Result<Token> {
    serde_json::from_value(compatible_token(value)).map_err(|_| invalid())
}

type ProviderClient<HasTokenUrl = EndpointNotSet> = Client<
    BasicErrorResponse,
    Token,
    BasicTokenIntrospectionResponse,
    StandardRevocableToken,
    BasicRevocationErrorResponse,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    HasTokenUrl,
>;

fn client(id: &str, url: String) -> Result<ProviderClient<EndpointSet>> {
    Ok(ProviderClient::new(ClientId::new(id.to_owned()))
        .set_auth_type(AuthType::RequestBody)
        .set_token_uri(TokenUrl::new(url).map_err(|_| invalid())?))
}

type ResponseFuture = Pin<Box<dyn Future<Output = std::io::Result<HttpResponse>> + Send>>;

fn transport(http: reqwest::Client, provider: Provider) -> impl Fn(HttpRequest) -> ResponseFuture {
    move |request| {
        let http = http.clone();
        Box::pin(async move {
            let (parts, body) = request.into_parts();
            let request = http
                .request(parts.method, parts.uri.to_string())
                .headers(parts.headers)
                .body(body);
            let (status, body) = protocol::read(protocol::identity(request, provider))
                .await
                .map_err(|_| std::io::Error::other("provider authorization HTTP request failed"))?;
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                return Err(std::io::Error::other(
                    "provider authorization is temporarily unavailable",
                ));
            }
            let body = if status.is_success() {
                serde_json::from_slice(&body)
                    .ok()
                    .and_then(|value| serde_json::to_vec(&compatible_token(value)).ok())
                    .unwrap_or_else(|| body.to_vec())
            } else {
                body.to_vec()
            };
            oauth2::http::Response::builder()
                .status(status)
                .header("Content-Type", "application/json")
                .body(body)
                .map_err(|_| {
                    std::io::Error::other("provider authorization HTTP response is invalid")
                })
        })
    }
}

fn token_error(
    error: oauth2::RequestTokenError<std::io::Error, BasicErrorResponse>,
) -> crate::auth::AuthError {
    use oauth2::{RequestTokenError, basic::BasicErrorResponseType};
    match error {
        RequestTokenError::ServerResponse(response) => match response.error() {
            BasicErrorResponseType::InvalidGrant
            | BasicErrorResponseType::InvalidClient
            | BasicErrorResponseType::UnauthorizedClient => crate::auth::AuthError::Denied,
            BasicErrorResponseType::InvalidRequest
            | BasicErrorResponseType::InvalidScope
            | BasicErrorResponseType::UnsupportedGrantType => crate::auth::AuthError::Invalid,
            _ => crate::auth::AuthError::Unavailable,
        },
        RequestTokenError::Parse(..) => crate::auth::AuthError::Invalid,
        _ => crate::auth::AuthError::Unavailable,
    }
}

pub(crate) async fn exchange_code(
    http: reqwest::Client,
    id: &str,
    url: String,
    code: String,
    verifier: String,
    redirect: &str,
    resource: Option<&str>,
) -> Result<Token> {
    let client = client(id, url)?
        .set_redirect_uri(RedirectUrl::new(redirect.to_owned()).map_err(|_| invalid())?);
    let request = client
        .exchange_code(AuthorizationCode::new(code))
        .set_pkce_verifier(PkceCodeVerifier::new(verifier));
    let request = if let Some(resource) = resource {
        request.add_extra_param("resource", resource)
    } else {
        request
    };
    request
        .request_async(&transport(http, Provider::Chatgpt))
        .await
        .map_err(token_error)
}

pub(crate) async fn refresh(
    http: reqwest::Client,
    provider: Provider,
    id: &str,
    url: String,
    refresh: &str,
    resource: Option<&str>,
) -> Result<Token> {
    let client = client(id, url)?;
    let work = async {
        let attempts = match provider {
            Provider::Xai => 3,
            Provider::Kimi => 4,
            _ => 1,
        };
        for attempt in 0..attempts {
            let refresh_value = RefreshToken::new(refresh.to_owned());
            let request = client.exchange_refresh_token(&refresh_value);
            let request = if let Some(resource) = resource {
                request.add_extra_param("resource", resource)
            } else {
                request
            };
            let result = request
                .request_async(&transport(http.clone(), provider))
                .await
                .map_err(token_error);
            if result
                .as_ref()
                .is_err_and(|error| *error == crate::auth::AuthError::Unavailable)
                && attempt + 1 < attempts
            {
                tokio::time::sleep(if provider == Provider::Kimi {
                    std::time::Duration::from_secs(1 << attempt)
                } else {
                    std::time::Duration::from_millis(250 << attempt)
                })
                .await;
            } else {
                return result;
            }
        }
        Err(unavailable())
    };
    tokio::time::timeout(std::time::Duration::from_secs(35), work)
        .await
        .map_err(|_| unavailable())?
}

pub(crate) async fn github_device(
    http: reqwest::Client,
    id: &str,
    url: String,
) -> Result<StandardDeviceAuthorizationResponse> {
    let client = BasicClient::new(ClientId::new(id.to_owned()))
        .set_auth_type(AuthType::RequestBody)
        .set_device_authorization_url(DeviceAuthorizationUrl::new(url).map_err(|_| invalid())?);
    client
        .exchange_device_code()
        .add_scope(Scope::new("read:user".into()))
        .request_async(&transport(http, Provider::Copilot))
        .await
        .map_err(|_| unavailable())
}

pub(crate) async fn xai_device(
    http: reqwest::Client,
    id: &str,
    url: String,
) -> Result<StandardDeviceAuthorizationResponse> {
    let client = BasicClient::new(ClientId::new(id.to_owned()))
        .set_auth_type(AuthType::RequestBody)
        .set_device_authorization_url(DeviceAuthorizationUrl::new(url).map_err(|_| invalid())?);
    client
        .exchange_device_code()
        .add_scopes(
            [
                "openid",
                "profile",
                "email",
                "offline_access",
                "grok-cli:access",
                "api:access",
            ]
            .into_iter()
            .map(|scope| Scope::new(scope.into())),
        )
        .add_extra_param("referrer", "swarmie")
        .request_async(&transport(http, Provider::Xai))
        .await
        .map_err(|_| unavailable())
}
