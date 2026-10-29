use serde::{Deserialize, Serialize};

/// Captured checkout and content identity. Selection is not authorization.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RepositoryTarget {
    pub root: Option<String>,
    pub git_dir: String,
    pub common_dir: String,
    pub bare: bool,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub fingerprint: String,
    pub index_fingerprint: String,
    /// Missing content identity never authorizes a mutation, even after rediscovery.
    pub observation_error: Option<ObservationError>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationError {
    OutputLimit,
}

/// Continuation bound to the complete serialized inventory, not just its offset.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PageCursor {
    pub offset: usize,
    pub digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Request {
    Discover {
        path: String,
    },
    Status {
        target: RepositoryTarget,
        page: Option<PageCursor>,
    },
    Diff {
        target: RepositoryTarget,
        path: Option<String>,
        staged: bool,
        mode: DiffMode,
    },
    History {
        target: RepositoryTarget,
        limit: u32,
    },
    Remotes {
        target: RepositoryTarget,
    },
    Files {
        target: RepositoryTarget,
        page: Option<PageCursor>,
    },
    File {
        target: RepositoryTarget,
        path: String,
        revision: Option<String>,
    },
    Mutate {
        operation_id: String,
        target: RepositoryTarget,
        operation: Operation,
    },
    Recover {
        operation_id: String,
    },
    Operations {
        target: RepositoryTarget,
        page: Option<PageCursor>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Response {
    Repository {
        repository: Option<RepositoryTarget>,
    },
    Status {
        status: Status,
        next_page: Option<PageCursor>,
    },
    Diff {
        diff: Diff,
    },
    History {
        commits: Vec<Commit>,
    },
    Remotes {
        remotes: Vec<Remote>,
    },
    Files {
        paths: Vec<String>,
        next_page: Option<PageCursor>,
    },
    File {
        text: String,
        truncated: bool,
    },
    Receipt {
        receipt: Box<Receipt>,
    },
    Operations {
        operations: Vec<RecoveryEntry>,
        unavailable: Vec<crate::JournalUnavailable>,
        next_page: Option<PageCursor>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Operation {
    /// Owner-defined facade operations are admitted through execute_extension only.
    Extension {
        name: String,
        arguments: serde_json::Value,
    },
    Stage {
        paths: Vec<String>,
    },
    Unstage {
        paths: Vec<String>,
    },
    Commit {
        message: String,
        reviewed_index: String,
        /// Capture the effective hook choice before authorization and admission.
        #[serde(default = "stored_commit_hooks")]
        run_hooks: bool,
    },
    CreateBranch {
        name: String,
    },
    Fetch {
        remote: String,
    },
    Push {
        remote: String,
        source_oid: String,
        destination_ref: String,
    },
}

// Commit records admitted before this field existed used native hooks. Retain
// that meaning when reading the user's journal; new callers specify the choice.
fn stored_commit_hooks() -> bool {
    true
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffMode {
    Unified,
    Split,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Status {
    pub target: RepositoryTarget,
    pub entries: Vec<StatusEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StatusEntry {
    pub path: String,
    pub original_path: Option<String>,
    pub index: String,
    pub worktree: String,
    pub kind: EntryKind,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Tracked,
    Untracked,
    Ignored,
    Unmerged,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Diff {
    pub text: String,
    pub truncated: bool,
    pub mode: DiffMode,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Commit {
    pub oid: String,
    pub parents: Vec<String>,
    pub author: String,
    pub authored_at: String,
    pub subject: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Remote {
    pub name: String,
    pub fetch_urls: Vec<String>,
    pub push_urls: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Intent {
    pub target: RepositoryTarget,
    pub operation: IntentOperation,
}

/// Exhaustive host authorization values. Arguments retain their native types.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IntentOperation {
    Discover,
    Status,
    Diff {
        path: Option<String>,
        staged: bool,
        mode: DiffMode,
    },
    History {
        limit: u32,
    },
    Remotes,
    Files,
    File {
        path: String,
        revision: Option<String>,
    },
    Recover {
        operation_id: String,
        operation: Operation,
    },
    Operations,
    Mutate {
        operation: Operation,
    },
    Managed {
        request: Box<crate::ManagedRequest>,
    },
    CreateWorktree {
        request: crate::WorktreeRequest,
    },
    Setup {
        script: crate::SetupScript,
    },
    ArchiveCapture {
        archive_id: String,
    },
    ArchiveRestore {
        archive_id: String,
        managed_root: std::path::PathBuf,
        destination: std::path::PathBuf,
    },
    CleanupCheck {
        archive_id: String,
        pinned: bool,
        active_writers: usize,
    },
    ArchiveList,
    ConversationHold {
        conversation_id: String,
        pinned: bool,
        archived: bool,
        active: bool,
    },
}

/// Complete identity and arguments for a facade-owned durable operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExtensionOperation {
    pub intent: IntentOperation,
    pub operation_id: String,
    pub target: RepositoryTarget,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingOperation {
    pub operation_id: String,
    pub principal: String,
    pub target: RepositoryTarget,
    pub operation: Operation,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt {
    pub result: Option<serde_json::Value>,
    pub operation: Operation,
    pub operation_id: String,
    pub principal: String,
    pub target: RepositoryTarget,
    pub state: Outcome,
    pub message: String,
    pub head: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    InProgress,
    Completed,
    Failed,
    Unknown,
}

/// Read-only observations do not prove which process caused a repository state.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecoveryInspection {
    pub repository: RepositoryTarget,
    pub head_matches_admission: bool,
    pub index_matches_admission: bool,
    pub remote_ref: Option<RemoteRefInspection>,
    pub causality_confirmed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteRefInspection {
    pub remote: String,
    pub destination_ref: String,
    pub observed_oid: Option<String>,
    pub matches_requested_source: bool,
    pub endpoint_matches_admission: Option<bool>,
    pub error: Option<String>,
}

/// Bounded catalog projection. Full admitted arguments remain in the journal.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecoveryEntry {
    pub operation_id: String,
    pub principal: String,
    pub target: RepositoryTarget,
    pub state: Outcome,
    pub message: String,
    pub head: Option<String>,
    pub operation_digest: String,
}
