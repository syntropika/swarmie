//! Native project content, independent of model execution, storage and scheduling.
//! All filesystem calls are synchronous. The embedding runs them on a blocking worker
//! after authorizing the selected roots and invoking principal.
mod capture;
mod files;
mod metadata;
mod skills;

pub use capture::{Configuration, Environment};
pub use files::{Limits, Scope};
pub use skills::{Diagnostic, Inventory, Selection, Skill};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("project source I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid project content: {0}")]
    Invalid(String),
    #[error("project content exceeds the configured limit")]
    Limit,
    #[error("selected content changed; refresh and select it again")]
    Changed,
    #[error("skill selection is ambiguous")]
    Ambiguous,
    #[error("selected skill is unavailable")]
    Unavailable,
    #[error("skill requires unsupported execution semantics")]
    Unsupported,
}

pub type Result<T> = std::result::Result<T, Error>;
