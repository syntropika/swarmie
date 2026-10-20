use tembra::RuntimeError;

/// An administrative refusal or an observed operation failure.
///
/// Runtime failures retain their original code, reason and structured details. An error
/// after runtime publication or filesystem/provider activity does not imply rollback.
#[derive(Debug, thiserror::Error)]
pub enum ManagerError {
    #[error("administrative capability is absent or invalid")]
    Unauthorized,
    #[error("explicit installation confirmation is required")]
    ConfirmationRequired,
    #[error("invalid package: {0}")]
    InvalidPackage(String),
    #[error("package integrity mismatch: {0}")]
    Integrity(String),
    #[error("runtime compatibility refused: {0}")]
    Compatibility(String),
    #[error("dependency refused: {0}")]
    Dependency(String),
    #[error("plugin not installed: {0}")]
    NotInstalled(String),
    #[error("compiled core plugins cannot be modified: {0}")]
    CorePlugin(String),
    #[error("source provider operation unavailable: {provider}/{operation}")]
    SourceUnavailable { provider: String, operation: String },
    #[error("source provider schema differs from its advertised operation: {provider}/{operation}")]
    SourceSchema { provider: String, operation: String },
    #[error("another administrative operation is in progress")]
    Busy,
    #[error(
        "runtime publication committed but durable selection failed for {plugin_id}@{version}: {cause}"
    )]
    PublicationUnrecorded {
        plugin_id: String,
        version: String,
        #[source]
        cause: Box<ManagerError>,
    },
    #[error(
        "runtime and durable selection require a fresh supervisor session before further mutations"
    )]
    RecoveryRequired,
    #[error("durable administrative state is closed")]
    Closed,
    #[error("sqlite: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("serialization: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("runtime: {0}")]
    Runtime(#[from] RuntimeError),
    #[error("source filesystem: {0}")]
    Io(#[from] std::io::Error),
}

impl ManagerError {
    pub(crate) fn into_runtime(self) -> RuntimeError {
        match self {
            Self::Runtime(error) => error,
            Self::Unauthorized => RuntimeError::new(
                tembra::core::ErrorCode::Forbidden,
                "administration",
                "administrative capability is absent or invalid",
            ),
            other => RuntimeError::invalid("plugin_administration", other.to_string()),
        }
    }
}

/// Native administrative result preserving typed failure categories.
pub type Result<T> = std::result::Result<T, ManagerError>;
