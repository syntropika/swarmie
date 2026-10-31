use crate::sqlite::SqliteOwner;
use swarmie_storage_core::ChangeSource;
use tembra::Context;

type Apply = Box<dyn FnOnce(&rusqlite::Transaction<'_>) -> tembra::Result<()>>;

/// A trusted SQLite adapter command, staged without opening a database transaction.
/// Intended for storage providers; task authors use ordinary domain orbs.
/// Commands may validate and modify domain tables inside the supplied transaction.
/// They must not change task-owned tables, control transactions, publish state,
/// invoke user callbacks or perform external effects. Failure rolls back the batch.
pub struct SqliteWrite {
    apply: Option<Apply>,
    changes: Option<ChangeSource>,
}

impl SqliteWrite {
    /// Creates a domain-table command for a later atomic commit.
    /// The command must not change task tables, publish state or perform external effects.
    pub fn new(
        apply: impl FnOnce(&rusqlite::Transaction<'_>) -> tembra::Result<()> + 'static,
    ) -> Self {
        Self {
            apply: Some(Box::new(apply)),
            changes: None,
        }
    }

    /// Publishes a coalescible invalidation only after a confirmed joint commit,
    /// after releasing the connection borrow. Unknown outcomes do not publish success.
    pub fn notifies(mut self, changes: ChangeSource) -> Self {
        self.changes = Some(changes);
        self
    }

    pub(crate) fn apply(&mut self, transaction: &rusqlite::Transaction<'_>) -> tembra::Result<()> {
        self.apply
            .take()
            .expect("a command is consumed exactly once")(transaction)
    }

    pub(crate) fn notify(self) {
        if let Some(changes) = self.changes {
            changes.invalidate();
        }
    }
}

impl SqliteOwner {
    /// Acknowledges staging only; the encompassing execution confirms durability.
    /// This adapter does not infer an independent transaction when the scope is absent.
    pub fn stage(&self, context: &Context, command: SqliteWrite) -> tembra::Result<()> {
        self.authority().stage(context, command)
    }
}
