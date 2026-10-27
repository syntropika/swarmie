use tembra::{Code, Error};

#[derive(Clone, Copy)]
pub(crate) enum Reason {
    Configuration,
    Request,
    Response,
    Credentials,
    Runtime,
    Transport,
}
impl Reason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Configuration => "swarmie.models.genai.configuration",
            Self::Request => "swarmie.models.genai.request",
            Self::Response => "swarmie.models.genai.response",
            Self::Credentials => "swarmie.models.genai.credentials",
            Self::Runtime => "swarmie.models.genai.runtime",
            Self::Transport => "swarmie.models.genai.transport",
        }
    }
}
pub(crate) fn invalid(reason: Reason, message: &str) -> Error {
    Error::invalid(reason.as_str(), message)
}

/// Remote diagnostics may contain credentials, prompts and response bodies.
/// Keep them private rather than forwarding upstream Display or Debug text.
pub(crate) fn upstream(error: genai::Error) -> Error {
    let code = match error.status().map(|s| s.as_u16()) {
        Some(401 | 403) => Code::Forbidden,
        Some(429) => Code::Exhausted,
        Some(400 | 404 | 422) => Code::Invalid,
        _ => Code::Unavailable,
    };
    Error::new(
        code,
        Reason::Transport.as_str(),
        "model provider request did not yield a confirmed response",
    )
}
