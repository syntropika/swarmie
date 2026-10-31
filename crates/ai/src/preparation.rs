use crate::catalog::{Capability, ModelIdentity, Snapshot};
use crate::{Error, Execution, Failure, Protocol};
use reqwest::{
    Client, Url,
    header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use zeroize::Zeroizing;

/// Authentication profiles are explicit. Anonymous access is limited to native Ollama.
/// Declaring another profile cannot inherit the API-key route's qualification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthProfile {
    ApiKey,
    ChatGpt,
    LegacyCodex,
    Anonymous,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WireMode {
    Complete,
    Stream,
}

/// Canonical preparation accepts typed completion input or explicit protocol JSON.
/// Both forms freeze one final request and pass through the same admission path.
pub enum Request {
    Protocol(Value),
    Completion(Box<crate::CompletionRequest>),
}

impl From<Value> for Request {
    fn from(value: Value) -> Self {
        Self::Protocol(value)
    }
}

impl From<crate::CompletionRequest> for Request {
    fn from(value: crate::CompletionRequest) -> Self {
        Self::Completion(Box::new(value))
    }
}

/// Complete calls have a total deadline. Streams have separate header and idle
/// deadlines. Limits apply even when no progress observer remains attached.
#[derive(Clone, Copy, Debug)]
pub struct Timeouts {
    pub completion: Duration,
    pub headers: Duration,
    pub idle: Duration,
    pub max_response_bytes: usize,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            completion: Duration::from_secs(120),
            headers: Duration::from_secs(30),
            idle: Duration::from_secs(60),
            max_response_bytes: 16 * 1024 * 1024,
        }
    }
}

/// Inspection data, without payload or credentials. It grants no authority.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Description {
    pub protocol: Protocol,
    pub destination: String,
    pub operation: uuid::Uuid,
    pub identity: ModelIdentity,
    pub generation: String,
    pub fingerprint: [u8; 32],
    pub body_bytes: usize,
    pub mode: WireMode,
    pub output_reserve: u64,
}

pub(crate) struct Configuration {
    pub snapshot: Arc<Snapshot>,
    pub endpoint: Url,
    pub headers: HeaderMap,
    pub protocol: Protocol,
    pub credential_expires_at_ms: Option<u64>,
    pub timeouts: Timeouts,
    pub client: Client,
    pub credential_pending: bool,
}

impl Configuration {
    pub fn check(&self) -> Result<(), Error> {
        let now = now_ms()?;
        if !self.snapshot.is_valid_at(now)
            || self
                .credential_expires_at_ms
                .is_some_and(|until| now >= until)
        {
            return Err(Error::before(Failure::Expired));
        }
        Ok(())
    }
}

/// Immutable operational route. Credentials are captured at construction or
/// attached after admission through an explicit deferred route. Account selection,
/// login and renewal remain with the host credential source.
#[derive(Clone)]
pub struct Model(Arc<Configuration>);

impl Model {
    /// The snapshot binds a full POST destination, or a Gemini model-resource URL.
    /// Cleartext HTTP is accepted only for loopback fixtures. The caller supplies
    /// the exact selected credential; no environment fallback or SDK auth runs.
    pub fn new(
        snapshot: Arc<Snapshot>,
        protocol: Protocol,
        profile: AuthProfile,
        secret: Option<Zeroizing<String>>,
        credential_expires_at_ms: Option<u64>,
        timeouts: Timeouts,
    ) -> Result<Self, Error> {
        Self::build(
            snapshot,
            protocol,
            profile,
            secret,
            credential_expires_at_ms,
            timeouts,
            false,
        )
    }

    /// Prepares public content before the host opens credential custody. The final
    /// admitted request cannot dispatch until exact-route credentials are attached.
    pub fn deferred(
        snapshot: Arc<Snapshot>,
        protocol: Protocol,
        profile: AuthProfile,
        timeouts: Timeouts,
    ) -> Result<Self, Error> {
        if profile != AuthProfile::ApiKey {
            return Err(Error::before(Failure::UnsupportedProfile));
        }
        Self::build(snapshot, protocol, profile, None, None, timeouts, true)
    }

