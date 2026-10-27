use crate::{Conversation, ConversationId, Page, Read, Write, WriteConfirmation};
use futures::future::LocalBoxFuture;
use serde::{Deserialize, Serialize};
use std::rc::Rc;
use tembra::{Code, Context, Error};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum Rejection {
    #[error("conversation or entry does not exist")]
    NotFound,
    #[error("identity is already committed")]
    AlreadyExists,
    #[error("conversation head changed")]
    Conflict,
    #[error("conversation belongs to another principal")]
    Forbidden,
    #[error("invalid history selection or write")]
    Invalid,
    #[error("history position exhausted")]
    Exhausted,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum StorageError {
    #[error(transparent)]
    Owner(#[from] swarmie_storage_core::OwnerError),
    #[error(transparent)]
    Refused(Rejection),
    #[error("conversation storage is closed")]
    Closed,
    #[error("conversation storage is unavailable: {0}")]
    Unavailable(String),
    #[error("invalid conversation data: {0}")]
    Invalid(String),
    #[error("conversation commit acknowledgement is unknown: {0}")]
    Unknown(String),
}

impl From<StorageError> for Error {
    fn from(error: StorageError) -> Self {
        let code = match &error {
            StorageError::Refused(Rejection::NotFound) => Code::Invalid,
            StorageError::Refused(Rejection::AlreadyExists | Rejection::Conflict) => Code::Conflict,
            StorageError::Refused(Rejection::Forbidden) => Code::Forbidden,
            StorageError::Refused(Rejection::Invalid) | StorageError::Invalid(_) => Code::Invalid,
            StorageError::Refused(Rejection::Exhausted) => Code::Exhausted,
            StorageError::Closed
            | StorageError::Owner(swarmie_storage_core::OwnerError::Closed) => Code::Closed,
            _ => Code::Unavailable,
        };
        Error::new(code, Reason::Storage.as_str(), error.to_string())
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Reason {
    Storage,
    Instance,
}

impl Reason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Storage => "swarmie.conversations.storage",
            Self::Instance => "swarmie.conversations.instance",
        }
    }
}

/// Backend-neutral history capability. Implementations authorize every referenced
/// conversation, preserve immutable payloads, and return bounded ordered pages.
/// They may replace the physical history representation without changing callers.
pub trait ConversationStorage {
    fn owner(&self) -> swarmie_storage_core::StorageOwner;

    fn get<'a>(
        &'a self,
        principal: &'a str,
        id: ConversationId,
    ) -> LocalBoxFuture<'a, Result<Option<Conversation>, StorageError>>;

    fn read<'a>(
        &'a self,
        principal: &'a str,
        request: Read,
    ) -> LocalBoxFuture<'a, Result<Page, StorageError>>;

    /// Under a valid shared write scope, stage only. With no scope, confirm one
    /// independent transaction. Foreign or expired scopes must never fall back
    /// to an independent commit. Validate expected heads inside the transaction.
    fn write<'a>(
        &'a self,
        context: &'a Context,
        write: Write,
    ) -> LocalBoxFuture<'a, tembra::Result<WriteConfirmation>>;

    /// Release this capability only. Its owner outlives other active domain leases.
    fn close(&self) -> LocalBoxFuture<'_, Result<(), StorageError>>;
}

pub type StorageFactory =
    Rc<dyn Fn() -> LocalBoxFuture<'static, Result<Rc<dyn ConversationStorage>, StorageError>>>;
