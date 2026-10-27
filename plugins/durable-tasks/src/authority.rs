use crate::{CommitOutcome, Rejection, TaskChange, TaskStorage};
use std::rc::Rc;
use tembra::{Code, Error};

#[derive(Clone, Copy, Debug)]
pub(crate) enum Reason {
    Storage,
    Conflict,
    Recovery,
    Closed,
    Scope,
    Busy,
    Strategy,
    Definition,
    Forbidden,
}

impl Reason {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Storage => "swarmie.tasks.storage",
            Self::Conflict => "swarmie.tasks.conflict",
            Self::Recovery => "swarmie.tasks.recovery_required",
            Self::Closed => "swarmie.tasks.closed",
            Self::Scope => "swarmie.tasks.scope",
            Self::Busy => "swarmie.tasks.busy",
            Self::Strategy => "swarmie.tasks.strategy",
            Self::Definition => "swarmie.tasks.definition",
            Self::Forbidden => "swarmie.tasks.forbidden",
        }
    }
}

pub(crate) fn error(code: Code, reason: Reason, message: impl ToString) -> Error {
    Error::new(code, reason.as_str(), message.to_string())
}

pub(crate) fn storage_error(cause: crate::StorageError) -> Error {
    use crate::StorageError;
    use swarmie_storage_core::OwnerError;
    match cause {
        StorageError::Owner(OwnerError::Closed) | StorageError::Closed => {
            error(Code::Closed, Reason::Closed, cause)
        }
        StorageError::Owner(OwnerError::RecoveryRequired) => {
            error(Code::Unavailable, Reason::Recovery, cause)
        }
        StorageError::Refused(error) => error,
        StorageError::Invalid(_) => error(Code::Invalid, Reason::Storage, cause),
        StorageError::Unavailable(_) => error(Code::Unavailable, Reason::Storage, cause),
    }
}

pub(crate) struct Authority {
    pub storage: Rc<dyn TaskStorage>,
}

impl Authority {
    pub fn new(storage: Rc<dyn TaskStorage>) -> Self {
        Self { storage }
    }

    pub fn check(&self) -> tembra::Result<()> {
        self.storage
            .owner()
            .check()
            .map_err(|e| storage_error(e.into()))
    }

    pub async fn commit(
        &self,
        changes: Vec<TaskChange>,
    ) -> tembra::Result<Result<Vec<crate::AssignmentGrant>, Rejection>> {
        self.check()?;
        self.confirm(self.storage.commit(changes).await)
    }
    pub(crate) fn confirm(
        &self,
        outcome: Result<CommitOutcome, crate::StorageError>,
    ) -> tembra::Result<Result<Vec<crate::AssignmentGrant>, Rejection>> {
        match outcome.map_err(storage_error)? {
            CommitOutcome::Applied { grants } => Ok(Ok(grants)),
            CommitOutcome::Rejected(reason) => Ok(Err(reason)),
            CommitOutcome::Unknown { message } => {
                self.storage.owner().require_recovery();
                Err(error(Code::Unavailable, Reason::Recovery, message))
            }
        }
    }
}
