//! Controlled Contract proxy over real SQLite for concurrent scheduling acceptance.

use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

use swarmie_task_contracts::*;
use tembra::{core, prelude::*};

/// Shared observations and one-shot race controls. No task records are stored by this proxy.
pub struct StorageControl {
    backend: TaskStorageRef,
    owner: RefCell<Option<String>>,
    pub gets: Cell<usize>,
    pub scans: Cell<usize>,
    pub commits: Cell<usize>,
    pub page_limit: Cell<u32>,
    pub unfiltered_scans: Cell<usize>,
    pub scanned_rows: Cell<usize>,
    pub terminal_root_rows: Cell<usize>,
    pub cursor_invalidations: Cell<usize>,
    pub injected_revision_conflicts: Cell<usize>,
    pub injected_checkpoint_change: Cell<bool>,
    pub metadata_conflicts: Cell<usize>,
    pub insert_child_during_cancel: Cell<bool>,
    pub insert_child_during_complete: Cell<bool>,
    pub pause_final_commit: Cell<bool>,
    pub final_commit_started: Cell<bool>,
    released: core::CancellationToken,
    effects: RefCell<BTreeMap<String, usize>>,
}

impl StorageControl {
    pub fn new(backend: TaskStorageRef) -> Rc<Self> {
        Rc::new(Self {
            backend,
            owner: RefCell::new(None),
            gets: Cell::new(0),
            scans: Cell::new(0),
            commits: Cell::new(0),
            page_limit: Cell::new(0),
            unfiltered_scans: Cell::new(0),
            scanned_rows: Cell::new(0),
            terminal_root_rows: Cell::new(0),
            cursor_invalidations: Cell::new(0),
            injected_revision_conflicts: Cell::new(0),
            injected_checkpoint_change: Cell::new(false),
            metadata_conflicts: Cell::new(0),
            insert_child_during_cancel: Cell::new(false),
            insert_child_during_complete: Cell::new(false),
            pause_final_commit: Cell::new(false),
            final_commit_started: Cell::new(false),
            released: core::CancellationToken::new(),
            effects: RefCell::new(BTreeMap::new()),
        })
    }

    pub fn reset_counts(&self) {
        self.gets.set(0);
        self.scans.set(0);
        self.commits.set(0);
        self.unfiltered_scans.set(0);
        self.scanned_rows.set(0);
        self.terminal_root_rows.set(0);
    }

    pub fn release_commit(&self) {
        self.released.cancel(RuntimeError::aborted(
            "fixture_release",
            "release controlled storage commit",
        ));
    }

    pub fn effects(&self, id: &str) -> usize {
        *self.effects.borrow().get(id).unwrap_or(&0)
    }

    pub async fn external_create(&self, spec: TaskSpec) -> Result<()> {
        let owner = self
            .owner
            .borrow()
            .clone()
            .expect("claimed scheduler storage");
        match self
            .backend
            .commit(TaskTransaction {
                owner,
                writes: vec![TaskWrite {
                    provenance: None,
                    expected_revision: None,
                    record: TaskRecord::new(spec, None),
                }],
            })
            .await?
        {
            CommitResult::Committed { .. } => Ok(()),
            CommitResult::Conflict { .. } => Err(RuntimeError::invalid(
                "fixture_conflict",
                "external task already exists",
            )),
        }
    }

    async fn bump(&self, id: &str, owner: &str) -> Result<()> {
        let mut record = self
            .backend
            .get(TaskLookup { id: id.into() })
            .await?
            .ok_or_else(|| {
                RuntimeError::invalid(
                    "fixture_missing",
                    "revision injection requires a committed task",
                )
            })?;
        let revision = record.revision;
        record.revision += 1;
        record.attempts += 1;
        if id == "parent" && self.injected_checkpoint_change.replace(false) {
            record.checkpoint = serde_json::json!({"newer_checkpoint":true});
            record.phase = "newer-phase".into();
            record.status = TaskStatus::Sleeping;
            record.wake_at_ms = Some(9_000_000_000_000_000);
        }
        match self
            .backend
            .commit(TaskTransaction {
                owner: owner.into(),
                writes: vec![TaskWrite {
                    provenance: None,
                    expected_revision: Some(revision),
                    record,
                }],
            })
            .await?
        {
            CommitResult::Committed { .. } => Ok(()),
            CommitResult::Conflict { .. } => Err(RuntimeError::invalid(
                "fixture_conflict",
                "controlled backend mutation unexpectedly conflicted",
            )),
        }
    }
}

