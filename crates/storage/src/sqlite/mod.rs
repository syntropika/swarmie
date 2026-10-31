//! SQLite adapters for independent inventory, task and conversation capabilities.
//!
//! The owning file uses WAL and `synchronous=NORMAL`. Confirmed commits survive an
//! application crash; this setting does not promise persistence through power loss.
//! Synchronous SQLite work runs on the calling local executor in this first adapter.

mod conversations;
mod owner;
mod storage;
mod tasks;
mod vault;
mod write;

pub use conversations::SqliteConversationStorage;
pub use owner::SqliteOwner;
pub use storage::{SqliteStorage, factory};
pub use tasks::SqliteTaskStorage;
pub use vault::SqliteVaultStorage;
pub use write::SqliteWrite;

mod projects;
pub use projects::SqliteProjectsStorage;

mod activity;
pub use activity::SqliteActivityStorage;
