//! Independently usable Git capability. Synchronous calls must run on a host worker.
//! The host supplies authorization, durable operation storage and process execution.

#[cfg(feature = "tembra")]
mod orb;

#[cfg(feature = "tools")]
pub use orb::tool_definitions;
#[cfg(feature = "tembra")]
pub use orb::{INSTANCE_KEY, SERVICE, orb};

#[cfg(feature = "plugin-manager")]
mod plugin;
#[cfg(feature = "plugin-manager")]
pub use plugin::{PLUGIN_ID, core_plugin};

mod archive;
mod domain;
mod journal;
mod objects;
mod operations;
mod persistence;
mod process;
mod repository;
mod service;
mod settings;
mod worktrees;

pub use archive::{
    ArchiveDirectory, ArchiveListing, ArchiveReceipt, ArchiveRestoreRequest, ArchiveUnavailable,
    CleanupEligibility,
};
pub use domain::*;
pub use journal::{
    Authority, DirectoryJournal, Journal, JournalEntry, JournalScan, JournalUnavailable,
};
pub use operations::Git;
pub use process::{Cancellation, CommandOutput, CommandSpec, NativeProcess, ProcessExecutor};
pub use repository::parse_status;
pub use service::{
    ArchiveState, ConversationHold, ConversationHoldUpdate, GitService, ManagedRequest,
    ManagedResponse, WireRequest, WireResponse,
};
pub use settings::{DiffLayout, GitSettings, SetupScript};
pub use worktrees::{CreatedWorktree, SetupOutcome, WorktreeRequest};

/// Failures before a confirmed Git outcome are distinct from operation receipts.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("execution did not start: {0}")]
    NotStarted(String),
    #[error("invalid request: {0}")]
    Invalid(String),
    #[error("Git failed ({status}): {message}")]
    Command { status: i32, message: String },
    #[error("the checkout changed; refresh before continuing")]
    Stale,
    #[error("permission denied: {0}")]
    Denied(String),
    #[error("operation cancelled after its process settled")]
    Cancelled,
    #[error("process execution exceeded its time limit")]
    Timeout,
    #[error("Git output exceeded the configured limit")]
    OutputLimit,
    #[error("journal error: {0}")]
    Journal(String),
}

pub type Result<T> = std::result::Result<T, Error>;
