use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::codecs::{
    completion::UnsupportedOption, http_client, observe::AdapterErrorBoundary,
    provider_response::ProviderResponseError,
};

mod retry;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    Http,
    Json,
    Url,
    Request,
    Response,
    Provider,
    ProviderResponse,
    Internal,
    Cancelled,
    Timeout,
    HandlerUnavailable,
    Divergence,
    Denied,
    Other,
}

impl ErrorKind {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Json => "json",
            Self::Url => "url",
            Self::Request => "request",
            Self::Response => "response",
            Self::Provider => "provider",
            Self::ProviderResponse => "provider_response",
            Self::Internal => "internal",
            Self::Cancelled => "cancelled",
            Self::Timeout => "timeout",
            Self::HandlerUnavailable => "handler_unavailable",
            Self::Divergence => "divergence",
            Self::Denied => "denied",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorReport {
    pub kind: ErrorKind,
    pub retryable: bool,
    pub message: String,
    pub code: Option<String>,
    pub http_status: Option<u16>,
    pub refusal: bool,
    pub source_chain: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_response: Option<crate::codecs::provider_response::ProviderResponseError>,
}

impl ErrorReport {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            retryable: false,
            message: message.into(),
            code: None,
            http_status: None,
            refusal: false,
            source_chain: Vec::new(),
            request_id: None,
            provider_response: None,
        }
    }

    pub fn with_retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    pub fn with_http_status(mut self, status: u16) -> Self {
        self.http_status = Some(status);
        self
    }

    pub fn refused(mut self) -> Self {
        self.refusal = true;
        self
    }

    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }

    pub const fn is_retryable(&self) -> bool {
        self.retryable
    }
}

impl ErrorReport {
    pub fn provider_response_body(&self) -> Option<&str> {
        self.provider_response
            .as_ref()
            .map(|response| response.body.as_str())
    }

    pub fn provider_response_json(&self) -> Result<Option<serde_json::Value>, serde_json::Error> {
        crate::codecs::provider_response::json(self.provider_response_body())
    }

    pub fn provider_response_headers(&self) -> Option<&http::HeaderMap> {
        self.provider_response
            .as_ref()
            .and_then(|response| response.headers.as_ref())
    }

    pub fn provider_response_status(&self) -> Option<http::StatusCode> {
        self.http_status
            .and_then(|status| http::StatusCode::from_u16(status).ok())
    }

    pub fn provider_request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }
}

impl fmt::Display for ErrorReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ErrorReport {}

pub const fn retryable_status(status: Option<u16>) -> bool {
    match status {
        None => false,
        Some(408 | 425 | 429) => true,
        Some(s) => s >= 500 && s <= 599,
    }
}

pub fn transient_transport(error: &http_client::Error) -> bool {
    match error {
        http_client::Error::StreamEnded | http_client::Error::Instance(_) => true,
        http_client::Error::InvalidStatusCodeWithDetails { status, .. } => {
            retryable_status(Some(status.as_u16()))
        }
        _ => false,
    }
}

pub(crate) fn source_chain(error: &(dyn std::error::Error + 'static)) -> Vec<String> {
    let mut chain = Vec::new();
    let mut current = error.source();
    while let Some(source) = current {
        chain.push(source.to_string());
        current = source.source();
    }
    chain
}

#[cfg(not(target_family = "wasm"))]
pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

#[cfg(target_family = "wasm")]
pub type BoxError = Box<dyn std::error::Error + 'static>;

#[cfg(not(target_family = "wasm"))]
pub type SharedError = Arc<dyn std::error::Error + Send + Sync + 'static>;

#[cfg(target_family = "wasm")]
pub type SharedError = Arc<dyn std::error::Error + 'static>;

#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum ProviderError {
    Http(Arc<http_client::Error>),
    Json(Arc<serde_json::Error>),
    Url(url::ParseError),
    Request(SharedError),
    Response(String),
    Provider(String),
    ProviderResponse(Box<ProviderResponseError>),
    InvalidAuthentication(Box<ProviderResponseError>),
    CacheExpired {
        name: String,
        response: Box<ProviderResponseError>,
    },
    MismatchedDimensions {
        provider: String,
        requested: usize,
        returned: usize,
    },
    Relayed(Box<ErrorReport>),
    Truncated,
    Stalled(std::time::Duration),
    UnsupportedOption(UnsupportedOption),
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(error) => write!(f, "HttpError: {error}"),
            Self::Json(error) => write!(f, "JsonError: {error}"),
            Self::Url(error) => write!(f, "UrlError: {error}"),
            Self::Request(error) => write!(f, "RequestError: {error}"),
            Self::Response(message) => write!(f, "ResponseError: {message}"),
            Self::Provider(message) => write!(f, "ProviderError: {message}"),
            Self::ProviderResponse(response) => write!(f, "ProviderResponseError: {response}"),
            Self::InvalidAuthentication(response) => {
                write!(f, "invalid authentication: {response}")
            }
            Self::CacheExpired { name, response } => write!(
                f,
                "cached content `{name}` is expired or was deleted: {}",
                response.body
            ),
            Self::MismatchedDimensions {
                provider,
                requested,
                returned,
            } => write!(
                f,
                "{provider} embedding response returned {returned}-dimension vectors, but the \
                 model was created with {requested} dimensions; this provider does not resize \
                 embeddings"
            ),
            Self::Relayed(report) => f.write_str(&report.message),
            Self::Truncated => {
                f.write_str("ResponseError: the reply ended before the provider ended it")
            }
            Self::Stalled(idle) => write!(
                f,
                "TimeoutError: the streamed reply sent nothing for {}s",
                idle.as_secs_f64()
            ),
            Self::UnsupportedOption(option) => write!(f, "RequestError: {option}"),
        }
    }
}