/// The separate backend Host owns the actual SQLite Resource and its physical connection.
pub fn storage_facet(control: Rc<StorageControl>) -> Result<NativeFacet> {
    NativeFacet::new("fixture.concurrent-storage", vec![], move |_, setup| {
        let control = control.clone();
        setup.serve_with_options(
            TaskStorage,
            move |_, _, _| {
                Ok(Proxy {
                    control: control.clone(),
                })
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )
    })
}

struct Proxy {
    control: Rc<StorageControl>,
}

impl TaskStorageProvider for Proxy {
    async fn outbox_status(&self, _context: Context) -> Result<TaskOutboxStatus> {
        self.control.backend.outbox_status().await
    }

    async fn register_outbox_consumer(
        &self,
        _request: TaskOutboxConsumer,
        context: Context,
    ) -> Result<TaskOutboxConsumerState> {
        task_outbox_consumer_owner(&context)?;
        // This fixture delegates to a different Host; a local Context cannot transfer its
        // authenticated Facet attribution across that boundary. Do not impersonate callers.
        Err(RuntimeError::invalid(
            "fixture_outbox_delegation",
            "cross-Host consumer delegation is unsupported",
        ))
    }

    async fn read_outbox(
        &self,
        _request: TaskOutboxQuery,
        context: Context,
    ) -> Result<TaskOutboxPage> {
        task_outbox_consumer_owner(&context)?;
        // This fixture delegates to a different Host; a local Context cannot transfer its
        // authenticated Facet attribution across that boundary. Do not impersonate callers.
        Err(RuntimeError::invalid(
            "fixture_outbox_delegation",
            "cross-Host consumer delegation is unsupported",
        ))
    }

    async fn acknowledge_outbox(
        &self,
        _request: TaskOutboxAcknowledgement,
        context: Context,
    ) -> Result<()> {
        task_outbox_consumer_owner(&context)?;
        // This fixture delegates to a different Host; a local Context cannot transfer its
        // authenticated Facet attribution across that boundary. Do not impersonate callers.
        Err(RuntimeError::invalid(
            "fixture_outbox_delegation",
            "cross-Host consumer delegation is unsupported",
        ))
    }

    async fn remove_outbox_consumer(
        &self,
        _request: TaskOutboxConsumer,
        context: Context,
    ) -> Result<()> {
        task_outbox_consumer_owner(&context)?;
        // This fixture delegates to a different Host; a local Context cannot transfer its
        // authenticated Facet attribution across that boundary. Do not impersonate callers.
        Err(RuntimeError::invalid(
            "fixture_outbox_delegation",
            "cross-Host consumer delegation is unsupported",
        ))
    }

    async fn compact_outbox(
        &self,
        request: TaskOutboxCompaction,
        _context: Context,
    ) -> Result<TaskOutboxRetention> {
        self.control.backend.compact_outbox(request).await
    }

    async fn claim_owner(&self, owner: StorageOwner, _context: Context) -> Result<()> {
        self.control.backend.claim_owner(owner.clone()).await?;
        *self.control.owner.borrow_mut() = Some(owner.id);
        Ok(())
    }
    async fn release_owner(&self, owner: StorageOwner, _context: Context) -> Result<()> {
        self.control.backend.release_owner(owner).await?;
        *self.control.owner.borrow_mut() = None;
        Ok(())
    }
    async fn get(&self, request: TaskLookup, _context: Context) -> Result<Option<TaskRecord>> {
        self.control.gets.set(self.control.gets.get() + 1);
        self.control.backend.get(request).await
    }
    async fn scan(&self, mut query: TaskQuery, _context: Context) -> Result<TaskPage> {
        if self.control.page_limit.get() > 0 {
            query.limit = query.limit.min(self.control.page_limit.get());
        }
        self.control.scans.set(self.control.scans.get() + 1);
        if query.parent.is_none() && query.statuses.is_empty() {
            self.control
                .unfiltered_scans
                .set(self.control.unfiltered_scans.get() + 1);
        }
        if query.cursor.is_some() && self.control.cursor_invalidations.get() > 0 {
            self.control
                .cursor_invalidations
                .set(self.control.cursor_invalidations.get() - 1);
            let owner = self
                .control
                .owner
                .borrow()
                .clone()
                .expect("claimed scheduler storage");
            self.control.bump("churn", &owner).await?;
        }
        let page = self.control.backend.scan(query).await?;
        self.control
            .scanned_rows
            .set(self.control.scanned_rows.get() + page.records.len());
        self.control.terminal_root_rows.set(
            self.control.terminal_root_rows.get()
                + page
                    .records
                    .iter()
                    .filter(|record| record.parent.is_none() && record.is_terminal())
                    .count(),
        );
        Ok(page)
    }
    async fn commit(
        &self,
        transaction: TaskTransaction,
        _context: Context,
    ) -> Result<CommitResult> {
        self.control.commits.set(self.control.commits.get() + 1);
        if self.control.insert_child_during_cancel.get()
            && transaction.writes.iter().any(|write| {
                write.record.id == "leaf"
                    && write.record.is_terminal()
                    && write.record.cancellation_requested
            })
        {
            self.control.insert_child_during_cancel.set(false);
            let child = TaskRecord::new(
                TaskSpec {
                    id: "late-grandchild".into(),
                    kind: "fixture.concurrent".into(),
                    definition_version: 1,
                    input: serde_json::Value::Null,
                    checkpoint: serde_json::Value::Null,
                    phase: "effect".into(),
                },
                Some("leaf".into()),
            );
            assert!(matches!(
                self.control
                    .backend
                    .commit(TaskTransaction {
                        owner: transaction.owner.clone(),
                        writes: vec![TaskWrite {
                            provenance: None,
                            expected_revision: None,
                            record: child
                        }]
                    })
                    .await?,
                CommitResult::Committed { .. }
            ));
        }
        let settled = transaction.writes.iter().any(|write| {
            write.record.id == "parent"
                && write.record.status == TaskStatus::Terminal
                && !write.record.cancellation_requested
        });
        if settled && self.control.insert_child_during_complete.replace(false) {
            let child = TaskRecord::new(
                TaskSpec {
                    id: "late-owned-child".into(),
                    kind: "fixture.missing".into(),
                    definition_version: 1,
                    input: serde_json::Value::Null,
                    checkpoint: serde_json::Value::Null,
                    phase: "effect".into(),
                },
                Some("parent".into()),
            );
            assert!(matches!(
                self.control
                    .backend
                    .commit(TaskTransaction {
                        owner: transaction.owner.clone(),
                        writes: vec![TaskWrite {
                            provenance: None,
                            expected_revision: None,
                            record: child
                        }]
                    })
                    .await?,
                CommitResult::Committed { .. }
            ));
        }
        if settled && self.control.pause_final_commit.replace(false) {
            self.control.final_commit_started.set(true);
            self.control.released.cancelled().await;
        }
        if settled && self.control.injected_revision_conflicts.get() > 0 {
            self.control
                .injected_revision_conflicts
                .set(self.control.injected_revision_conflicts.get() - 1);
            self.control.bump("parent", &transaction.owner).await?;
        }
        if transaction
            .writes
            .iter()
            .any(|write| write.record.id == "parent" && write.record.status == TaskStatus::Parked)
            && self.control.metadata_conflicts.get() > 0
        {
            self.control
                .metadata_conflicts
                .set(self.control.metadata_conflicts.get() - 1);
            self.control.bump("parent", &transaction.owner).await?;
        }
        self.control.backend.commit(transaction).await
    }
}

#[tembra::contract(local, name = "swarmie.acceptance-concurrent-handler", version = 1)]
trait ConcurrentHandler {
    #[portable]
    async fn step(&self, invocation: TaskInvocation) -> Result<TaskTransition>;
}

pub fn handler_facet(control: Rc<StorageControl>) -> Result<NativeFacet> {
    NativeFacet::new("fixture.concurrent-handler", vec![], move |_, setup| {
        let control = control.clone();
        setup.serve_with_options(
            ConcurrentHandler,
            move |_, _, declaration| {
                declaration.core_mut().register_method(
                    &task_definitions()?,
                    "fixture.concurrent",
                    serde_json::json!({"kind":"fixture.concurrent","version":1}),
                    "step",
                )?;
                Ok(Handler {
                    control: control.clone(),
                })
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )
    })
}

struct Handler {
    control: Rc<StorageControl>,
}

impl ConcurrentHandlerProvider for Handler {
    async fn step(&self, invocation: TaskInvocation, _context: Context) -> Result<TaskTransition> {
        let TaskInvocation::Run { task, .. } = invocation else {
            return Ok(TaskTransition::RejectMigration {
                reason: "no migration needed".into(),
            });
        };
        *self
            .control
            .effects
            .borrow_mut()
            .entry(task.id.clone())
            .or_default() += 1;
        if task.input["invalidate_after_effect"] == true {
            self.control.page_limit.set(1);
            self.control.cursor_invalidations.set(12);
        }
        if task.input["mutate_before_result"] == true {
            let owner = self
                .control
                .owner
                .borrow()
                .clone()
                .expect("claimed storage");
            self.control.injected_checkpoint_change.set(true);
            self.control.bump(&task.id, &owner).await?;
        }
        if task.input["handler_error"] == true {
            return Err(RuntimeError::invalid(
                "fixture_handler_error",
                "handler failed after effect",
            ));
        }
        if task.input["control_error"] == true {
            return Err(RuntimeError::invalid(
                "fixture_handler_error",
                "\u{0001}".repeat(20_000),
            ));
        }
        if task.input["protocol_fault"] == true {
            return Ok(TaskTransition::Migrate {
                checkpoint: serde_json::Value::Null,
                phase: "illegal".into(),
            });
        }
        if task.input["collision"] == true {
            Ok(TaskTransition::Wait {
                checkpoint: serde_json::json!({"effect_performed":true}),
                phase: "complete".into(),
                children: vec![TaskSpec {
                    id: "collision".into(),
                    kind: "fixture.concurrent".into(),
                    definition_version: 1,
                    input: serde_json::Value::Null,
                    checkpoint: serde_json::Value::Null,
                    phase: "effect".into(),
                }],
                task_ids: vec![],
                policy: WaitPolicy::AllSettled,
            })
        } else {
            Ok(TaskTransition::Complete {
                value: serde_json::json!({"effect_performed":true}),
            })
        }
    }
}
