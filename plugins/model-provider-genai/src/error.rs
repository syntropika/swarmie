use swarmie_model_contracts::{ProviderError, ProviderErrorKind as K, Submission};

pub(crate) fn failure(kind: K, submission: Submission) -> ProviderError {
    ProviderError {
        kind,
        submission,
        http_status: None,
        retry_after_ms: None,
    }
}

/// Never format genai errors: their Display/Debug can contain credentials and full prompts.
pub(crate) fn map(error: genai::Error) -> ProviderError {
    let detail = match &error {
        genai::Error::WebStream { error, .. } => error.downcast_ref::<genai::Error>(),
        _ => None,
    }
    .unwrap_or(&error);
    let status = detail.status().map(|s| s.as_u16());
    let retry_after_ms = detail
        .headers()
        .and_then(|h| {
            h.get("retry-after-ms")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .or_else(|| {
                    h.get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok())
                        .and_then(|v| v.checked_mul(1000))
                })
        })
        .filter(|v| *v <= 86_400_000);
    let kind = match status {
        Some(401 | 403) => K::Authentication,
        Some(429) => K::RateLimited,
        Some(_) => K::Http,
        None => match &error {
            genai::Error::WebStream { error, .. }
                if error.downcast_ref::<genai::webc::StreamTimeout>().is_some()
                    || error
                        .downcast_ref::<reqwest::Error>()
                        .is_some_and(|e| e.is_timeout()) =>
            {
                K::Timeout
            }
            genai::Error::WebModelCall {
                webc_error: genai::webc::Error::Reqwest(e),
                ..
            }
            | genai::Error::WebAdapterCall {
                webc_error: genai::webc::Error::Reqwest(e),
                ..
            } if e.is_timeout() => K::Timeout,
            genai::Error::WebModelCall {
                webc_error:
                    genai::webc::Error::ResponseFailedInvalidJson { .. }
                    | genai::webc::Error::ResponseFailedNotJson { .. },
                ..
            } => K::Parsing,
            genai::Error::StreamParse { .. }
            | genai::Error::ChatResponseGeneration { .. }
            | genai::Error::InvalidJsonResponseElement { .. }
            | genai::Error::NoChatResponse { .. } => K::Parsing,
            genai::Error::RequiresApiKey { .. }
            | genai::Error::NoAuthData { .. }
            | genai::Error::NoAuthResolver { .. } => K::Authentication,
            genai::Error::AdapterNotSupported { .. }
            | genai::Error::MessageRoleNotSupported { .. }
            | genai::Error::MessageContentTypeNotSupported { .. } => K::Unsupported,
            _ => K::Transport,
        },
    };
    ProviderError {
        kind,
        submission: if status.is_some() {
            Submission::ResponseReceived
        } else {
            Submission::MayHaveBeenSent
        },
        http_status: status,
        retry_after_ms,
    }
}