impl std::error::Error for ProviderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(&**error),
            Self::Url(error) => Some(error),
            Self::Request(error) => Some(&**error),
            _ => None,
        }
    }
}

impl From<url::ParseError> for ProviderError {
    fn from(error: url::ParseError) -> Self {
        Self::Url(error)
    }
}

impl ProviderError {
    pub fn request(reason: impl Into<BoxError>) -> Self {
        Self::Request(Arc::from(reason.into()))
    }

    pub fn from_http_response(status: http::StatusCode, body: impl Into<String>) -> Self {
        Self::ProviderResponse(Box::new(ProviderResponseError::new(status, body)))
    }

    pub fn from_provider_body(body: impl Into<String>) -> Self {
        Self::ProviderResponse(Box::new(ProviderResponseError::from_body(body)))
    }

    pub fn from_transport_error(error: http_client::Error) -> Self {
        match error {
            http_client::Error::InvalidStatusCodeWithDetails {
                status,
                body,
                headers,
            } => Self::from_http_response(status, body).with_response_headers(Some(headers)),
            other => Self::Http(Arc::new(other)),
        }
    }

    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::Http(_) => ErrorKind::Http,
            Self::Json(_) => ErrorKind::Json,
            Self::Url(_) => ErrorKind::Url,
            Self::Request(_) | Self::UnsupportedOption(_) => ErrorKind::Request,
            Self::Response(_) | Self::MismatchedDimensions { .. } | Self::Truncated => {
                ErrorKind::Response
            }
            Self::Provider(_) => ErrorKind::Provider,
            Self::Stalled(_) => ErrorKind::Timeout,
            Self::ProviderResponse(_)
            | Self::InvalidAuthentication(_)
            | Self::CacheExpired { .. } => ErrorKind::ProviderResponse,
            Self::Relayed(report) => report.kind,
        }
    }

    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Http(error) => transient_transport(error),
            Self::ProviderResponse(response) => response.is_retryable(),
            Self::Relayed(report) => report.retryable,
            // A reply cut short, by its transport or its runtime, may end on
            // the next attempt.
            Self::Truncated => true,
            // A stalled stream is dropped, so the next attempt starts afresh.
            Self::Stalled(_) => true,
            _ => false,
        }
    }

    pub fn provider_response(&self) -> Option<&ProviderResponseError> {
        match self {
            Self::ProviderResponse(response)
            | Self::InvalidAuthentication(response)
            | Self::CacheExpired { response, .. } => Some(response),
            Self::Relayed(report) => report.provider_response.as_ref(),
            _ => None,
        }
    }

    pub fn provider_response_body(&self) -> Option<&str> {
        self.provider_response()
            .map(|response| response.body.as_str())
    }

    pub fn provider_response_json(&self) -> Result<Option<serde_json::Value>, serde_json::Error> {
        crate::codecs::provider_response::json(self.provider_response_body())
    }

    pub fn provider_response_status(&self) -> Option<http::StatusCode> {
        self.provider_response()
            .and_then(|response| response.status)
    }

    pub fn provider_request_id(&self) -> Option<&str> {
        self.provider_response()
            .and_then(|response| response.provider_request_id.as_deref())
    }

    pub fn provider_response_headers(&self) -> Option<&http::HeaderMap> {
        self.provider_response()
            .and_then(|response| response.headers.as_ref())
    }

    pub fn with_provider_request_id(self, request_id: Option<String>) -> Self {
        self.map_response(|response| match response.provider_request_id {
            Some(_) => response,
            None => response.with_provider_request_id(request_id),
        })
    }

    pub fn with_response_headers(self, headers: Option<http::HeaderMap>) -> Self {
        self.map_response(|response| match (&response.headers, headers) {
            (None, Some(headers)) => response.with_headers(Some(headers)),
            _ => response,
        })
    }

    pub fn with_provider_status(self, status: Option<http::StatusCode>) -> Self {
        self.map_response(|response| response.with_status(status))
    }

    pub fn with_provider_code(self, code: Option<String>) -> Self {
        self.map_response(|response| response.with_code(code))
    }

    pub fn with_transient(self, transient: Option<bool>) -> Self {
        self.map_response(|response| response.with_transient(transient))
    }

    pub fn report(&self) -> ErrorReport {
        ErrorReport::from(self)
    }

    pub fn boundary(&self) -> AdapterErrorBoundary {
        match (self, self.kind()) {
            (Self::Http(error), _) => AdapterErrorBoundary::from_http(error),
            (Self::Stalled(_), _) => AdapterErrorBoundary::Transport,
            (_, ErrorKind::Json | ErrorKind::Response) => AdapterErrorBoundary::Decode,
            (_, ErrorKind::Provider | ErrorKind::ProviderResponse) => {
                AdapterErrorBoundary::ProviderResponse
            }
            _ => AdapterErrorBoundary::Request,
        }
    }

    fn map_response(
        self,
        map: impl FnOnce(ProviderResponseError) -> ProviderResponseError,
    ) -> Self {
        match self {
            Self::ProviderResponse(response) => Self::ProviderResponse(Box::new(map(*response))),
            Self::InvalidAuthentication(response) => {
                Self::InvalidAuthentication(Box::new(map(*response)))
            }
            Self::CacheExpired { name, response } => Self::CacheExpired {
                name,
                response: Box::new(map(*response)),
            },
            // The report restates what it read from the reply it preserves,
            // so what the map fills reaches both. What it held is kept.
            Self::Relayed(mut report) => {
                if let Some(response) = report.provider_response.take() {
                    let response = map(response);
                    report.request_id = report
                        .request_id
                        .or_else(|| response.provider_request_id.clone());
                    report.http_status = report
                        .http_status
                        .or_else(|| response.status.map(|status| status.as_u16()));
                    report.code = report.code.or_else(|| response.machine_code());
                    report.refusal |= response.refusal;
                    report.provider_response = Some(response);
                }
                Self::Relayed(report)
            }
            other => other,
        }
    }
}

