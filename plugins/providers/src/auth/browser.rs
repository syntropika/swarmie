//! Owned loopback callbacks and browser authorization-code flows.

use std::{
    fmt,
    net::{Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
    time::Duration,
};

use oauth2::{CsrfToken, PkceCodeChallenge};
use reqwest::{Client, Url};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Notify, watch},
    task::{AbortHandle, JoinHandle},
    time::Instant,
};
use zeroize::Zeroizing;

use crate::auth::{AuthError, Credential, Provider, Result, invalid, now_ms, unavailable};

const XAI_CLIENT: &str = "b1a00492-073a-47ea-816f-4c329264a828";
const XAI_SCOPE: &str = "openid profile email offline_access grok-cli:access api:access";
const HEADER_LIMIT: usize = 8192;
const LOGIN_TTL: Duration = Duration::from_secs(600);

/// Browser instructions exclude the PKCE verifier and access and refresh tokens.
/// Returning ChatGPT instructions include an identity-token hint for the provider.
#[derive(Clone)]
pub struct BrowserInstructions {
    pub authorization_url: String,
    pub callback_url: String,
    pub expires_at_ms: u64,
}

impl fmt::Debug for BrowserInstructions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Returning account authorization URLs can contain a protected ID-token hint.
        f.debug_struct("BrowserInstructions")
            .field("expires_at_ms", &self.expires_at_ms)
            .finish_non_exhaustive()
    }
}

enum State {
    Waiting,
    Exchanging,
    Ready(Credential),
    Failed(AuthError),
    Consumed,
}

struct Shared {
    state: Mutex<State>,
    verifier: Mutex<Option<Zeroizing<String>>>,
    csrf: CsrfToken,
    redirect: Url,
    deadline: Instant,
    client: Client,
    provider: Provider,
    token_url: String,
    chatgpt: Option<crate::auth::chatgpt::Attempt>,
    cancel: watch::Sender<bool>,
    changed: Notify,
    exchange_task: Mutex<Option<JoinHandle<()>>>,
}

/// A single-use native sign-in that owns its loopback listener and private PKCE grant.
///
/// Dropping or cancelling the handle stops accepting callbacks and prevents later credential
/// delivery. Upstream requests already received by a provider may still have external effects.
/// Cancelling a `poll` or `submit_redirect` future alone does not cancel this owned sign-in.
pub struct BrowserLogin {
    instructions: BrowserInstructions,
    shared: Arc<Shared>,
    server: AbortHandle,
    server_join: tokio::sync::Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for BrowserLogin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BrowserLogin")
            .field("provider", &self.shared.provider)
            .field("instructions", &self.instructions)
            .finish_non_exhaustive()
    }
}

impl BrowserLogin {
    /// Returns public instructions for a browser on the same machine as the callback listener.
    pub fn instructions(&self) -> &BrowserInstructions {
        &self.instructions
    }

    /// Returns the credential once, or `None` while waiting for authorization or token exchange.
    ///
    /// # Errors
    /// Returns a typed terminal failure after cancellation, expiry, denial or exchange failure.
    /// After delivery, all further credential reads return [`AuthError::Consumed`].
    pub async fn poll(&self) -> Result<Option<Credential>> {
        self.take()
    }

    fn take(&self) -> Result<Option<Credential>> {
        let mut state = self.shared.state.lock().map_err(|_| unavailable())?;
        if Instant::now() >= self.shared.deadline
            && matches!(*state, State::Waiting | State::Exchanging | State::Ready(_))
        {
            *state = State::Failed(AuthError::Expired);
            if let Ok(mut verifier) = self.shared.verifier.lock() {
                verifier.take();
            }
            self.shared.cancel.send_replace(true);
            self.server.abort();
            self.shared.changed.notify_waiters();
        }
        match &*state {
            State::Waiting | State::Exchanging => Ok(None),
            State::Failed(error) => Err(*error),
            State::Consumed => Err(AuthError::Consumed),
            State::Ready(_) => {
                let State::Ready(credential) = std::mem::replace(&mut *state, State::Consumed)
                else {
                    return Err(unavailable());
                };
                Ok(Some(credential))
            }
        }
    }

