#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Reason {
    Invalid,
    Unavailable,
    OutcomeUnknown,
    Projection,
    Cancelled,
    Busy,
}

impl Reason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Invalid => "swarmie.mcp.invalid",
            Self::Unavailable => "swarmie.mcp.unavailable",
            Self::OutcomeUnknown => "swarmie.mcp.outcome_unknown",
            Self::Projection => "swarmie.mcp.projection",
            Self::Cancelled => "swarmie.mcp.cancelled",
            Self::Busy => "swarmie.mcp.busy",
        }
    }
}

pub(crate) fn invalid(message: impl ToString) -> tembra::Error {
    tembra::Error::invalid(Reason::Invalid.as_str(), message.to_string())
}

pub(crate) fn unavailable(message: &str) -> tembra::Error {
    tembra::Error::new(
        tembra::Code::Unavailable,
        Reason::Unavailable.as_str(),
        message,
    )
}

pub(crate) fn unknown(message: &str) -> tembra::Error {
    tembra::Error::new(
        tembra::Code::Unavailable,
        Reason::OutcomeUnknown.as_str(),
        message,
    )
}

pub(crate) fn projection(_: impl std::fmt::Display) -> tembra::Error {
    tembra::Error::new(
        tembra::Code::Unavailable,
        Reason::Projection.as_str(),
        "MCP outcome is retained but cannot be projected",
    )
}

pub(crate) fn cancelled() -> tembra::Error {
    tembra::Error::new(
        tembra::Code::Cancelled,
        Reason::Cancelled.as_str(),
        "MCP connection was cancelled before dispatch",
    )
}

pub(crate) fn busy() -> tembra::Error {
    tembra::Error::new(
        tembra::Code::Unavailable,
        Reason::Busy.as_str(),
        "MCP connection action is already in progress",
    )
}
