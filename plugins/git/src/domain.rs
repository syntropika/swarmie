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
    pub operation: String,
}

/// Complete identity and arguments for a facade-owned durable operation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExtensionOperation {
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
    Completed,
    Failed,
    Unknown,
}