    fn build(
        snapshot: Arc<Snapshot>,
        protocol: Protocol,
        profile: AuthProfile,
        secret: Option<Zeroizing<String>>,
        credential_expires_at_ms: Option<u64>,
        timeouts: Timeouts,
        deferred: bool,
    ) -> Result<Self, Error> {
        if profile != AuthProfile::ApiKey
            && !(profile == AuthProfile::Anonymous && protocol == Protocol::Ollama)
        {
            return Err(Error::before(Failure::UnsupportedProfile));
        }
        let endpoint = validate_route(&snapshot, protocol, timeouts)?;
        let mut headers = HeaderMap::new();
        match (profile, secret) {
            (AuthProfile::ApiKey, None) if deferred => {
                let name = credential_header(protocol);
                let mut value = HeaderValue::from_static("");
                value.set_sensitive(true);
                headers.insert(name, value);
            }
            (AuthProfile::Anonymous, None) => {
                if credential_expires_at_ms.is_some() {
                    return Err(Error::before(Failure::Configuration));
                }
            }
            (AuthProfile::ApiKey, Some(secret)) => {
                if secret.is_empty() || secret.chars().any(|c| c.is_control() || c.is_whitespace())
                {
                    return Err(Error::before(Failure::Configuration));
                }
                let (name, value) = match protocol {
                    Protocol::Anthropic => ("x-api-key", Zeroizing::new(secret.to_string())),
                    Protocol::Gemini => ("x-goog-api-key", Zeroizing::new(secret.to_string())),
                    _ => (
                        AUTHORIZATION.as_str(),
                        Zeroizing::new(format!("Bearer {}", secret.as_str())),
                    ),
                };
                let mut value = HeaderValue::from_str(&value)
                    .map_err(|_| Error::before(Failure::Configuration))?;
                value.set_sensitive(true);
                headers.insert(
                    reqwest::header::HeaderName::from_bytes(name.as_bytes())
                        .expect("static header"),
                    value,
                );
            }
            _ => return Err(Error::before(Failure::Configuration)),
        }
        if protocol == Protocol::Anthropic {
            headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        }
        protocol.destination(&endpoint, WireMode::Complete, snapshot.identity())?;
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_proxy()
            .build()
            .map_err(|_| Error::before(Failure::Configuration))?;
        let config = Configuration {
            snapshot,
            endpoint,
            headers,
            protocol,
            credential_expires_at_ms,
            timeouts,
            client,
            credential_pending: deferred,
        };
        config.check()?;
        Ok(Self(Arc::new(config)))
    }

