use serde::{Deserialize, Serialize};

/// Deliberately payload-free: errors must never reflect a credential or upstream body.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum Error {
    #[error("invalid vault input")]
    Invalid,
    #[error("vault is locked")]
    Locked,
    #[error("authenticated owner is required")]
    Forbidden,
    #[error("human decision is required")]
    ApprovalRequired,
    #[error("authority has ended or changed")]
    Revoked,
    #[error("Cloud definitively refused this request")]
    CloudRefused,
    #[error("cloud confirmation is unavailable")]
    Offline,
    #[error("sign in again to restore Cloud confirmation")]
    SignInRequired,
    #[error("protected storage is unavailable")]
    Unavailable,
    #[error("authentication or ciphertext verification failed")]
    Integrity,
    #[error("expected revision changed")]
    Stale,
    #[error("secret name is already in use")]
    NameConflict,
    #[error("vault is not current")]
    VaultNotCurrent,
    #[error("root is known exposed")]
    RootExposed,
    #[error("outcome must be recognized before another attempt")]
    Unknown,
    #[error("configured resource limit exceeded")]
    Exhausted,
    #[error("secret or operation does not exist")]
    NotFound,
    #[error("unsupported credential delivery")]
    Unsupported,
}

pub type Result<T> = std::result::Result<T, Error>;