impl From<serde_json::Error> for ProviderError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(Arc::new(error))
    }
}

impl From<BoxError> for ProviderError {
    fn from(error: BoxError) -> Self {
        Self::Request(Arc::from(error))
    }
}

impl From<http_client::Error> for ProviderError {
    fn from(error: http_client::Error) -> Self {
        Self::from_transport_error(error)
    }
}

#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct EncodeError(ProviderError);

impl EncodeError {
    pub fn request(reason: impl Into<BoxError>) -> Self {
        Self(ProviderError::request(reason))
    }

    pub fn unsupported(option: UnsupportedOption) -> Self {
        Self(ProviderError::UnsupportedOption(option))
    }

    pub fn unsupported_option(&self) -> Option<&UnsupportedOption> {
        match &self.0 {
            ProviderError::UnsupportedOption(option) => Some(option),
            _ => None,
        }
    }
}

impl From<EncodeError> for ProviderError {
    fn from(error: EncodeError) -> Self {
        debug_assert_eq!(error.0.kind(), ErrorKind::Request);
        error.0
    }
}

impl From<http::Error> for EncodeError {
    fn from(error: http::Error) -> Self {
        Self::request(error)
    }
}

impl From<serde_json::Error> for EncodeError {
    fn from(error: serde_json::Error) -> Self {
        Self::request(error)
    }
}

impl From<crate::codecs::message::MessageError> for EncodeError {
    fn from(error: crate::codecs::message::MessageError) -> Self {
        Self::request(error)
    }
}

impl From<BoxError> for EncodeError {
    fn from(error: BoxError) -> Self {
        Self(ProviderError::from(error))
    }
}

impl From<http::Error> for ProviderError {
    fn from(error: http::Error) -> Self {
        Self::request(error)
    }
}

impl From<&ProviderError> for ErrorReport {
    fn from(error: &ProviderError) -> Self {
        if let ProviderError::Relayed(report) = error {
            return (**report).clone();
        }
        let response = error.provider_response();
        ErrorReport {
            kind: error.kind(),
            retryable: error.is_retryable(),
            message: error.to_string(),
            code: response.and_then(ProviderResponseError::machine_code),
            http_status: response
                .and_then(|response| response.status)
                .map(|status| status.as_u16()),
            refusal: response.is_some_and(|response| response.refusal),
            source_chain: source_chain(error),
            request_id: response.and_then(|response| response.provider_request_id.clone()),
            provider_response: response.cloned(),
        }
    }
}

impl From<ProviderError> for ErrorReport {
    fn from(error: ProviderError) -> Self {
        Self::from(&error)
    }
}

// The report is the wire error of the effect protocol: it must cross threads
// and serialize on every target, browser wasm included.
const _: fn() = || {
    fn assert_wire<T: Send + Sync + 'static + Serialize + serde::de::DeserializeOwned>() {}
    assert_wire::<ErrorReport>();
    assert_wire::<ErrorKind>();
};