    /// Canonical preparation for complete and streamed responses. The SDK checks
    /// known request shapes. Serialization uses the original value, so unknown
    /// fields on accepted items, opaque reasoning and tool arguments survive.
    /// Unknown SDK union variants fail explicitly instead of being discarded.
    pub fn prepare(
        &self,
        operation: uuid::Uuid,
        request: impl Into<Request>,
        mode: WireMode,
    ) -> Result<Prepared, Error> {
        if operation.is_nil() {
            return Err(Error::before(Failure::Request));
        }
        self.0.check()?;
        let protocol = self.0.protocol;
        let mut headers = self.0.headers.clone();
        let (mut payload, encoded, decode, request_id_header, expected_reserve, start_span) =
            match request.into() {
                Request::Protocol(payload) => (
                    payload,
                    None,
                    None,
                    match protocol {
                        Protocol::Responses | Protocol::ChatCompletions(_) => Some("x-request-id"),
                        Protocol::Anthropic => Some("request-id"),
                        _ => None,
                    },
                    None,
                    None,
                ),
                Request::Completion(request) => {
                    let call =
                        crate::codec::encode(protocol, self.0.snapshot.identity(), *request, mode)?;
                    let payload = serde_json::from_slice(&call.body)
                        .map_err(|_| Error::before(Failure::Request))?;
                    headers.extend(call.headers);
                    (
                        payload,
                        Some(call.body),
                        Some(call.decode),
                        call.request_id_header,
                        Some(call.output_reserve),
                        Some(call.start_span),
                    )
                }
            };
        let original = encoded.as_ref().map(|_| payload.clone());
        let reserve = protocol.prepare(&mut payload, mode, &self.0.snapshot)?;
        if expected_reserve.is_some_and(|expected| expected != reserve) {
            return Err(Error::before(Failure::Request));
        }
        if reserve
            > self
                .0
                .snapshot
                .output()
                .known()
                .expect("validated snapshot")
                .tokens
                .get()
        {
            return Err(Error::before(Failure::Metadata));
        }
        if mode == WireMode::Stream
            && !matches!(self.0.snapshot.streaming(), Capability::Supported { .. })
        {
            return Err(Error::before(Failure::Metadata));
        }
        if payload
            .get("tools")
            .and_then(Value::as_array)
            .is_some_and(|tools| !tools.is_empty())
            && !matches!(self.0.snapshot.tools(), Capability::Supported { .. })
        {
            return Err(Error::before(Failure::Metadata));
        }
        let endpoint = protocol.destination(&self.0.endpoint, mode, self.0.snapshot.identity())?;
        let body = if let Some(bytes) = encoded {
            // The protocol guards may validate, but cannot rewrite an encoded request.
            if original.as_ref() != Some(&payload) {
                return Err(Error::before(Failure::Request));
            }
            bytes
        } else {
            serde_json::to_vec(&payload).map_err(|_| Error::before(Failure::Request))?
        };
        headers.insert(
            reqwest::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        headers.insert(
            reqwest::header::ACCEPT,
            HeaderValue::from_static(match (mode, protocol) {
                (WireMode::Complete, _) => "application/json",
                (WireMode::Stream, Protocol::Ollama) => "application/x-ndjson",
                (WireMode::Stream, _) => "text/event-stream",
            }),
        );
        let mut hash = Sha256::new();
        for part in [
            b"swarmie.protocol.v2".as_slice(),
            protocol.adapter().as_bytes(),
            operation.as_bytes().as_slice(),
            endpoint.as_str().as_bytes(),
            self.0.snapshot.generation().as_bytes(),
            body.as_slice(),
        ] {
            hash.update((part.len() as u64).to_be_bytes());
            hash.update(part);
        }
        let mut frozen_headers = headers.iter().collect::<Vec<_>>();
        frozen_headers.sort_by(|(left, _), (right, _)| left.as_str().cmp(right.as_str()));
        for (name, value) in frozen_headers {
            // The public digest does not expose an offline credential verifier.
            let value = if value.is_sensitive() {
                b"[sensitive]".as_slice()
            } else {
                value.as_bytes()
            };
            for part in [name.as_str().as_bytes(), value] {
                hash.update((part.len() as u64).to_be_bytes());
                hash.update(part);
            }
        }
        let description = Description {
            protocol,
            destination: endpoint.to_string(),
            operation,
            identity: self.0.snapshot.identity().clone(),
            generation: self.0.snapshot.generation().into(),
            fingerprint: hash.finalize().into(),
            body_bytes: body.len(),
            mode,
            output_reserve: reserve,
        };
        Ok(Prepared {
            configuration: self.0.clone(),
            endpoint,
            body,
            headers,
            decode,
            request_id_header,
            start_span,
            description,
            credential_pending: self.0.credential_pending,
            credential_expires_at_ms: self.0.credential_expires_at_ms,
        })
    }
}

/// Single-use private payload. It has no Debug, Clone or serialization implementation.
pub struct Prepared {
    pub(crate) endpoint: Url,
    pub(crate) configuration: Arc<Configuration>,
    pub(crate) body: Vec<u8>,
    pub(crate) headers: HeaderMap,
    pub(crate) decode: Option<crate::codec::ResponseDecoder>,
    pub(crate) request_id_header: Option<&'static str>,
    pub(crate) start_span: Option<crate::codec::StartSpan>,
    pub(crate) description: Description,
    pub(crate) credential_pending: bool,
    pub(crate) credential_expires_at_ms: Option<u64>,
}

impl Prepared {
    pub(crate) fn check_dispatch(&self) -> Result<(), Error> {
        self.configuration.check()?;
        if self.credential_pending {
            return Err(Error::before(Failure::Credential));
        }
        if let Some(until) = self.credential_expires_at_ms
            && now_ms()? >= until
        {
            return Err(Error::before(Failure::Expired));
        }
        Ok(())
    }

    pub fn description(&self) -> &Description {
        &self.description
    }

