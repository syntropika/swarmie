//! Shared write ownership, logical preparation and local change notifications.
//! A backend closes its owner only after the final lease and physical write settle.

mod changes;
mod owner;
mod scope;

pub use changes::{ChangeSource, Subscription};
pub use owner::{OwnerError, StorageOwner};
pub use scope::{PreparedChanges, WriteScope};
