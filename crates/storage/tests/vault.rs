#![cfg(feature = "sqlite")]

use swarmie_storage::sqlite::SqliteVaultStorage;
use swarmie_vault::{Error, VaultIdentity, VaultSnapshot, VaultStorage};
use tempfile::tempdir;
use uuid::Uuid;

fn identity() -> VaultIdentity {
    VaultIdentity {
        account: Uuid::new_v4(),
        vault: Uuid::new_v4(),
        epoch: Uuid::new_v4(),
    }
}

#[test]
fn journal_survives_reopen_and_separates_account_roots() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("vault.sqlite");
    let id = identity();
    let store = SqliteVaultStorage::open(&path).unwrap();
    let snapshot = VaultSnapshot {
        revision: 1,
        authority_cursor: 42,
        ..Default::default()
    };
    store.compare_exchange(id, 0, &snapshot).unwrap();
    store.close().unwrap();
    let reopened = SqliteVaultStorage::open(&path).unwrap();
    assert_eq!(reopened.load(id).unwrap().authority_cursor, 42);
    assert_eq!(reopened.load(identity()).unwrap().revision, 0);
}

#[test]
fn independent_connections_compare_and_exchange_one_namespace() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("vault.sqlite");
    let id = identity();
    let a = SqliteVaultStorage::open(&path).unwrap();
    let b = SqliteVaultStorage::open(&path).unwrap();
    let snapshot = VaultSnapshot {
        revision: 1,
        ..Default::default()
    };
    a.compare_exchange(id, 0, &snapshot).unwrap();
    assert_eq!(b.compare_exchange(id, 0, &snapshot), Err(Error::Stale));
    assert_eq!(b.load(id).unwrap().revision, 1);
}