    /// Accepts a complete callback URL when the browser cannot reach this machine's listener.
    ///
    /// The scheme, host, port and path must match this sign-in. ChatGPT and xAI also require
    /// matching CSRF state; OpenRouter uses an unguessable path because it does not echo state.
    /// Invalid input leaves
    /// a waiting sign-in usable. Only one callback is exchanged; concurrent delivery through
    /// `poll` can consume the result first. An accepted exchange survives cancellation of this
    /// future and remains available through `poll` until the handle is cancelled or dropped.
    ///
    /// # Errors
    /// Returns [`AuthError::Invalid`] for malformed or unrelated callbacks, and a typed terminal
    /// error for denied, expired, cancelled, already consumed or unsuccessful sign-ins.
    pub async fn submit_redirect(&self, input: &str) -> Result<Credential> {
        let callback = self.shared.validate_redirect(input)?;
        self.shared.accept(callback)?;
        self.server.abort();
        loop {
            let changed = self.shared.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if let Some(credential) = self.take()? {
                return Ok(credential);
            }
            changed.await;
        }
    }

    /// Cancels the sign-in and waits until its callback listener has stopped.
    /// Use this method before starting another sign-in on the same callback port.
    /// Repeated calls are safe. An upstream authorization cannot be revoked here.
    pub async fn close(&self) {
        self.cancel();
        let mut server = self.server_join.lock().await;
        if let Some(server) = server.take() {
            let _ = server.await;
        }
        let exchange = self
            .shared
            .exchange_task
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        if let Some(exchange) = exchange {
            let _ = exchange.await;
        }
    }

    /// Stops this listener and prevents further credential delivery. Repeated cancellation is safe.
    /// A credential already delivered to the caller cannot be revoked by this operation.
    pub fn cancel(&self) {
        if let Ok(mut state) = self.shared.state.lock()
            && !matches!(*state, State::Consumed)
        {
            *state = State::Failed(AuthError::Cancelled);
        }
        if let Ok(mut verifier) = self.shared.verifier.lock() {
            verifier.take();
        }
        self.shared.cancel.send_replace(true);
        self.shared.changed.notify_waiters();
        self.server.abort();
    }
}

