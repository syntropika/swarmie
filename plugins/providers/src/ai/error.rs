use tembra::{Code, Error};

#[derive(Clone, Copy)]
pub(crate) enum Reason {
    Configuration,
    Request,
    Response,
    Runtime,
    Transport,
}
impl Reason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Configuration => "swarmie.models.ai.configuration",
            Self::Request => "swarmie.models.ai.request",
            Self::Response => "swarmie.models.ai.response",
            Self::Runtime => "swarmie.models.ai.runtime",
            Self::Transport => "swarmie.models.ai.transport",
        }
    }
}
pub(crate) fn invalid(reason: Reason, message: &str) -> Error {
    Error::invalid(reason.as_str(), message)
}

/// Remote diagnostics may contain credentials, prompts and response bodies.
/// Keep them private rather than forwarding upstream Display or Debug text.
pub(crate) fn upstream(error: swarmie_ai::Error) -> Error {
    let code = match error.failure {
        swarmie_ai::Failure::Http(401 | 403) | swarmie_ai::Failure::Admission => Code::Forbidden,
        swarmie_ai::Failure::Http(429) | swarmie_ai::Failure::Limit => Code::Exhausted,
        swarmie_ai::Failure::Request
        | swarmie_ai::Failure::Configuration
        | swarmie_ai::Failure::Protocol
        | swarmie_ai::Failure::Http(400 | 404 | 422) => Code::Invalid,
        swarmie_ai::Failure::Stopped => Code::Cancelled,
        _ => Code::Unavailable,
    };
    Error::new(
        code,
        if error.effect == swarmie_ai::Effect::NotDispatched {
            "swarmie.models.ai.not_dispatched"
        } else {
            Reason::Transport.as_str()
        },
        format!("model provider request failed ({:?})", error.failure),
    )
}

/// This marker is issued only while the adapter still owns the undispatched body.
pub(crate) fn before_dispatch(error: Error) -> Error {
    Error::new(
        error.code,
        "swarmie.models.ai.not_dispatched",
        "model admission refused before HTTP dispatch",
    )
}
