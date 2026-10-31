use super::{SqliteOwner, owner::Lease};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use std::path::Path;
use swarmie_vault::{Error, Result, VaultIdentity, VaultSnapshot, VaultStorage};

/// Ciphertext-only vault snapshot and issuance journal, isolated by account/root/epoch.
/// WAL/NORMAL is process-crash durable; power-loss persistence is not promised.
pub struct SqliteVaultStorage {
    pub(crate) lease: Lease,
}

impl SqliteVaultStorage {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        SqliteOwner::new(path.as_ref().to_path_buf()).vault()
    }

    pub fn close(&self) -> Result<()> {
        self.lease.close().map_err(|_| Error::Unavailable)
    }

    fn check(&self) -> Result<()> {
        self.lease.check().map_err(|_| Error::Unavailable)?;
        self.lease
            .database
            .health
            .check()
            .map_err(|_| Error::Unknown)
    }
}

impl VaultStorage for SqliteVaultStorage {
    fn health(&self) -> Result<()> {
        self.check()
    }

    fn load(&self, identity: VaultIdentity) -> Result<VaultSnapshot> {
        self.check()?;
        let stored = self.lease.database.connection.borrow();
        let connection = stored.as_ref().ok_or(Error::Unavailable)?;
        let row = connection.query_row(
            "SELECT revision,snapshot FROM swarmie_vaults WHERE account=?1 AND vault=?2 AND epoch=?3",
            params![identity.account.to_string(), identity.vault.to_string(), identity.epoch.to_string()],
            |r| Ok((r.get::<_, u64>(0)?, r.get::<_, Vec<u8>>(1)?)),
        ).optional().map_err(|_| Error::Unavailable)?;
        let Some((revision, bytes)) = row else {
            return Ok(VaultSnapshot::default());
        };
        let snapshot: VaultSnapshot =
            serde_json::from_slice(&bytes).map_err(|_| Error::Integrity)?;
        if snapshot.revision != revision || !valid_namespace(identity, &snapshot) {
            return Err(Error::Integrity);
        }
        Ok(snapshot)
    }

    fn compare_exchange(
        &self,
        identity: VaultIdentity,
        expected: u64,
        next: &VaultSnapshot,
    ) -> Result<()> {
        self.check()?;
        if next.revision != expected.checked_add(1).ok_or(Error::Exhausted)?
            || next.revision > i64::MAX as u64
            || !valid_namespace(identity, next)
        {
            return Err(Error::Invalid);
        }
        let bytes = serde_json::to_vec(next).map_err(|_| Error::Invalid)?;
        let mut stored = self.lease.database.connection.borrow_mut();
        let connection = stored.as_mut().ok_or(Error::Unavailable)?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|_| Error::Unavailable)?;
        let current = tx
            .query_row(
                "SELECT revision FROM swarmie_vaults WHERE account=?1 AND vault=?2 AND epoch=?3",
                params![
                    identity.account.to_string(),
                    identity.vault.to_string(),
                    identity.epoch.to_string()
                ],
                |r| r.get::<_, u64>(0),
            )
            .optional()
            .map_err(|_| Error::Unavailable)?
            .unwrap_or(0);
        if current != expected {
            return Err(Error::Stale);
        }
        tx.execute(
            "INSERT INTO swarmie_vaults(account,vault,epoch,revision,snapshot) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(account,vault,epoch) DO UPDATE SET revision=excluded.revision,snapshot=excluded.snapshot",
            params![identity.account.to_string(), identity.vault.to_string(), identity.epoch.to_string(), next.revision, bytes],
        ).map_err(|_| Error::Unavailable)?;
        if tx.commit().is_err() {
            self.lease.database.health.require_recovery();
            return Err(Error::Unknown);
        }
        Ok(())
    }
}

fn valid_namespace(identity: VaultIdentity, snapshot: &VaultSnapshot) -> bool {
    snapshot
        .records
        .iter()
        .all(|r| r.header.identity == identity)
        && snapshot
            .pending
            .iter()
            .all(|c| c.record().header.identity == identity)
        && snapshot
            .rejected
            .iter()
            .all(|draft| draft.command.record().header.identity == identity)
        && snapshot.accepted.iter().all(|operation| {
            snapshot
                .pending
                .iter()
                .any(|command| command.operation == *operation)
        })
        && snapshot.captures.iter().all(|c| {
            c.intent.identity == identity
                && c.candidate
                    .as_ref()
                    .is_none_or(|r| r.header.identity == identity)
        })
}
