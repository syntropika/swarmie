use crate::schema::io;
use rusqlite::{OptionalExtension, params};
use std::rc::Rc;
use swarmie_session_store_sqlite::backend as sessions;
use swarmie_sqlite_local::Database;
use swarmie_task_contracts::{CommitResult, TaskTransaction};
use swarmie_task_session_contracts::*;
use swarmie_task_storage_sqlite::backend::{Error, Store};
use tembra::{Result, RuntimeError};

pub(crate) struct Committer {
    pub database: Database,
    pub tasks: Rc<Store>,
    pub identity: TaskSessionCommitIdentity,
}
impl Committer {
    /// Validates current authority before replay; immutable operation equality excludes that
    /// transient authority so a new admitted scheduler can reconcile after a process restart.
    pub fn commit(&self, request: TaskSessionCommitRequest) -> Result<TaskSessionCommitResult> {
        self.tasks
            .authorize(&request.owner)
            .map_err(Error::into_runtime)?;
        request.operation.validate()?;
        if request.operation.identity != self.identity {
            return Err(RuntimeError::invalid(
                "task_session_commit_store_mismatch",
                "joint operation names a different live domain pair",
            ));
        }
        let canonical = swarmie_session_contracts::canonical(&request.operation)?;
        let operation = &request.operation;
        let result =
            self.database
                .transaction(|connection| -> Result<(TaskSessionCommitResult, bool)> {
                    let prior: Option<(String, String)> = connection
                    .prepare_cached(
                        "SELECT request,receipt FROM task_session_commits WHERE operation_id=?1",
                    )
                    .and_then(|mut statement| {
                        statement
                            .query_row([&operation.operation_id], |row| {
                                Ok((row.get(0)?, row.get(1)?))
                            })
                            .optional()
                    })
                    .map_err(io)?;
                    if let Some((prior, receipt)) = prior {
                        if prior != canonical {
                            return Err(RuntimeError::invalid(
                                "task_session_commit_duplicate",
                                "operation identity has different immutable content",
                            ));
                        }
                        let receipt: TaskSessionCommitReceipt =
                            serde_json::from_str(&receipt).map_err(io)?;
                        validate_task_session_commit_receipt(operation, &receipt)?;
                        return Ok((TaskSessionCommitResult::Committed { receipt }, true));
                    }
                    let tasks = self
                        .tasks
                        .commit_in(
                            connection,
                            TaskTransaction {
                                owner: request.owner.clone(),
                                writes: operation.writes.clone(),
                            },
                        )
                        .map_err(Error::into_runtime)?;
                    let (task_sequence, records) = match tasks {
                        CommitResult::Conflict {
                            id,
                            actual_revision,
                        } => {
                            return Ok((
                                TaskSessionCommitResult::Conflict {
                                    id,
                                    actual_revision,
                                },
                                false,
                            ));
                        }
                        CommitResult::Committed { sequence, records } => (sequence, records),
                    };
                    // Explicit handler history cannot accompany superseded progress. Ordinary
                    // scheduler cancellation propagation has no staged handler history and must
                    // still be able to mark a nonterminal descendant as cancellation-requested.
                    if !operation.history.is_empty() {
                        swarmie_task_storage_sqlite::backend::validate_history_ancestors(
                            connection, &records,
                        )?;
                    }
                    swarmie_task_session_integration::backend::validate_history_in(
                        connection,
                        &self.identity.task_store_id,
                        &self.identity.session_store_id,
                        &operation.history,
                    )?;
                    let mut appends = Vec::with_capacity(operation.history.len());
                    for history in &operation.history {
                        appends.push(sessions::append_in(
                            connection,
                            &self.identity.session_store_id,
                            history.append.clone(),
                        )?);
                    }
                    // Structural facts use their existing immutable mapping and exact Session
                    // batch retry identity. Consumer ACK remains with its actual caller Facet.
                    swarmie_task_session_integration::backend::project_task_commit_in(
                        connection,
                        &self.identity.task_store_id,
                        &self.identity.session_store_id,
                        task_sequence,
                        &records,
                    )?;
                    let receipt = TaskSessionCommitReceipt {
                        operation_id: operation.operation_id.clone(),
                        task_sequence,
                        records,
                        appends,
                        head: sessions::head(connection)?,
                    };
                    validate_task_session_commit_receipt(operation, &receipt)?;
                    let encoded = swarmie_session_contracts::canonical(&receipt)?;
                    connection
                        .prepare_cached("INSERT INTO task_session_commits VALUES(?1,?2,?3)")
                        .and_then(|mut statement| {
                            statement.execute(params![operation.operation_id, canonical, encoded])
                        })
                        .map_err(io)?;
                    Ok((TaskSessionCommitResult::Committed { receipt }, false))
                });
        match result {
            Ok((result, replay)) => {
                if let TaskSessionCommitResult::Committed { receipt } = &result {
                    if replay {
                        self.tasks.refresh_sequence();
                    } else {
                        self.tasks.adopt_sequence(receipt.task_sequence);
                    }
                }
                Ok(result)
            }
            Err(error) => {
                self.tasks.refresh_sequence();
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        path::Path,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };
    use swarmie_session_contracts::*;
    use swarmie_task_contracts::*;
    use tembra::{core, prelude::*};

    struct Fixture {
        host: Host,
        database: Database,
        tasks: TaskStorageRef,
        sessions: SessionStoreRef,
        committer: Committer,
    }
    async fn open(path: &Path, driver: Rc<core::ManualDriver>) -> Result<Fixture> {
        let host = Host::with_driver("joint-storage-fixture", "application", driver.clone())?;
        let (owner, handle) = swarmie_sqlite_local::facet(
            &swarmie_sqlite_local::Configuration {
                database: path.into(),
            },
            vec![
                swarmie_task_storage_sqlite::sqlite_schema(),
                swarmie_session_store_sqlite::sqlite_schema(),
                swarmie_task_session_integration::sqlite_schema(),
                crate::sqlite_schema(),
            ],
        )?;
        host.install(owner, core::LoadOptions::default())?.await?;
        let (task_facet, tasks_handle) =
            swarmie_task_storage_sqlite::facet_with_database(&handle, Default::default())?;
        let (session_facet, _) =
            swarmie_session_store_sqlite::facet_with_database(&handle, driver)?;
        let installing_tasks = host.install(task_facet, core::LoadOptions::default())?;
        let installing_sessions = host.install(session_facet, core::LoadOptions::default())?;
        installing_tasks.await?;
        installing_sessions.await?;
        let tasks = host.get(TaskStorage, TaskStorageInput {}).await?;
        let sessions = host.get(SessionStore, SessionStoreInput {}).await?;
        tasks
            .claim_owner(StorageOwner { id: "owner".into() })
            .await?;
        sessions
            .create(CreateSession {
                session_id: "chat".into(),
                metadata: serde_json::json!({}),
            })
            .await?;
        let database = handle.database()?;
        let committer = Committer {
            database: database.clone(),
            tasks: tasks_handle.store()?,
            identity: TaskSessionCommitIdentity {
                task_store_id: tasks.identity().await?.store_id,
                session_store_id: sessions.identity().await?.store_id,
            },
        };
        Ok(Fixture {
            host,
            database,
            tasks,
            sessions,
            committer,
        })
    }
    fn request(fixture: &Fixture, id: &str) -> TaskSessionCommitRequest {
        let record = TaskRecord::new(
            TaskSpec {
                id: id.into(),
                kind: "fixture.task".into(),
                definition_version: 1,
                input: serde_json::json!({}),
                checkpoint: serde_json::json!({"prepared":true}),
                phase: "prepared".into(),
            },
            None,
        );
        TaskSessionCommitRequest {
            owner: "owner".into(),
            operation: TaskSessionCommitOperation::new(
                fixture.committer.identity.clone(),
                vec![TaskWrite {
                    provenance: None,
                    expected_revision: None,
                    record,
                }],
                vec![TaskSessionAppend {
                    session_store_id: fixture.committer.identity.session_store_id.clone(),
                    append: AppendEvents {
                        session_id: "chat".into(),
                        batch_id: id.into(),
                        expected_head: None,
                        events: vec![NewEvent {
                            event_id: id.into(),
                            kind: "fixture.prepared".into(),
                            version: 1,
                            correlation: Correlation {
                                task_id: Some(id.into()),
                                ..Default::default()
                            },
                            occurred_at_ms: None,
                            payload: serde_json::json!({"prepared":true}),
                        }],
                    },
                }],
            )
            .unwrap(),
        }
    }
    fn configure_route(fixture: &Fixture, request: &TaskSessionCommitRequest) -> Result<()> {
        let record = &request.operation.writes[0].record;
        let binding = TaskSessionBinding {
            key: BindingKey {
                task_store_id: fixture.committer.identity.task_store_id.clone(),
                task_id: record.id.clone(),
            },
            destination: SessionDestination {
                session_store_id: fixture.committer.identity.session_store_id.clone(),
                session_id: "chat".into(),
                conversation_id: "conversation".into(),
                turn_id: "turn".into(),
            },
            root_task_id: record.id.clone(),
            parent_task_id: None,
            routing_version: 1,
            task: TaskSpec {
                id: record.id.clone(),
                kind: record.kind.clone(),
                definition_version: record.definition_version,
                input: record.input.clone(),
                checkpoint: record.checkpoint.clone(),
                phase: record.phase.clone(),
            },
            admission: BindingAdmission::Pending,
        };
        let encoded = swarmie_session_contracts::canonical(&BindingDisposition::Routed {
            binding: Box::new(binding.clone()),
        })?;
        fixture.database.transaction::<_, RuntimeError>(|connection| {
            connection.execute("INSERT INTO binding_configuration VALUES(1,?1,?2,'publisher')", params![fixture.committer.identity.task_store_id, fixture.committer.identity.session_store_id]).map_err(io)?;
            connection.execute("INSERT INTO task_session_bindings VALUES(?1,?2,?3)", params![binding.key.task_store_id,binding.key.task_id,encoded]).map_err(io)?;
            connection.execute("INSERT INTO task_outbox_consumers VALUES('publisher',0,-1,0,-1,'fixture.publisher')", []).map_err(io)?;
            Ok(())
        })?;
        Ok(())
    }

    fn count(fixture: &Fixture, table: &str) -> u64 {
        fixture
            .database
            .with_connection::<_, swarmie_sqlite_local::Error>(|connection| {
                Ok(
                    connection.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                        row.get(0)
                    })?,
                )
            })
            .unwrap()
    }
    async fn assert_absent(fixture: &Fixture, id: &str) -> Result<()> {
        assert!(
            fixture
                .tasks
                .get(TaskLookup { id: id.into() })
                .await?
                .is_none()
        );
        assert_eq!(count(fixture, "task_records"), 0);
        assert_eq!(count(fixture, "task_outbox"), 0);
        assert_eq!(count(fixture, "session_events"), 0);
        assert_eq!(count(fixture, "session_batches"), 0);
        assert_eq!(count(fixture, "task_session_commits"), 0);
        Ok(())
    }

