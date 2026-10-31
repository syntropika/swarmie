/// Whether dispatch might have reached the provider. Unknown never grants replay.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Effect {
    NotDispatched,
    Unknown,
}

/// Stable failure categories. Error messages never contain credentials or payloads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Failure {
    Configuration,
    Credential,
    UnsupportedProfile,
    Metadata,
    Request,
    Admission,
    Expired,
    Runtime,
    Stopped,
    Timeout,
    Transport,
    Http(u16),
    Protocol,
    Provider,
    Limit,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("provider request failed: {failure:?} ({effect:?})")]
pub struct Error {
    pub failure: Failure,
    pub effect: Effect,
}

impl Error {
    pub(crate) fn before(failure: Failure) -> Self {
        Self {
            failure,
            effect: Effect::NotDispatched,
        }
    }

    pub(crate) fn unknown(failure: Failure) -> Self {
        Self {
            failure,
            effect: Effect::Unknown,
        }
    }
}