impl Drop for BrowserLogin {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Native caller configuration for qualified OAuth registrations or local HTTP fixtures.
///
/// This configuration must not be accepted from untrusted browser request fields. Callback
/// binding remains restricted to IPv4 loopback, regardless of the configured endpoints.
#[derive(Clone, Debug)]
pub struct BrowserOptions {
    pub authorization_url: String,
    pub token_url: String,
    pub callback_port: u16,
    pub timeout: Duration,
}

impl BrowserOptions {
    fn production(provider: Provider) -> Result<Self> {
        match provider {
            Provider::Chatgpt => Err(invalid()),
            Provider::OpenRouter => Ok(Self {
                authorization_url: "https://openrouter.ai/auth".into(),
                token_url: "https://openrouter.ai/api/v1/auth/keys".into(),
                callback_port: 0,
                timeout: LOGIN_TTL,
            }),
            Provider::Xai => Ok(Self {
                authorization_url: "https://auth.x.ai/oauth2/authorize".into(),
                token_url: "https://auth.x.ai/oauth2/token".into(),
                callback_port: 0,
                timeout: LOGIN_TTL,
            }),
            _ => Err(invalid()),
        }
    }
}

/// Starts a PKCE sign-in and binds its callback server before returning browser instructions.
///
/// OpenRouter uses
/// a fresh random path under `http://localhost:<free-port>/auth/callback/`; xAI uses
/// `http://127.0.0.1:<free-port>/callback`.
/// Only IPv4 loopback is bound, never a LAN address.
/// Browser opening and credential persistence belong to the caller. This function performs no
/// token exchange until a valid callback arrives. The listener expires after ten minutes.
///
/// # Errors
/// Returns [`AuthError::Invalid`] for providers without a browser flow and
/// [`AuthError::CallbackUnavailable`] if the callback port cannot be acquired. It never displaces
/// another listener. ChatGPT requires the host-bound `begin_chatgpt_browser` entrypoint.
///
/// # Examples
///
/// Starting and cancelling a flow requires no upstream request or provider account:
///
/// ```
/// use swarmie_providers::auth::{AuthError, Provider, begin_browser};
///
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> swarmie_providers::auth::Result<()> {
/// let login = begin_browser(Provider::OpenRouter).await?;
/// assert!(login.poll().await?.is_none());
/// login.cancel();
/// assert!(matches!(login.poll().await, Err(AuthError::Cancelled)));
/// # Ok(())
/// # }
/// ```
pub async fn begin_browser(provider: Provider) -> Result<BrowserLogin> {
    begin_browser_with_options(provider, BrowserOptions::production(provider)?).await
}

/// Starts a loopback browser flow using an explicit native caller configuration.
///
/// # Errors
/// Returns [`AuthError::Invalid`] for unsupported providers, zero or excessive timeouts,
/// credentials or fragments in endpoints, and HTTP endpoints outside loopback. The maximum
/// timeout is ten minutes. Bind and exchange failures have the same behavior as `begin_browser`.
pub async fn begin_browser_with_options(
    provider: Provider,
    options: BrowserOptions,
) -> Result<BrowserLogin> {
    if provider == Provider::Chatgpt {
        return Err(invalid());
    }
    start_browser(provider, options, None).await
}

async fn start_browser(
    provider: Provider,
    options: BrowserOptions,
    chatgpt: Option<crate::auth::chatgpt::Attempt>,
) -> Result<BrowserLogin> {
    if !matches!(
        provider,
        Provider::Chatgpt | Provider::OpenRouter | Provider::Xai
    ) || options.timeout.is_zero()
        || options.timeout > LOGIN_TTL
    {
        return Err(invalid());
    }
    let mut authorization_url = endpoint(&options.authorization_url)?;
    endpoint(&options.token_url)?;
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("Swarmie/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| unavailable())?;
    let listener = TcpListener::bind(SocketAddr::from((
        Ipv4Addr::LOCALHOST,
        options.callback_port,
    )))
    .await
    .map_err(|_| AuthError::CallbackUnavailable)?;
    let port = listener
        .local_addr()
        .map_err(|_| AuthError::CallbackUnavailable)?
        .port();
    let csrf = CsrfToken::new_random();
    let redirect = Url::parse(&match provider {
        // OpenRouter does not echo OAuth state; the one-use callback path owns correlation.
        Provider::OpenRouter => format!("http://localhost:{port}/auth/callback/{}", csrf.secret()),
        Provider::Xai => format!("http://127.0.0.1:{port}/callback"),
        _ => format!("http://127.0.0.1:{port}/auth/callback"),
    })
    .map_err(|_| invalid())?;
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    {
        let mut query = authorization_url.query_pairs_mut();
        query
            .append_pair("code_challenge", challenge.as_str())
            .append_pair("code_challenge_method", "S256");
        if provider != Provider::OpenRouter {
            query.append_pair("state", csrf.secret());
        }
        match provider {
            Provider::Chatgpt => {
                let attempt = chatgpt.as_ref().ok_or_else(invalid)?;
                query
                    .append_pair("response_type", "code")
                    .append_pair("redirect_uri", redirect.as_str())
                    .append_pair("resource", crate::auth::chatgpt::RESOURCE)
                    .append_pair("scope", crate::auth::chatgpt::SCOPE)
                    .append_pair("nonce", &attempt.nonce)
                    .append_pair("ext_agent_host_id", &attempt.host_id);
                if let Some(previous) = &attempt.returning {
                    query
                        .append_pair("client_id", &previous.client_id)
                        .append_pair("id_token_hint", &previous.id_token);
                    if let Some(email) = &previous.identity.email {
                        query.append_pair("login_hint", email);
                    }
                } else {
                    query
                        .append_pair("client_id", crate::auth::chatgpt::DYNAMIC_CLIENT)
                        .append_pair("agent_name_hint", "Swarmie");
                }
            }
            Provider::OpenRouter => {
                query
                    .append_pair("callback_url", redirect.as_str())
                    .append_pair("key_label", "Swarmie");
            }
            Provider::Xai => {
                query
                    .append_pair("response_type", "code")
                    .append_pair("client_id", XAI_CLIENT)
                    .append_pair("redirect_uri", redirect.as_str())
                    .append_pair("scope", XAI_SCOPE)
                    .append_pair("nonce", CsrfToken::new_random().secret())
                    .append_pair("referrer", "swarmie");
            }
            _ => return Err(invalid()),
        }
    }
    let (cancel, _) = watch::channel(false);
    let shared = Arc::new(Shared {
        state: Mutex::new(State::Waiting),
        verifier: Mutex::new(Some(Zeroizing::new(verifier.secret().to_owned()))),
        csrf,
        redirect: redirect.clone(),
        deadline: Instant::now() + options.timeout,
        client,
        provider,
        token_url: options.token_url,
        chatgpt,
        cancel,
        changed: Notify::new(),
        exchange_task: Mutex::new(None),
    });
    let server_shared = shared.clone();
    let task = tokio::spawn(async move { serve(listener, server_shared).await });
    Ok(BrowserLogin {
        instructions: BrowserInstructions {
            authorization_url: authorization_url.into(),
            callback_url: redirect.into(),
            expires_at_ms: now_ms().saturating_add(options.timeout.as_millis() as u64),
        },
        shared,
        server: task.abort_handle(),
        server_join: tokio::sync::Mutex::new(Some(task)),
    })
}

/// Native direct ChatGPT registration configuration. Overrides are for trusted local fixtures.
#[derive(Clone, Debug)]
pub struct ChatgptBrowserOptions {
    pub browser: BrowserOptions,
    pub jwks_url: String,
}
impl Default for ChatgptBrowserOptions {
    fn default() -> Self {
        Self {
            browser: BrowserOptions {
                authorization_url: crate::auth::chatgpt::AUTHORIZE.into(),
                token_url: crate::auth::chatgpt::TOKEN.into(),
                callback_port: 1455,
                timeout: LOGIN_TTL,
            },
            jwks_url: crate::auth::chatgpt::JWKS.into(),
        }
    }
}

/// Starts direct ChatGPT authorization for a UUID already persisted by this native host.
/// A returning account reuses its issued registration and verified identity. No credentials
/// are replaced until a correctly bound callback and signed ID token have been validated.
///
/// # Errors
/// Rejects a nil host UUID, unrelated returning account, occupied callback port, or invalid
/// provider configuration. The caller owns persistence and may cancel the returned login.
pub async fn begin_chatgpt_browser(
    host_id: uuid::Uuid,
    returning: Option<&Credential>,
) -> Result<BrowserLogin> {
    begin_chatgpt_browser_with_options(host_id, returning, ChatgptBrowserOptions::default()).await
}

/// Starts the same host-bound flow with native endpoint overrides for deterministic fixtures.
/// The issuer remains OpenAI's fixed issuer; token and JWKS endpoints cannot be browser inputs.
pub async fn begin_chatgpt_browser_with_options(
    host_id: uuid::Uuid,
    returning: Option<&Credential>,
    options: ChatgptBrowserOptions,
) -> Result<BrowserLogin> {
    endpoint(&options.jwks_url)?;
    let attempt = crate::auth::chatgpt::Attempt::new(host_id, returning, options.jwks_url)?;
    start_browser(Provider::Chatgpt, options.browser, Some(attempt)).await
}

fn endpoint(raw: &str) -> Result<Url> {
    let url = Url::parse(raw).map_err(|_| invalid())?;
    let loopback = url.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    });
    if raw.len() > HEADER_LIMIT
        || !(url.scheme() == "https" || url.scheme() == "http" && loopback)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.host_str().is_none()
    {
        return Err(invalid());
    }
    Ok(url)
}

enum Callback {
    Code {
        code: Zeroizing<String>,
        client_id: Option<String>,
    },
    Denied,
}

impl Shared {
    fn validate_redirect(&self, input: &str) -> Result<Callback> {
        if input.len() > HEADER_LIMIT {
            return Err(invalid());
        }
        let url = Url::parse(input).map_err(|_| invalid())?;
        if url.scheme() != self.redirect.scheme()
            || url.host_str() != self.redirect.host_str()
            || url.port_or_known_default() != self.redirect.port_or_known_default()
            || url.path() != self.redirect.path()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid());
        }
        let mut state = None;
        let mut code = None;
        let mut error = None;
        let mut client_id = None;
        for (key, value) in url.query_pairs() {
            let slot = match key.as_ref() {
                "state" => &mut state,
                "code" => &mut code,
                "error" => &mut error,
                "client_id" => &mut client_id,
                _ => continue,
            };
            if slot.is_some() || value.is_empty() || value.chars().any(char::is_control) {
                return Err(invalid());
            }
            *slot = Some(Zeroizing::new(value.into_owned()));
        }
        match state {
            Some(state) if CsrfToken::new(state.to_string()) != self.csrf => return Err(invalid()),
            None if self.provider != Provider::OpenRouter => return Err(invalid()),
            _ => {}
        }
        match (code, error) {
            (Some(code), None) => {
                let client_id = if self.provider == Provider::Chatgpt {
                    Some(
                        self.chatgpt
                            .as_ref()
                            .ok_or_else(invalid)?
                            .issued_client(client_id.map(|id| id.to_string()))?,
                    )
                } else {
                    None
                };
                Ok(Callback::Code { code, client_id })
            }
            (None, Some(_)) => Ok(Callback::Denied),
            _ => Err(invalid()),
        }
    }

    fn accept(self: &Arc<Self>, callback: Callback) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| unavailable())?;
        match &*state {
            State::Waiting => {}
            State::Failed(error) => return Err(*error),
            State::Consumed | State::Ready(_) | State::Exchanging => {
                return Err(AuthError::Consumed);
            }
        }
        if Instant::now() >= self.deadline {
            *state = State::Failed(AuthError::Expired);
            self.changed.notify_waiters();
            return Err(AuthError::Expired);
        }
        let verifier = self
            .verifier
            .lock()
            .map_err(|_| unavailable())?
            .take()
            .ok_or_else(invalid)?;
        let Callback::Code { code, client_id } = callback else {
            *state = State::Failed(AuthError::Denied);
            self.changed.notify_waiters();
            return Ok(());
        };
        *state = State::Exchanging;
        let shared = self.clone();
        let task = tokio::spawn(async move {
            let mut cancellation = shared.cancel.subscribe();
            let result = if *cancellation.borrow() {
                Err(AuthError::Cancelled)
            } else {
                tokio::select! {
                    biased;
                    _ = cancellation.changed() => Err(AuthError::Cancelled),
                    () = tokio::time::sleep_until(shared.deadline) => Err(AuthError::Expired),
                    result = shared.exchange(code, verifier, client_id) => result,
                }
            };
            if let Ok(mut state) = shared.state.lock()
                && matches!(*state, State::Exchanging)
            {
                *state = match result {
                    Ok(credential) => State::Ready(credential),
                    Err(error) => State::Failed(error),
                };
            }
            shared.changed.notify_waiters();
        });
        *self
            .exchange_task
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(task);
        Ok(())
    }

    async fn exchange(
        &self,
        code: Zeroizing<String>,
        verifier: Zeroizing<String>,
        client_id: Option<String>,
    ) -> Result<Credential> {
        match self.provider {
            Provider::Chatgpt => {
                self.chatgpt
                    .as_ref()
                    .ok_or_else(invalid)?
                    .exchange(
                        self.client.clone(),
                        self.token_url.clone(),
                        crate::auth::chatgpt::CodeGrant {
                            code,
                            verifier,
                            client_id: client_id.ok_or_else(invalid)?,
                            redirect: self.redirect.to_string(),
                        },
                    )
                    .await
            }
            Provider::Xai => {
                let token = crate::auth::standard::exchange_code(
                    self.client.clone(),
                    XAI_CLIENT,
                    self.token_url.clone(),
                    code.to_string(),
                    verifier.to_string(),
                    self.redirect.as_str(),
                    None,
                )
                .await?;
                // Non-ChatGPT ID tokens are not used as verified identity or routing claims.
                crate::auth::protocol::credential_from_token(&token, self.provider, None)
            }
            Provider::OpenRouter => {
                let (status, body) = crate::auth::protocol::read(
                    self.client.post(&self.token_url).json(&serde_json::json!({
                        "code": code.as_str(),
                        "code_verifier": verifier.as_str(),
                        "code_challenge_method": "S256",
                    })),
                )
                .await?;
                if !status.is_success() {
                    return Err(unavailable());
                }
                #[derive(serde::Deserialize)]
                struct KeyResponse {
                    #[serde(deserialize_with = "deserialize_key")]
                    key: Zeroizing<String>,
                }
                fn deserialize_key<'de, D: serde::Deserializer<'de>>(
                    deserializer: D,
                ) -> std::result::Result<Zeroizing<String>, D::Error> {
                    <String as serde::Deserialize>::deserialize(deserializer).map(Zeroizing::new)
                }
                let value: KeyResponse = serde_json::from_slice(&body).map_err(|_| invalid())?;
                if value.key.is_empty()
                    || value.key.len() > 16_384
                    || value.key.chars().any(char::is_control)
                {
                    return Err(invalid());
                }
                Ok(Credential {
                    provider: Provider::OpenRouter,
                    access_token: value.key,
                    refresh_token: Zeroizing::new(String::new()),
                    expires_at_ms: u64::MAX,
                    chatgpt: None,
                    endpoint: "https://openrouter.ai/api/v1/".into(),
                })
            }
            _ => Err(invalid()),
        }
    }
}