    #[test]
    fn native_writes_refuse_another_owners_connection_before_mutation_or_replay() -> Result<()> {
        let directory = tempfile::tempdir().unwrap();
        let driver = core::ManualDriver::new();
        driver.run_until(async {
            let first = open(&directory.path().join("first.sqlite"), driver.clone()).await?;
            let second = open(&directory.path().join("second.sqlite"), driver.clone()).await?;
            let first_request = request(&first, "same");
            let second_request = request(&second, "same");
            second
                .database
                .transaction::<_, RuntimeError>(|connection| {
                    let error = first
                        .committer
                        .tasks
                        .commit_in(
                            connection,
                            TaskTransaction {
                                owner: first_request.owner.clone(),
                                writes: first_request.operation.writes.clone(),
                            },
                        )
                        .unwrap_err();
                    assert!(error.to_string().contains("different task store"));
                    let error = sessions::append_in(
                        connection,
                        &first.committer.identity.session_store_id,
                        first_request.operation.history[0].append.clone(),
                    )
                    .unwrap_err();
                    assert_eq!(error.reason, "session_store_identity");
                    Ok(())
                })?;
            assert_absent(&first, "same").await?;
            assert_absent(&second, "same").await?;
            second.committer.commit(second_request.clone())?;
            second
                .database
                .transaction::<_, RuntimeError>(|connection| {
                    let error = sessions::append_in(
                        connection,
                        &first.committer.identity.session_store_id,
                        first_request.operation.history[0].append.clone(),
                    )
                    .unwrap_err();
                    assert_eq!(
                        error.reason, "session_store_identity",
                        "foreign connection cannot replay a successful identically named batch"
                    );
                    Ok(())
                })?;
            assert_absent(&first, "same").await?;
            assert_eq!(count(&second, "task_records"), 1);
            assert_eq!(count(&second, "session_events"), 1);
            assert_eq!(count(&second, "task_session_commits"), 1);
            first.committer.commit(first_request)?;
            first.host.close().await?;
            second.host.close().await
        })
    }