    /// The embedding authority must measure these exact bytes, check input/context
    /// budgets and output reservation, and apply its submission protection policy.
    /// The callback grants inspection; a description alone cannot dispatch.
    pub fn admit(
        self,
        policy: impl FnOnce(&Description, &[u8], &HeaderMap, &Snapshot) -> Result<(), Error>,
    ) -> Result<Admitted, Error> {
        self.configuration.check()?;
        policy(
            &self.description,
            &self.body,
            &self.headers,
            &self.configuration.snapshot,
        )?;
        Ok(Admitted(self))
    }
}

/// An in-process, single-use proof bound to the inspected private request.
/// The caller must record dispatch intent durably before calling `dispatch`.
pub struct Admitted(pub(crate) Prepared);

impl Admitted {
    /// Attach only the credential for the already admitted route. Content,
    /// destination and public fingerprint remain unchanged. No refresh occurs.
    pub fn authenticate(mut self, credentials: crate::Credentials) -> Result<Self, Error> {
        self.0.configuration.check()?;
        if !self.0.credential_pending
            || credentials.identity != self.0.description.identity
            || credentials.profile != AuthProfile::ApiKey
        {
            return Err(Error::before(Failure::Credential));
        }
        if let Some(until) = credentials.expires_at_ms
            && now_ms()? >= until
        {
            return Err(Error::before(Failure::Expired));
        }
        let secret = credentials
            .secret
            .ok_or_else(|| Error::before(Failure::Credential))?;
        let value = if matches!(
            self.0.configuration.protocol,
            Protocol::Anthropic | Protocol::Gemini
        ) {
            Zeroizing::new(secret.to_string())
        } else {
            Zeroizing::new(format!("Bearer {}", secret.as_str()))
        };
        let mut value =
            HeaderValue::from_str(&value).map_err(|_| Error::before(Failure::Credential))?;
        value.set_sensitive(true);
        self.0
            .headers
            .insert(credential_header(self.0.configuration.protocol), value);
        self.0.credential_expires_at_ms = credentials.expires_at_ms;
        self.0.credential_pending = false;
        self.0.check_dispatch()?;
        Ok(self)
    }

    /// Dispatch on the host's existing local executor, retaining local authority
    /// for the last guard before transport. No second executor is created.
    pub fn dispatch_local(
        self,
        executor: &impl futures::task::LocalSpawn,
        guard: impl FnOnce(&Description) -> Result<(), Error> + 'static,
    ) -> Result<Execution, Error> {
        self.0.check_dispatch()?;
        crate::execution::dispatch_local(self.0, executor, guard)
    }

    /// Dispatch uses a caller-owned runtime. Dropping its observation handle does
    /// not abort the task. Keep the runtime alive through settlement; runtime loss
    /// leaves an unknown external outcome and must not authorize another dispatch.
    /// The guard rechecks captured authority immediately before transport starts.
    /// It must perform only local checks and must not renew or select accounts.
    pub fn dispatch(
        self,
        runtime: &tokio::runtime::Handle,
        guard: impl FnOnce(&Description) -> Result<(), Error> + Send + 'static,
    ) -> Result<Execution, Error> {
        self.0.check_dispatch()?;
        crate::execution::dispatch(self.0, runtime, guard)
    }
}

pub(crate) fn now_ms() -> Result<u64, Error> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|v| v.as_millis().try_into().ok())
        .ok_or_else(|| Error::before(Failure::Configuration))
}

/// Checks public route facts before the host accesses credential custody.
pub(crate) fn validate_route(
    snapshot: &Snapshot,
    protocol: Protocol,
    timeouts: Timeouts,
) -> Result<Url, Error> {
    if snapshot.identity().adapter != protocol.adapter() {
        return Err(Error::before(Failure::Metadata));
    }
    snapshot
        .identity()
        .validate()
        .map_err(|_| Error::before(Failure::Configuration))?;
    if snapshot.context().known().is_none()
        || snapshot.input().known().is_none()
        || snapshot.output().known().is_none()
    {
        return Err(Error::before(Failure::Metadata));
    }
    let endpoint = Url::parse(&snapshot.identity().endpoint)
        .map_err(|_| Error::before(Failure::Configuration))?;
    let loopback = endpoint.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if endpoint.scheme() != "https" && !(endpoint.scheme() == "http" && loopback) {
        return Err(Error::before(Failure::Configuration));
    }
    if timeouts.completion.is_zero()
        || timeouts.headers.is_zero()
        || timeouts.idle.is_zero()
        || timeouts.max_response_bytes == 0
    {
        return Err(Error::before(Failure::Configuration));
    }
    if !snapshot.is_valid_at(now_ms()?) {
        return Err(Error::before(Failure::Expired));
    }
    Ok(endpoint)
}

fn credential_header(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::Anthropic => "x-api-key",
        Protocol::Gemini => "x-goog-api-key",
        _ => "authorization",
    }
}
