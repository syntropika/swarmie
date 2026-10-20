//! Deliberately ephemeral replacement fixture. Root tasks only; owned work is explicitly refused.
//! This provider proves Profile substitution, and does not claim backend conformance or durability.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    sync::atomic::{AtomicU64, Ordering},
};

use futures::FutureExt;
use swarmie_task_contracts::*;
use tembra::{core, prelude::*};

pub const PLUGIN_ID: &str = "fixture.volatile-task-storage";
pub const FACET_ID: &str = "fixture.volatile-task-storage.provider";

struct Data {
    store_id: String,
    owner: Option<String>,
    records: BTreeMap<String, TaskRecord>,
    sequence: u64,
}

impl Default for Data {
    fn default() -> Self {
        static NEXT_STORE: AtomicU64 = AtomicU64::new(1);
        Self {
            store_id: format!(
                "fixture-store-{}",
                NEXT_STORE.fetch_add(1, Ordering::Relaxed)
            ),
            owner: None,
            records: BTreeMap::new(),
            sequence: 0,
        }
    }
}

pub fn facet() -> Result<NativeFacet> {
    NativeFacet::new(FACET_ID, vec![], |_, setup| {
        setup.serve_with_options(
            TaskStorage,
            |_, _, declaration| {
                let resource = declaration.core_mut().resource(
                    "fixture-records",
                    ResourceDeclaration::new(|_| {
                        async { Ok(RefCell::new(Data::default())) }.boxed_local()
                    })
                    .with_close(|value| {
                        async move {
                            *value.borrow_mut() = Data::default();
                            Ok(())
                        }
                        .boxed_local()
                    }),
                )?;
                Ok(Service { resource })
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )
    })
}

struct Service {
    resource: Resource<RefCell<Data>>,
}

fn invalid(message: &str) -> RuntimeError {
    RuntimeError::invalid("fixture_storage_unsupported", message)
}

impl TaskStorageProvider for Service {
    async fn claim_owner(&self, owner: StorageOwner, _context: Context) -> Result<()> {
        validate_identity(&owner.id)?;
        let value = self.resource.value()?;
        let mut data = value.borrow_mut();
        if data
            .owner
            .as_ref()
            .is_some_and(|actual| actual != &owner.id)
        {
            return Err(invalid("another fixture owner is active"));
        }
        data.owner = Some(owner.id);
        Ok(())
    }
    async fn release_owner(&self, owner: StorageOwner, _context: Context) -> Result<()> {
        let value = self.resource.value()?;
        let mut data = value.borrow_mut();
        if data.owner.as_deref() != Some(&owner.id) {
            return Err(invalid("fixture owner does not match"));
        }
        data.owner = None;
        Ok(())
    }
    async fn get(&self, request: TaskLookup, _context: Context) -> Result<Option<TaskRecord>> {
        validate_identity(&request.id)?;
        Ok(self
            .resource
            .value()?
            .borrow()
            .records
            .get(&request.id)
            .cloned())
    }
    async fn scan(&self, query: TaskQuery, _context: Context) -> Result<TaskPage> {
        if !(1..=256).contains(&query.limit) {
            return Err(invalid("fixture query limit must be 1..=256"));
        }
        let value = self.resource.value()?;
        let data = value.borrow();
        if let Some(cursor) = &query.cursor
            && (cursor.store_id != data.store_id
                || cursor.sequence != data.sequence
                || cursor.parent != query.parent
                || cursor.statuses != query.statuses)
        {
            return Err(RuntimeError::invalid(
                "task_storage_cursor_invalidated",
                "fixture cursor does not match committed snapshot",
            ));
        }
        let after = query.cursor.as_ref().map(|cursor| cursor.after.as_str());
        let mut selected = data
            .records
            .values()
            .filter(|record| {
                after.is_none_or(|id| record.id.as_str() > id)
                    && (query.statuses.is_empty() || query.statuses.contains(&record.status))
                    && query
                        .parent
                        .as_ref()
                        .is_none_or(|id| record.parent.as_ref() == Some(id))
            })
            .take(query.limit as usize + 1)
            .cloned()
            .collect::<Vec<_>>();
        let more = selected.len() > query.limit as usize;
        if more {
            selected.pop();
        }
        let next = if more {
            Some(TaskCursor {
                store_id: data.store_id.clone(),
                sequence: data.sequence,
                after: selected.last().unwrap().id.clone(),
                statuses: query.statuses,
                parent: query.parent,
            })
        } else {
            None
        };
        Ok(TaskPage {
            sequence: data.sequence,
            records: selected,
            next,
        })
    }
    async fn commit(
        &self,
        transaction: TaskTransaction,
        _context: Context,
    ) -> Result<CommitResult> {
        let value = self.resource.value()?;
        let mut data = value.borrow_mut();
        if data.owner.as_deref() != Some(&transaction.owner) {
            return Err(invalid("fixture transaction requires its active owner"));
        }
        if transaction.writes.is_empty() || transaction.writes.len() > 256 {
            return Err(invalid("fixture transaction requires 1..=256 writes"));
        }
        let mut ids = BTreeSet::new();
        for write in &transaction.writes {
            let record = &write.record;
            validate_identity(&record.id)?;
            validate_identity(&record.kind)?;
            validate_identity(&record.phase)?;
            if !ids.insert(record.id.clone()) {
                return Err(invalid("duplicate fixture write identity"));
            }
            if record.parent.is_some()
                || !record.wait_for.is_empty()
                || record.status == TaskStatus::Waiting
            {
                return Err(invalid(
                    "fixture replacement intentionally supports root tasks only",
                ));
            }
            let old = data.records.get(&record.id);
            if old.map(|record| record.revision) != write.expected_revision {
                return Ok(CommitResult::Conflict {
                    id: record.id.clone(),
                    actual_revision: old.map(|record| record.revision),
                });
            }
            let expected = write
                .expected_revision
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| invalid("revision overflow"))?;
            if record.revision != expected {
                return Err(invalid("fixture revisions must increment by one"));
            }
            if old.is_some_and(|old| {
                old.is_terminal() || old.kind != record.kind || old.input != record.input
            }) {
                return Err(invalid("immutable fixture record changed"));
            }
            if record.is_terminal() != record.outcome.is_some()
                && record.status != TaskStatus::Completing
            {
                return Err(invalid("fixture outcome does not match status"));
            }
        }
        let records = transaction
            .writes
            .into_iter()
            .map(|write| write.record)
            .collect::<Vec<_>>();
        for record in &records {
            data.records.insert(record.id.clone(), record.clone());
        }
        data.sequence += 1;
        Ok(CommitResult::Committed {
            sequence: data.sequence,
            records,
        })
    }
}
