#![cfg(feature = "sqlite")]

use futures::executor::block_on;
use std::num::NonZeroU32;
use swarmie_plugin_manager::{
    CommitOutcome, InstalledPlugin, PluginChange, PluginId, PluginStorage, StorageError,
};
use swarmie_storage::sqlite::SqliteStorage;

fn plugin(id: &str) -> InstalledPlugin {
    InstalledPlugin {
        id: PluginId::new(id).unwrap(),
        version: NonZeroU32::new(1).unwrap(),
        enabled: true,
    }
}

#[test]
fn revision_checked_batches_are_atomic_and_survive_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("runtime.sqlite");
    let owner = SqliteStorage::open(&file).unwrap();
    let other = SqliteStorage::open(&file).unwrap();
    block_on(async {
        assert_eq!(
            owner.commit(0, vec![]).await.unwrap(),
            CommitOutcome::Applied { revision: 0 }
        );
        assert_eq!(
            owner
                .commit(
                    0,
                    vec![
                        PluginChange::Put(plugin("one")),
                        PluginChange::Put(plugin("two"))
                    ]
                )
                .await
                .unwrap(),
            CommitOutcome::Applied { revision: 1 }
        );
        assert_eq!(
            other
                .commit(
                    0,
                    vec![
                        PluginChange::Remove(PluginId::new("one").unwrap()),
                        PluginChange::Put(plugin("three"))
                    ]
                )
                .await
                .unwrap(),
            CommitOutcome::Rejected {
                current_revision: 1
            }
        );
        let inventory = other.inventory().await.unwrap();
        assert_eq!(inventory.plugins, vec![plugin("one"), plugin("two")]);
        assert_eq!(inventory.revision, 1);
        owner.close().await.unwrap();
        other.close().await.unwrap();
    });
    let reopened = SqliteStorage::open(&file).unwrap();
    assert_eq!(
        block_on(reopened.inventory()).unwrap().plugins,
        vec![plugin("one"), plugin("two")]
    );
    block_on(reopened.close()).unwrap();
}

#[test]
fn sql_failure_rolls_back_the_entire_batch_and_revision() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("runtime.sqlite");
    let owner = SqliteStorage::open(&file).unwrap();
    let connection = rusqlite::Connection::open(&file).unwrap();
    connection.execute_batch("CREATE TRIGGER reject_second BEFORE INSERT ON swarmie_plugins WHEN NEW.id='second' BEGIN SELECT RAISE(ABORT,'injected failure'); END;").unwrap();
    let result = block_on(owner.commit(
        0,
        vec![
            PluginChange::Put(plugin("first")),
            PluginChange::Put(plugin("second")),
        ],
    ));
    assert!(matches!(result, Err(StorageError::InvalidInventory(_))));
    let inventory = block_on(owner.inventory()).unwrap();
    assert_eq!(inventory.revision, 0);
    assert!(inventory.plugins.is_empty());
    block_on(owner.close()).unwrap();
}

#[test]
fn invalid_inventory_is_reported_instead_of_becoming_an_empty_installation() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("runtime.sqlite");
    let owner = SqliteStorage::open(&file).unwrap();
    let connection = rusqlite::Connection::open(&file).unwrap();
    connection
        .execute("INSERT INTO swarmie_plugins VALUES('invalid id',1,1)", [])
        .unwrap();
    assert!(matches!(
        block_on(owner.inventory()),
        Err(StorageError::InvalidInventory(_))
    ));
    block_on(owner.close()).unwrap();
}

#[test]
fn closing_is_idempotent_and_refuses_later_reads_and_writes() {
    let directory = tempfile::tempdir().unwrap();
    let owner = SqliteStorage::open(directory.path().join("runtime.sqlite")).unwrap();
    block_on(async {
        owner.close().await.unwrap();
        owner.close().await.unwrap();
        assert_eq!(
            owner.inventory().await,
            Err(StorageError::Owner(swarmie_storage::OwnerError::Closed))
        );
        assert_eq!(
            owner.commit(0, vec![]).await,
            Err(StorageError::Owner(swarmie_storage::OwnerError::Closed))
        );
    });
}

#[test]
fn unsupported_schema_is_not_migrated_or_overwritten() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("runtime.sqlite");
    let owner = SqliteStorage::open(&file).unwrap();
    block_on(owner.close()).unwrap();
    let connection = rusqlite::Connection::open(&file).unwrap();
    connection
        .execute("UPDATE swarmie_plugins_meta SET schema_version=999", [])
        .unwrap();
    assert!(matches!(
        SqliteStorage::open(&file),
        Err(StorageError::InvalidInventory(_))
    ));
    assert_eq!(
        connection
            .query_row(
                "SELECT schema_version FROM swarmie_plugins_meta",
                [],
                |row| row.get::<_, u32>(0)
            )
            .unwrap(),
        999
    );
}