    #[test]
    fn bound_task_only_commit_projects_local_history_without_publisher_or_foreign_ack() -> Result<()>
    {
        let directory = tempfile::tempdir().unwrap();
        let driver = core::ManualDriver::new();
        driver.run_until(async {
            let fixture = open(&directory.path().join("local.sqlite"), driver.clone()).await?;
            let mut request = request(&fixture, "first");
            configure_route(&fixture, &request)?;
            request.operation = TaskSessionCommitOperation::new(request.operation.identity, request.operation.writes, vec![])?;
            let result = fixture.committer.commit(request.clone())?;
            let TaskSessionCommitResult::Committed { receipt } = &result else { panic!("fresh commit") };
            assert!(receipt.appends.is_empty(), "structural projection is outside explicit append receipts");
            assert_eq!(receipt.head.commit_sequence, 2);
            assert_eq!(count(&fixture, "task_records"), 1);
            assert_eq!(count(&fixture, "task_outbox"), 1);
            let page = fixture.sessions.events(EventQuery::new("chat")).await?;
            assert_eq!(page.events.len(), 1, "local history exists before any publisher is installed");
            assert_eq!(page.events[0].event.kind, "swarmie.task-transition");
            fixture.database.with_connection::<_, swarmie_sqlite_local::Error>(|connection| {
                let disposition: String = connection.query_row("SELECT disposition FROM task_session_bindings", [], |row| row.get(0))?;
                assert!(matches!(serde_json::from_str::<BindingDisposition>(&disposition).unwrap(), BindingDisposition::Routed { binding } if binding.admission == BindingAdmission::Active));
                let consumer: (u64,i64,u64,i64,String) = connection.query_row("SELECT ack_sequence,ack_ordinal,delivered_sequence,delivered_ordinal,owner_facet FROM task_outbox_consumers WHERE id='publisher'", [], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)))?;
                assert_eq!(consumer, (0,-1,0,-1,"fixture.publisher".into()));
                Ok(())
            }).unwrap();
            assert_eq!(fixture.committer.commit(request)?, result);
            let missing = self::request(&fixture, "missing");
            assert_eq!(fixture.committer.commit(missing).unwrap_err().reason, "binding_missing");
            assert!(fixture.tasks.get(TaskLookup { id: "missing".into() }).await?.is_none());
            assert_eq!(count(&fixture, "task_records"), 1);
            assert_eq!(count(&fixture, "task_outbox"), 1);
            assert_eq!(count(&fixture, "session_events"), 1, "explicit append is rolled back together with a missing structural route");
            assert_eq!(count(&fixture, "task_session_commits"), 1);
            fixture.host.close().await
        })
    }

    #[test]
    fn configured_explicit_history_requires_the_exact_task_route_and_rolls_back_refusals()
    -> Result<()> {
        let directory = tempfile::tempdir().unwrap();
        let driver = core::ManualDriver::new();
        driver.run_until(async {
            let fixture = open(&directory.path().join("local.sqlite"), driver.clone()).await?;
            fixture
                .sessions
                .create(CreateSession {
                    session_id: "other".into(),
                    metadata: serde_json::json!({}),
                })
                .await?;
            let mut valid = request(&fixture, "routed");
            valid.operation.history[0].append.events[0]
                .correlation
                .conversation_id = Some("conversation".into());
            valid.operation.history[0].append.events[0]
                .correlation
                .turn_id = Some("turn".into());
            valid.operation = TaskSessionCommitOperation::new(
                valid.operation.identity,
                valid.operation.writes,
                valid.operation.history,
            )?;
            configure_route(&fixture, &valid)?;
            for selector in 0..4 {
                let mut invalid = valid.clone();
                match selector {
                    0 => invalid.operation.history[0].append.session_id = "other".into(),
                    1 => {
                        invalid.operation.history[0].append.events[0]
                            .correlation
                            .conversation_id = Some("other".into())
                    }
                    2 => {
                        invalid.operation.history[0].append.events[0]
                            .correlation
                            .turn_id = Some("other".into())
                    }
                    _ => {
                        invalid.operation.history[0].append.events[0]
                            .correlation
                            .turn_id = None
                    }
                }
                invalid.operation = TaskSessionCommitOperation::new(
                    invalid.operation.identity,
                    invalid.operation.writes,
                    invalid.operation.history,
                )?;
                assert!(
                    fixture.committer.commit(invalid).is_err(),
                    "foreign or missing selector {selector} must be refused"
                );
                assert_absent(&fixture, "routed").await?;
            }
            let excluded = request(&fixture, "excluded");
            let disposition = BindingDisposition::Excluded {
                exclusion: ExcludedBinding {
                    key: BindingKey {
                        task_store_id: fixture.committer.identity.task_store_id.clone(),
                        task_id: "excluded".into(),
                    },
                    reason: "fixture unrelated task".into(),
                },
            };
            let encoded = swarmie_session_contracts::canonical(&disposition)?;
            fixture
                .database
                .transaction::<_, RuntimeError>(|connection| {
                    connection
                        .execute(
                            "INSERT INTO task_session_bindings VALUES(?1,'excluded',?2)",
                            params![fixture.committer.identity.task_store_id, encoded],
                        )
                        .map_err(io)?;
                    Ok(())
                })?;
            assert_eq!(
                fixture.committer.commit(excluded).unwrap_err().reason,
                "binding_excluded"
            );
            assert_absent(&fixture, "excluded").await?;
            fixture.committer.commit(valid)?;
            assert_eq!(count(&fixture, "task_records"), 1);
            assert_eq!(
                count(&fixture, "session_events"),
                2,
                "matching explicit and structural history commit together"
            );
            fixture.host.close().await
        })
    }

    #[test]
    fn native_domain_writes_without_an_outer_transaction_refuse_before_mutation() -> Result<()> {
        let directory = tempfile::tempdir().unwrap();
        let driver = core::ManualDriver::new();
        driver.run_until(async {
            let fixture = open(&directory.path().join("local.sqlite"), driver.clone()).await?;
            let request = request(&fixture, "unbounded");
            fixture
                .database
                .with_connection::<_, swarmie_sqlite_local::Error>(|connection| {
                    assert!(
                        fixture
                            .committer
                            .tasks
                            .commit_in(
                                connection,
                                TaskTransaction {
                                    owner: request.owner.clone(),
                                    writes: request.operation.writes.clone(),
                                }
                            )
                            .is_err()
                    );
                    assert!(
                        sessions::append_in(
                            connection,
                            &fixture.committer.identity.session_store_id,
                            request.operation.history[0].append.clone()
                        )
                        .is_err()
                    );
                    assert!(
                        swarmie_task_session_integration::backend::project_task_commit_in(
                            connection,
                            &fixture.committer.identity.task_store_id,
                            &fixture.committer.identity.session_store_id,
                            1,
                            &[],
                        )
                        .is_err()
                    );
                    Ok(())
                })
                .unwrap();
            assert_absent(&fixture, "unbounded").await?;
            fixture.host.close().await
        })
    }

    #[test]
    fn malformed_task_domain_is_refused_without_journal_or_file_mutation() -> Result<()> {
        for mutation in [
            "DELETE FROM task_storage_metadata",
            "UPDATE task_storage_metadata SET store_id=''",
            "UPDATE task_storage_metadata SET sequence=9007199254740992",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("local.sqlite");
            let configuration = swarmie_sqlite_local::Configuration {
                database: path.clone(),
            };
            let schemas = vec![
                swarmie_task_storage_sqlite::sqlite_schema(),
                swarmie_session_store_sqlite::sqlite_schema(),
                crate::sqlite_schema(),
            ];
            Database::open(&configuration, schemas.clone())
                .map_err(RuntimeError::from)?
                .close()
                .map_err(RuntimeError::from)?;
            let connection = rusqlite::Connection::open(&path).map_err(io)?;
            connection
                .execute_batch("PRAGMA journal_mode=DELETE;")
                .map_err(io)?;
            connection.execute_batch(mutation).map_err(io)?;
            connection.close().map_err(|(_, error)| io(error))?;
            let bytes = std::fs::read(&path).unwrap();
            assert!(Database::open(&configuration, schemas).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        Ok(())
    }

    #[test]
    fn one_commit_retains_exact_receipt_after_advance_reopen_and_authority_replacement()
    -> Result<()> {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("local.sqlite");
        let driver = core::ManualDriver::new();
        driver.run_until(async {
            let fixture = open(&path, driver.clone()).await?;
            let commits = Arc::new(AtomicUsize::new(0));
            let observed = commits.clone();
            fixture
                .database
                .with_connection::<_, swarmie_sqlite_local::Error>(|connection| {
                    connection.commit_hook(Some(move || {
                        observed.fetch_add(1, Ordering::SeqCst);
                        false
                    }));
                    Ok(())
                })
                .unwrap();
            let first = request(&fixture, "first");
            let receipt = fixture.committer.commit(first.clone())?;
            assert_eq!(
                commits.load(Ordering::SeqCst),
                1,
                "task, outbox, history and ledger share one COMMIT"
            );
            assert_eq!(count(&fixture, "task_records"), 1);
            assert_eq!(count(&fixture, "session_events"), 1);
            let second = request(&fixture, "second");
            fixture.committer.commit(second)?;
            assert_eq!(
                fixture.committer.commit(first.clone())?,
                receipt,
                "replay precedes stale task CAS and returns the original receipt"
            );
            let mut changed = first.clone();
            changed.operation.history[0].append.events[0].payload =
                serde_json::json!({"changed":true});
            assert!(fixture.committer.commit(changed).is_err());
            let mut foreign = first.clone();
            foreign.owner = "foreign".into();
            assert!(
                fixture.committer.commit(foreign).is_err(),
                "replay still requires current authority"
            );
            fixture.host.close().await?;
            assert!(
                fixture.committer.commit(first.clone()).is_err(),
                "retired resources cannot replay"
            );
            let reopened = open(&path, driver.clone()).await?;
            reopened
                .tasks
                .release_owner(StorageOwner { id: "owner".into() })
                .await?;
            reopened
                .tasks
                .claim_owner(StorageOwner {
                    id: "successor".into(),
                })
                .await?;
            let mut retry = first;
            retry.owner = "successor".into();
            assert_eq!(reopened.committer.commit(retry)?, receipt);
            assert_eq!(count(&reopened, "task_records"), 2);
            assert_eq!(count(&reopened, "session_events"), 2);
            assert_eq!(count(&reopened, "task_session_commits"), 2);
            reopened.host.close().await
        })
    }

    #[test]
    fn history_refusal_and_failed_commit_roll_back_all_domains_and_sequence_cache() -> Result<()> {
        let directory = tempfile::tempdir().unwrap();
        let driver = core::ManualDriver::new();
        driver.run_until(async {
            let fixture = open(&directory.path().join("local.sqlite"), driver.clone()).await?;
            let valid = request(&fixture, "first");
            let mut invalid = valid.clone();
            invalid.operation.history[0].append.expected_head = Some(100);
            invalid.operation = TaskSessionCommitOperation::new(
                invalid.operation.identity,
                invalid.operation.writes,
                invalid.operation.history,
            )?;
            assert!(fixture.committer.commit(invalid).is_err());
            assert_absent(&fixture, "first").await?;
            fixture
                .database
                .with_connection::<_, swarmie_sqlite_local::Error>(|connection| {
                    connection.commit_hook(Some(|| true));
                    Ok(())
                })
                .unwrap();
            assert!(fixture.committer.commit(valid.clone()).is_err());
            fixture
                .database
                .with_connection::<_, swarmie_sqlite_local::Error>(|connection| {
                    connection.commit_hook(None::<fn() -> bool>);
                    Ok(())
                })
                .unwrap();
            assert_absent(&fixture, "first").await?;
            fixture.committer.commit(valid)?;
            assert_eq!(fixture.tasks.scan(Default::default()).await?.sequence, 1);
            assert_eq!(
                fixture
                    .sessions
                    .events(EventQuery::new("chat"))
                    .await?
                    .events
                    .len(),
                1
            );
            fixture.host.close().await
        })
    }

    #[test]
    fn process_exit_before_commit_never_retains_a_torn_task_history_or_receipt() -> Result<()> {
        const CHILD: &str = "SWARMIE_JOINT_COMMIT_CRASH_DATABASE";
        let driver = core::ManualDriver::new();
        if let Some(path) = std::env::var_os(CHILD) {
            return driver.run_until(async {
                let fixture = open(Path::new(&path), driver.clone()).await?;
                fixture
                    .database
                    .with_connection::<_, swarmie_sqlite_local::Error>(|connection| {
                        connection.commit_hook(Some(|| std::process::exit(77)));
                        Ok(())
                    })
                    .unwrap();
                fixture.committer.commit(request(&fixture, "crashed"))?;
                panic!("process must exit inside COMMIT");
            });
        }
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("local.sqlite");
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "commit::tests::process_exit_before_commit_never_retains_a_torn_task_history_or_receipt", "--nocapture"])
            .env(CHILD, &path).status().unwrap();
        assert_eq!(status.code(), Some(77));
        driver.run_until(async {
            let fixture = open(&path, driver.clone()).await?;
            assert_absent(&fixture, "crashed").await?;
            fixture.committer.commit(request(&fixture, "crashed"))?;
            fixture.host.close().await
        })
    }
}