async fn serve(listener: TcpListener, shared: Arc<Shared>) {
    let mut cancellation = shared.cancel.subscribe();
    loop {
        tokio::select! {
            biased;
            _ = cancellation.changed() => return,
            () = tokio::time::sleep_until(shared.deadline) => {
                if let Ok(mut state) = shared.state.lock()
                    && matches!(*state, State::Waiting)
                {
                    *state = State::Failed(AuthError::Expired);
                }
                if let Ok(mut verifier) = shared.verifier.lock() {
                    verifier.take();
                }
                shared.changed.notify_waiters();
                return;
            }
            accepted = listener.accept() => {
                let Ok((mut socket, _)) = accepted else {
                    if let Ok(mut state) = shared.state.lock()
                        && matches!(*state, State::Waiting)
                    {
                        *state = State::Failed(unavailable());
                    }
                    shared.changed.notify_waiters();
                    return;
                };
                let result = tokio::time::timeout_at(
                    shared.deadline.min(Instant::now() + Duration::from_secs(2)),
                    serve_request(&mut socket, &shared),
                ).await;
                if matches!(result, Ok(true)) {
                    return;
                }
            }
        }
    }
}

async fn serve_request(socket: &mut TcpStream, shared: &Arc<Shared>) -> bool {
    let mut header = Zeroizing::new(Vec::new());
    let mut chunk = [0_u8; 512];
    loop {
        let Ok(count) = socket.read(&mut chunk).await else {
            return false;
        };
        if count == 0 {
            return false;
        }
        if header.len() + count > HEADER_LIMIT {
            respond(socket, 431, "Request headers are too large.").await;
            return false;
        }
        header.extend_from_slice(&chunk[..count]);
        if header.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            break;
        }
    }
    let Ok(header) = std::str::from_utf8(&header) else {
        respond(socket, 400, "Invalid callback request.").await;
        return false;
    };
    let Some((line, headers)) = header.split_once("\r\n") else {
        return false;
    };
    let parts: Vec<_> = line.split_ascii_whitespace().collect();
    if parts.len() != 3 || parts[0] != "GET" || parts[2] != "HTTP/1.1" {
        respond(socket, 405, "Use a browser to complete this sign-in.").await;
        return false;
    }
    let host = headers
        .lines()
        .filter_map(|line| line.split_once(':'))
        .filter(|(key, _)| key.eq_ignore_ascii_case("Host"))
        .map(|(_, value)| value.trim())
        .collect::<Vec<_>>();
    let expected_host = format!(
        "{}:{}",
        shared.redirect.host_str().unwrap_or_default(),
        shared.redirect.port().unwrap_or(80)
    );
    if host.as_slice() != [expected_host.as_str()]
        || !parts[1].starts_with('/')
        || parts[1].starts_with("//")
    {
        respond(socket, 400, "Invalid callback address.").await;
        return false;
    }
    let Ok(url) = shared.redirect.join(parts[1]) else {
        respond(socket, 400, "Invalid callback address.").await;
        return false;
    };
    if url.path() != shared.redirect.path() {
        respond(socket, 404, "This callback route does not exist.").await;
        return false;
    }
    let accepted = shared.validate_redirect(url.as_str()).and_then(|callback| {
        let denied = matches!(callback, Callback::Denied);
        shared.accept(callback).map(|()| denied)
    });
    match accepted {
        Ok(denied) => {
            respond(
                socket,
                200,
                if denied {
                    "Authorization was declined. Return to Swarmie to try again."
                } else {
                    "Authorization received. Return to Swarmie to finish connecting."
                },
            )
            .await;
            true
        }
        Err(_) => {
            respond(
                socket,
                400,
                "This authorization callback is invalid or no longer active.",
            )
            .await;
            false
        }
    }
}

async fn respond(socket: &mut TcpStream, status: u16, message: &str) {
    let reason = match status {
        200 => "OK",
        404 => "Not Found",
        405 => "Method Not Allowed",
        431 => "Request Header Fields Too Large",
        _ => "Bad Request",
    };
    let body = format!(
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><title>Swarmie authorization</title><body><p>{message}</p></body></html>"
    );
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nContent-Security-Policy: default-src 'none'; frame-ancestors 'none'\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n{body}",
        body.len(),
    );
    let _ = socket.write_all(response.as_bytes()).await;
    let _ = socket.shutdown().await;
}

#[cfg(test)]
mod tests;
