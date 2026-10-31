use std::time::{Duration, SystemTime};

use super::{ErrorKind, ErrorReport};

const CONTEXT_OVERFLOW: &str = "context_length_exceeded";

impl ErrorReport {
    pub fn context_overflow(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Request, message).with_code(CONTEXT_OVERFLOW)
    }

    pub fn retry_after(&self, now: SystemTime) -> Option<Duration> {
        let header = |name: &str| {
            self.provider_response_headers()
                .and_then(|headers| headers.get(name))
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
        };
        if let Some(millis) = header("retry-after-ms").and_then(|value| value.parse::<f64>().ok()) {
            return seconds(millis / 1000.0);
        }
        if let Some(value) = header("retry-after") {
            return match value.parse::<f64>() {
                Ok(secs) => seconds(secs),
                Err(_) => httpdate::parse_http_date(value)
                    .ok()
                    .map(|at| at.duration_since(now).unwrap_or_default()),
            };
        }
        let body = self
            .provider_response_body()
            .unwrap_or(&self.message)
            .to_ascii_lowercase();
        ["\"retrydelay\"", "try again in "]
            .iter()
            .find_map(|marker| {
                let after = body.get(body.find(marker)? + marker.len()..)?;
                hinted_wait(after.trim_start_matches([' ', ':', '"']))
            })
    }

    pub fn is_context_overflow(&self) -> bool {
        if self.code.as_deref() == Some(CONTEXT_OVERFLOW) {
            return true;
        }
        if self.is_retryable() {
            return false;
        }
        if self.http_status == Some(413) {
            return true;
        }
        [Some(self.message.as_str()), self.provider_response_body()]
            .into_iter()
            .flatten()
            .map(str::to_ascii_lowercase)
            .any(|text| says_overflow(&text) && !says_rate_limit(&text))
    }
}

// Recognized context-overflow error forms.
const OVERFLOW: &[&str] = &[
    "prompt is too long",
    "prompt too long",
    "prompt exceeds max length",
    "request_too_large",
    "input is too long for requested model",
    "exceeds the context window",
    "maximum context length",
    "maximum prompt length is",
    "reduce the length of the messages",
    "maximum allowed input length",
    "than the model's context length",
    "than the models context length",
    "greater than the context length",
    "exceeds the limit of ",
    "exceeds the available context size",
    "context window exceeds limit",
    "exceeded model token limit",
    "but the configured context size is",
    "model_context_window_exceeded",
    "exceeded context length",
    "exceeded max context length",
    "range of input length should be",
    CONTEXT_OVERFLOW,
    "context length exceeded",
    "too many tokens",
    "token limit exceeded",
];

fn says_overflow(text: &str) -> bool {
    OVERFLOW.iter().any(|phrase| text.contains(phrase))
        || (text.contains("input token count") && text.contains("exceeds the maximum"))
}

fn says_rate_limit(text: &str) -> bool {
    text.starts_with("throttling error:")
        || text.starts_with("service unavailable:")
        || text.contains("rate limit")
        || text.contains("too many requests")
}

fn seconds(secs: f64) -> Option<Duration> {
    Duration::try_from_secs_f64(secs.max(0.0)).ok()
}

fn hinted_wait(text: &str) -> Option<Duration> {
    let end = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    let amount: f64 = text.get(..end)?.parse().ok()?;
    let unit = text.get(end..)?.trim_start();
    let unit_end = unit
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(unit.len());
    match unit.get(..unit_end)? {
        "ms" => seconds(amount / 1000.0),
        "s" => seconds(amount),
        _ => None,
    }
}
