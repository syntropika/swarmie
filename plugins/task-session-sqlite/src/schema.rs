use rusqlite::Connection;
use swarmie_sqlite_local::Schema;
use tembra::{Result, RuntimeError};

/// Current immutable operation ledger; task and session schemas retain their own ownership.
pub fn sqlite_schema() -> Schema {
    Schema {
        domain: "task-session-commit",
        version: 1,
        initialize,
        validate,
    }
}

pub(crate) fn io(error: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::unavailable("task_session_commit_io", error.to_string())
}

fn initialize(connection: &Connection) -> Result<()> {
    connection.execute_batch("CREATE TABLE task_session_commits(operation_id TEXT PRIMARY KEY,request TEXT NOT NULL,receipt TEXT NOT NULL) WITHOUT ROWID; CREATE TRIGGER immutable_task_session_commit_update BEFORE UPDATE ON task_session_commits BEGIN SELECT RAISE(ABORT,'immutable task session receipt'); END; CREATE TRIGGER immutable_task_session_commit_delete BEFORE DELETE ON task_session_commits BEGIN SELECT RAISE(ABORT,'retained task session receipt'); END;").map_err(io)
}

fn validate(connection: &Connection) -> Result<()> {
    connection
        .prepare("SELECT operation_id,request,receipt FROM task_session_commits LIMIT 0")
        .map_err(io)?;
    Ok(())
}
