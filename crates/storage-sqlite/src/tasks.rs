//! Task storage interface over a leased SQLite connection.

mod commit;
mod records;

use crate::{SqliteOwner, owner::Lease};
use futures::{FutureExt, future::LocalBoxFuture};
use records::{META, metadata, read, sql_error, summary, time};
use rusqlite::params;
use std::path::Path;
use swarmie_durable_tasks::{
    CandidateSnapshot, CommitOutcome, StorageError, TaskChange, TaskId, TaskPageRequest,
    TaskRecord, TaskState, TaskStorage, TaskSummary,
};

/// Task records and assignments stored through one owner lease.
pub struct SqliteTaskStorage {
    pub(crate) lease: Lease,
}

impl SqliteTaskStorage {
    /// Opens task storage. Use [`SqliteOwner::tasks`] to share an owner.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        SqliteOwner::new(path.as_ref().to_path_buf()).tasks()
    }
}

impl TaskStorage for SqliteTaskStorage {
    fn owner(&self) -> swarmie_storage::StorageOwner {
        self.lease.database.health.clone()
    }

    fn changes(&self) -> swarmie_storage::ChangeSource {
        self.lease.database.tasks.clone()
    }

    fn get(&self, id: TaskId) -> LocalBoxFuture<'_, Result<Option<TaskRecord>, StorageError>> {
        async move {
            self.lease.check()?;
            let stored = self.lease.database.connection.borrow();
            let connection = stored.as_ref().ok_or(StorageError::Closed)?;
            read(connection, id)
        }
        .boxed_local()
    }

    fn list(
        &self,
        principal: Option<String>,
        page: TaskPageRequest,
    ) -> LocalBoxFuture<'_, Result<Vec<TaskSummary>, StorageError>> {
        async move {
            self.lease.check()?;
            let stored = self.lease.database.connection.borrow();
            let connection = stored.as_ref().ok_or(StorageError::Closed)?;
            let principal_filter = if principal.is_some() {
                "principal=?1"
            } else {
                "?1 IS NULL"
            };
            let sql = format!(
                "SELECT {META} FROM swarmie_tasks
                WHERE {principal_filter} AND sequence>?2 ORDER BY sequence LIMIT ?3"
            );
            let mut statement = connection.prepare(&sql).map_err(sql_error)?;
            let rows = statement
                .query_map(
                    params![
                        principal,
                        page.after,
                        page.limit.min(TaskPageRequest::MAX_LIMIT).get()
                    ],
                    metadata,
                )
                .map_err(sql_error)?;
            rows.map(|r| r.map_err(sql_error).and_then(summary))
                .collect()
        }
        .boxed_local()
    }

    fn candidates(
        &self,
        after: u64,
        limit: usize,
    ) -> LocalBoxFuture<'_, Result<CandidateSnapshot, StorageError>> {
        async move {
            self.lease.check()?;
            let mut stored = self.lease.database.connection.borrow_mut();
            let connection = stored.as_mut().ok_or(StorageError::Closed)?;
            let transaction = connection.transaction().map_err(sql_error)?;
            let now = time(&transaction)?;
            let sql = format!("SELECT {META} FROM swarmie_tasks WHERE state=?1
                AND (expires_at IS NULL OR expires_at<=?2) AND sequence>?3
                UNION ALL SELECT {META} FROM swarmie_tasks INDEXED BY swarmie_tasks_wake WHERE state='sleeping' AND wake_at<=?2
                AND (expires_at IS NULL OR expires_at<=?2) AND sequence>?3
                ORDER BY sequence LIMIT ?4");
            let wake_sql = "SELECT min(wake_at) FROM swarmie_tasks INDEXED BY swarmie_tasks_wake WHERE state='sleeping' AND wake_at>?1";
            let next_wake: Option<u64> = transaction.query_row(wake_sql,
                [now], |row| row.get(0)).map_err(sql_error)?;
            let mut statement = transaction.prepare(&sql).map_err(sql_error)?;
            let rows = statement
                .query_map(
                    params![TaskState::Ready.as_str(), now, after, limit as u64],
                    metadata,
                )
                .map_err(sql_error)?;
            let candidates = rows
                .map(|r| r.map_err(sql_error).and_then(summary))
                .collect::<Result<Vec<_>, _>>()?;
            drop(statement);
            transaction.commit().map_err(sql_error)?;
            Ok(CandidateSnapshot { now, candidates, next_wake })
        }
        .boxed_local()
    }

    fn commit(
        &self,
        changes: Vec<TaskChange>,
    ) -> LocalBoxFuture<'_, Result<CommitOutcome, StorageError>> {
        self.commit_batch(changes, vec![])
    }

    fn commit_with(
        &self,
        changes: Vec<TaskChange>,
        prepared: swarmie_storage::PreparedChanges,
    ) -> LocalBoxFuture<'_, Result<CommitOutcome, StorageError>> {
        async move {
            let domain = prepared
                .into_changes(&self.owner())
                .map_err(StorageError::Refused)?
                .into_iter()
                .map(|change| {
                    change
                        .downcast::<crate::SqliteWrite>()
                        .map(|change| *change)
                        .map_err(|_| {
                            StorageError::Refused(tembra::Error::new(
                                tembra::Code::Unsupported,
                                "swarmie.storage.joint_commit",
                                "SQLite cannot apply this staged command type",
                            ))
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            self.commit_batch(changes, domain).await
        }
        .boxed_local()
    }

    fn close(&self) -> LocalBoxFuture<'_, Result<(), StorageError>> {
        async move { self.lease.close().map_err(StorageError::Unavailable) }.boxed_local()
    }
}
