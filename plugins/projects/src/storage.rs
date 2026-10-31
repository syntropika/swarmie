use crate::Snapshot;
use futures::future::LocalBoxFuture;
use std::rc::Rc;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error(transparent)]
    Owner(#[from] swarmie_storage_core::OwnerError),
    #[error("storage is closed")]
    Closed,
    #[error("storage is unavailable: {0}")]
    Unavailable(String),
    #[error("stored projects are invalid: {0}")]
    Invalid(String),
    #[error("the data changed; refresh before retrying")]
    Conflict,
    #[error("commit outcome is unknown; read before making another change: {0}")]
    Unknown(String),
}

impl From<StorageError> for tembra::Error {
    fn from(error: StorageError) -> Self {
        let code = match &error {
            StorageError::Conflict => tembra::Code::Conflict,
            StorageError::Invalid(_) => tembra::Code::Invalid,
            _ => tembra::Code::Unavailable,
        };
        Self::new(code, "swarmie.projects.storage", error.to_string())
    }
}

/// Principal-scoped storage. A successful replacement commits the complete snapshot.
/// A rejected revision leaves data unchanged. An unknown commit must not be replayed.
pub trait Storage {
    fn load<'a>(&'a self, principal: &'a str)
    -> LocalBoxFuture<'a, Result<Snapshot, StorageError>>;

    fn replace<'a>(
        &'a self,
        principal: &'a str,
        expected: u64,
        next: Snapshot,
    ) -> LocalBoxFuture<'a, Result<(), StorageError>>;

    fn close(&self) -> LocalBoxFuture<'_, Result<(), StorageError>>;
}

/// The embedding opens storage only during Tembra resource activation.
pub type StorageFactory =
    Rc<dyn Fn() -> LocalBoxFuture<'static, Result<Rc<dyn Storage>, StorageError>>>;
