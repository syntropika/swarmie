use crate::{Output, State};
use serde::{Deserialize, Serialize};
use tembra::{Code, Error};

/// Ordinary execution retains both the original typed cause and confirmed progress.
/// An invalid initial Request or resumed State has no progress.
/// Pending operations must be reconciled on resume.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionFailure {
    #[serde(with = "error_wire")]
    pub error: Error,
    pub state: Option<Box<State>>,
}

pub type ExecutionResult = Result<Output, ExecutionFailure>;

impl std::fmt::Display for ExecutionFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(formatter)
    }
}

impl std::error::Error for ExecutionFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

/// Dispatch errors can occur after an effect. Only the provider can establish its outcome.
pub(crate) enum DriveError {
    Local(Error),
    Provider(Error),
}

impl From<Error> for DriveError {
    fn from(error: Error) -> Self {
        Self::Local(error)
    }
}

impl DriveError {
    pub fn cause(&self) -> &Error {
        match self {
            Self::Local(error) | Self::Provider(error) => error,
        }
    }

    pub fn original(self) -> Error {
        match self {
            Self::Local(error) | Self::Provider(error) => error,
        }
    }

    #[cfg(feature = "durable-tasks")]
    pub fn for_task(self) -> Error {
        match self {
            Self::Provider(error)
                if !matches!(
                    error.code,
                    Code::Cancelled | Code::Closed | Code::Unavailable
                ) =>
            {
                Error::new(
                    Code::Unavailable,
                    crate::FailureReason::RecoveryRequired.as_str(),
                    error.to_string(),
                )
            }
            other => other.original(),
        }
    }
}

pub(crate) mod error_wire {
    use super::*;

    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
        code: String,
        reason: String,
        message: String,
    }

    pub fn serialize<S: serde::Serializer>(
        error: &Error,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        Wire {
            code: error.code.as_str().into(),
            reason: error.reason.clone(),
            message: error.message.clone(),
        }
        .serialize(serializer)
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Error, D::Error> {
        let wire = Wire::deserialize(deserializer)?;
        let code = Code::parse(&wire.code)
            .ok_or_else(|| serde::de::Error::custom("unknown error code"))?;
        Ok(Error::new(code, wire.reason, wire.message))
    }
}
