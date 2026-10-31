#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum Error {
    #[error("invalid account operation")]
    Invalid,
    #[error("account service unavailable")]
    Unavailable,
    #[error("protected account storage is unavailable")]
    ProtectedStorageUnavailable,
    #[error("sign-in is required")]
    SignInRequired,
    #[error("account service definitively refused the request")]
    Refused,
    #[error("account authorization ended")]
    Revoked,
    #[error("credential renewal outcome is unknown; sign in again")]
    RefreshUnknown,
    #[error("account identity or response failed validation")]
    Integrity,
    #[error("account action was refused")]
    Forbidden,
}

pub type Result<T> = std::result::Result<T, Error>;
