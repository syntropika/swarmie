//! Immutable, caller-defined history, independent of an agent loop or task scheduler.
//! A history point selects a fixed prefix. Writes acknowledge either preparation or
//! a confirmed commit; callers must preserve that distinction.

mod capability;
mod model;
mod orb;
mod storage;

pub use capability::Conversations;
pub use model::{
    Conversation, ConversationId, Entry, EntryDraft, EntryId, Head, HistoryPoint, Page, Read,
    Write, WriteConfirmation, WriteReceipt,
};
pub use orb::{INSTANCE_KEY, Method, SERVICE, orb};
pub use storage::{ConversationStorage, Rejection, StorageError, StorageFactory};
