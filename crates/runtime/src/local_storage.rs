//! Composition of the node's physical SQLite owner before administrative recovery.

use std::path::Path;

use swarmie_sqlite_local::{DatabaseHandle, SqliteLocal, SqliteLocalInput};
use tembra::{Host, core};

pub(crate) async fn install(host: &Host, path: &Path) -> tembra::Result<DatabaseHandle> {
    let schemas = vec![
        swarmie_task_storage_sqlite::sqlite_schema(),
        swarmie_session_store_sqlite::sqlite_schema(),
        swarmie_task_session_integration::sqlite_schema(),
        swarmie_task_session_sqlite::sqlite_schema(),
        swarmie_memory_store::sqlite_schema(),
        swarmie_plugin_manager::sqlite_schema(),
    ];
    let (facet, database) = swarmie_sqlite_local::facet(
        &swarmie_sqlite_local::Configuration {
            database: path.to_path_buf(),
        },
        schemas,
    )?;
    host.install(facet, core::LoadOptions::default())?.await?;
    let owner = host.get(SqliteLocal, SqliteLocalInput {}).await?;
    owner.identity().await?;
    Ok(database)
}
