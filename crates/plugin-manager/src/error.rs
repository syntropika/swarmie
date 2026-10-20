use tembra::RuntimeError;

/// An administrative refusal or an observed operation failure.
///
/// Runtime failures retain their original code, reason and structured details. An error
/// after runtime publication or filesystem/provider activity does not imply rollback.
#[derive(Debug, thiserror::Error)]
pub enum ManagerError {
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
    #[error("source provider is not approved by the supervisor: {0}")]
    UntrustedSource(String),
    #[error("source provider operation unavailable: {provider}/{operation}")]
    SourceUnavailable { provider: String, operation: String },
    #[error("source provider schema differs from its advertised operation: {provider}/{operation}")]
    SourceSchema { provider: String, operation: String },
    #[error(
        "plugin lifecycle is managed by the declared composition; replace and reconcile the composition instead"
    )]
    CompositionManaged,
    #[error("another administrative operation is in progress")]
    Busy,
    #[error(
        "initial installation failed: {cause}; cleanup error: {cleanup:?}; Host errors observed during cleanup: {cleanup_reports:?}; native facets may remain: {native_facets:?}; lingering physical executions: {lingering_physical}"
    )]
    StartupFailed {
        #[source]
        cause: Box<RuntimeError>,
        cleanup: Option<Box<RuntimeError>>,
        cleanup_reports: Vec<RuntimeError>,
        native_facets: Vec<String>,
        lingering_physical: usize,
    },
    #[error("operation failed: {failure}; recording its outcome also failed: {recording}")]
    FailureUnrecorded {
        failure: Box<ManagerError>,
        #[source]
        recording: Box<ManagerError>,
    },
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
    /// Whether safe continuation requires a fresh Host after effects that cannot be retired.
    pub fn requires_restart(&self) -> bool {
        match self {
            Self::StartupFailed {
                cleanup,
                cleanup_reports,
                native_facets,
                lingering_physical,
                ..
            } => {
                cleanup.is_some()
                    || !cleanup_reports.is_empty()
                    || !native_facets.is_empty()
                    || *lingering_physical > 0
            }
            Self::PublicationUnrecorded { .. }
            | Self::FailureUnrecorded { .. }
            | Self::RecoveryRequired => true,
            Self::Runtime(error) => matches!(
                error.reason.as_str(),
                "host_failed" | "closed" | "resource_restore"
            ),
            _ => false,
        }
    }

    pub(crate) fn into_runtime(self) -> RuntimeError {
        match self {
            Self::Runtime(error) => error,
            other => RuntimeError::invalid("plugin_administration", other.to_string()),
        }
    }
}

/// Native administrative result preserving typed failure categories.
pub type Result<T> = std::result::Result<T, ManagerError>;
