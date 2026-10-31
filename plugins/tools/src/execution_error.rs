use std::{error::Error, fmt, sync::Arc};

/// Classification for presentation and diagnostics; never retry authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolErrorKind {
    InvalidArgs,
    PermissionDenied,
    Timeout,
    Cancelled,
    NotFound,
    RateLimited,
    Provider,
    Network,
    Other,
}

/// An author's failure with separate model feedback and operator diagnostics.
/// An arbitrary source is uncertain and redacted. Only an explicit confirmed
/// failure or refusal becomes a terminal model result; uncertain effects must
/// be reconciled by the tool's recovery implementation.
#[derive(Clone)]
pub struct ToolExecutionError {
    kind: ToolErrorKind,
    message: String,
    feedback: String,
    confirmed: bool,
    refusal: bool,
    source: Option<Arc<dyn Error + Send + Sync>>,
}

impl ToolExecutionError {
    /// A confirmed failure, authored with deliberately model-visible feedback.
    pub fn failed(kind: ToolErrorKind, feedback: impl Into<String>) -> Self {
        let feedback = feedback.into();
        Self {
            kind,
            message: feedback.clone(),
            feedback,
            confirmed: true,
            refusal: false,
            source: None,
        }
    }

    pub fn invalid_args(feedback: impl Into<String>) -> Self {
        Self::failed(ToolErrorKind::InvalidArgs, feedback)
    }

    pub fn refused(feedback: impl Into<String>) -> Self {
        Self {
            refusal: true,
            ..Self::failed(ToolErrorKind::PermissionDenied, feedback)
        }
    }

    /// Preserve the source for direct diagnostics without sending it to a model.
    /// Transport failure does not establish whether an external effect happened.
    pub fn from_error<E: Error + Send + Sync + 'static>(error: E) -> Self {
        let source: Box<dyn Error + Send + Sync> = Box::new(error);
        match source.downcast::<Self>() {
            Ok(error) => *error,
            Err(source) => Self {
                kind: ToolErrorKind::Other,
                message: source.to_string(),
                feedback: "tool outcome is unknown; reconciliation is required".into(),
                confirmed: false,
                refusal: false,
                source: Some(Arc::from(source)),
            },
        }
    }

    pub fn with_model_feedback(mut self, feedback: impl Into<String>) -> Self {
        self.feedback = feedback.into();
        self
    }

    /// An uncertain failure with an operator-only diagnostic.
    pub fn other(message: impl Into<String>) -> Self {
        Self::from_error(std::io::Error::other(message.into()))
    }

    pub fn not_found(feedback: impl Into<String>) -> Self {
        Self::failed(ToolErrorKind::NotFound, feedback)
    }

    pub fn permission_denied(feedback: impl Into<String>) -> Self {
        Self::failed(ToolErrorKind::PermissionDenied, feedback)
    }

    pub fn with_source<E: Error + Send + Sync + 'static>(mut self, source: E) -> Self {
        self.source = Some(Arc::new(source));
        self
    }

    pub const fn kind(&self) -> ToolErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn model_feedback(&self) -> &str {
        &self.feedback
    }

    pub const fn is_confirmed(&self) -> bool {
        self.confirmed
    }

    pub const fn is_refusal(&self) -> bool {
        self.refusal
    }

    pub fn downcast_ref<E: Error + Send + Sync + 'static>(&self) -> Option<&E> {
        self.source.as_ref()?.downcast_ref()
    }
}

impl fmt::Display for ToolExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl fmt::Debug for ToolExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolExecutionError")
            .field("kind", &self.kind)
            .field("confirmed", &self.confirmed)
            .field("refusal", &self.refusal)
            .finish_non_exhaustive()
    }
}

impl Error for ToolExecutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn Error + 'static))
    }
}
