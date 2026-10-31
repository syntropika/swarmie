use http::{HeaderMap, HeaderValue, StatusCode};
pub mod framing;
pub mod multipart;
pub use multipart::MultipartForm;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("Http error: {0}")]
    Protocol(#[from] http::Error),
    #[error("Invalid status code {status} with message: {body}")]
    InvalidStatusCodeWithDetails {
        status: StatusCode,
        body: String,
        headers: http::HeaderMap,
    },
    #[error("Header value outside of legal range: {0}")]
    InvalidHeaderValue(#[from] http::header::InvalidHeaderValue),
    #[error("Request in error state, cannot access headers")]
    NoHeaders,
    #[error("Stream ended")]
    StreamEnded,
    #[error("Invalid content type was returned: {0:?}")]
    InvalidContentType(HeaderValue),
    #[cfg(not(target_family = "wasm"))]
    #[error("Http client error: {0}")]
    Instance(#[from] Box<dyn std::error::Error + Send + Sync + 'static>),

    #[cfg(target_family = "wasm")]
    #[error("Http client error: {0}")]
    Instance(#[from] Box<dyn std::error::Error + 'static>),

    #[cfg(not(target_family = "wasm"))]
    #[error("Http client unavailable: {0}")]
    Unbuilt(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),

    #[cfg(target_family = "wasm")]
    #[error("Http client unavailable: {0}")]
    Unbuilt(#[source] Box<dyn std::error::Error + 'static>),
}

impl Error {
    pub fn non_success_status(&self) -> Option<StatusCode> {
        match self {
            Self::InvalidStatusCodeWithDetails { status, .. } => Some(*status),
            _ => None,
        }
    }

    pub fn non_success_body(&self) -> Option<&str> {
        match self {
            Self::InvalidStatusCodeWithDetails { body, .. } => Some(body.as_str()),
            _ => None,
        }
    }

    pub fn non_success_with_details(status: StatusCode, headers: HeaderMap, body: String) -> Self {
        Self::InvalidStatusCodeWithDetails {
            status,
            body,
            headers,
        }
    }

    pub fn non_success_headers(&self) -> Option<&HeaderMap> {
        match self {
            Self::InvalidStatusCodeWithDetails { headers, .. } => Some(headers),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    #[cfg(not(target_family = "wasm"))]
    pub fn instance<E: std::error::Error + Send + Sync + 'static>(error: E) -> Self {
        Self::Instance(error.into())
    }

    #[cfg(target_family = "wasm")]
    pub fn instance<E: std::error::Error + 'static>(error: E) -> Self {
        Self::Instance(error.into())
    }

    #[cfg(not(target_family = "wasm"))]
    pub fn unbuilt<E: std::error::Error + Send + Sync + 'static>(error: E) -> Self {
        Self::Unbuilt(error.into())
    }

    #[cfg(target_family = "wasm")]
    pub fn unbuilt<E: std::error::Error + 'static>(error: E) -> Self {
        Self::Unbuilt(error.into())
    }
}
