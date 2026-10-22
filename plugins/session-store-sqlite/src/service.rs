use crate::{Configuration, FACET_ID, database::Store, publication::Publication};
use futures::FutureExt;
use std::rc::Rc;
use swarmie_session_contracts::*;
use tembra::{core, prelude::*};

/// Declares a selectable Core Plugin with one eagerly opened, lifecycle-owned SQLite Resource.
///
/// # Errors
/// Invalid configuration fails immediately; lock, schema and filesystem failures reject the
/// installation waiter. Closing the Host retains every event, session and idempotency receipt.
pub fn facet(configuration: &Configuration, driver: Rc<dyn core::Driver>) -> Result<NativeFacet> {
    configuration.validate()?;
    let database = configuration.database.clone();
    NativeFacet::new(FACET_ID, vec![], move |_, setup| {
        let database = database.clone();
        let driver = driver.clone();
        let committed_definition = setup.state("committed")?;
        setup.serve_with_options(
            SessionStore,
            move |_, _, declaration| {
                let database = database.clone();
                let resource = declaration.core_mut().resource(
                    "sqlite",
                    ResourceDeclaration::new(move |_| {
                        let database = database.clone();
                        async move { Store::open(&database) }.boxed_local()
                    })
                    .with_close(|store| async move { store.close() }.boxed_local()),
                )?;
                let initial = resource.clone();
                let committed =
                    declaration.state(committed_definition.with_initializer(move |_| {
                        let resource = initial.clone();
                        async move { resource.value()?.head() }
                    }))?;
                let publication =
                    Publication::new(resource.clone(), committed.clone(), driver.clone());
                publication.attach(declaration)?;
                Ok(Service {
                    resource,
                    committed,
                    publication,
                })
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )
    })
}

struct Service {
    resource: Resource<Store>,
    committed: Cell<StoreHead>,
    publication: Rc<Publication>,
}

impl SessionStoreProvider for Service {
    async fn publication_status(&self, _context: Context) -> Result<PublicationStatus> {
        self.publication.status()
    }
    fn committed(&self) -> Readable<StoreHead> {
        self.committed.readable()
    }

    async fn create(&self, request: CreateSession, context: Context) -> Result<SessionRecord> {
        let record = self.resource.value()?.create(request)?;
        self.publication.reconcile(&context);
        Ok(record)
    }

    async fn open(
        &self,
        request: SessionLookup,
        context: Context,
    ) -> Result<Option<SessionRecord>> {
        let record = self.resource.value()?.get(request)?;
        self.publication.reconcile(&context);
        Ok(record)
    }

    async fn append(&self, request: AppendEvents, context: Context) -> Result<AppendReceipt> {
        let receipt = self.resource.value()?.append(request)?;
        self.publication.reconcile(&context);
        Ok(receipt)
    }

    async fn events(&self, query: EventQuery, context: Context) -> Result<EventPage> {
        let page = self.resource.value()?.events(query)?;
        self.publication.reconcile(&context);
        Ok(page)
    }

    async fn project(&self, query: ProjectionQuery, context: Context) -> Result<ProjectionPage> {
        // Projection wrappers are slightly larger than source wrappers. Reserve 1 KiB before
        // source admission so a returned projection always fits the original requested budget.
        let budget = query.source.max_bytes as usize;
        let mut source = query.source;
        if source.max_bytes < 5120 {
            return Err(RuntimeError::invalid(
                "session_store_invalid",
                "projection byte budget requires at least 5120",
            ));
        }
        source.max_bytes -= 1024;
        let page = project(self.resource.value()?.events(source)?, query.kind);
        bounded(&page, budget)?;
        self.publication.reconcile(&context);
        Ok(page)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell as Flag, rc::Rc};

    #[tembra::contract(local, name = "fixture.session-publication-admission", version = 1)]
    trait AdmissionProbe {
        #[state]
        fn committed(&self) -> StoreHead;
        #[state]
        fn projected(&self) -> u64;
        #[portable]
        async fn write(&self) -> Result<()>;
        #[portable]
        async fn read_open(&self) -> Result<SessionRecord>;
        #[portable]
        async fn read_events(&self) -> Result<EventPage>;
        #[portable]
        async fn read_projection(&self) -> Result<ProjectionPage>;
        #[portable]
        async fn publication_status(&self) -> Result<PublicationStatus>;
    }

    struct Probe {
        service: Service,
        projected: Readable<u64>,
    }

    impl AdmissionProbeProvider for Probe {
        async fn publication_status(&self, _context: Context) -> Result<PublicationStatus> {
            self.service.publication.status()
        }
        fn committed(&self) -> Readable<StoreHead> {
            self.service.committed.readable()
        }
        fn projected(&self) -> Readable<u64> {
            self.projected.clone()
        }
        async fn write(&self, context: Context) -> Result<()> {
            self.service
                .create(
                    CreateSession {
                        session_id: "chat".into(),
                        metadata: serde_json::json!({}),
                    },
                    context.clone(),
                )
                .await?;
            self.service.append(AppendEvents { session_id: "chat".into(), batch_id: "committed".into(), expected_head: None, events: vec![NewEvent { event_id: "m1".into(), kind: MESSAGE.into(), version: 1, correlation: Correlation::default(), occurred_at_ms: None, payload: serde_json::json!({"role":"user","content":"Durable content","reply_to":null,"tool_call_id":null}) }] }, context).await?;
            Ok(())
        }
        async fn read_open(&self, context: Context) -> Result<SessionRecord> {
            self.service
                .open(
                    SessionLookup {
                        session_id: "chat".into(),
                    },
                    context,
                )
                .await
                .map(|record| record.unwrap())
        }
        async fn read_events(&self, context: Context) -> Result<EventPage> {
            self.service.events(EventQuery::new("chat"), context).await
        }
        async fn read_projection(&self, context: Context) -> Result<ProjectionPage> {
            self.service
                .project(
                    ProjectionQuery {
                        source: EventQuery::new("chat"),
                        kind: ProjectionKind::Conversation,
                    },
                    context,
                )
                .await
        }
    }

    #[test]
    fn authoritative_reads_survive_state_admission_failure() {
        publication_probe(true);
    }

    #[test]
    fn closing_cancels_pending_publication_and_releases_sqlite() {
        publication_probe(false);
    }

    fn publication_probe(recover: bool) {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("sessions.sqlite");
        let driver = tembra_native::TokioDriver::new();
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(driver.run_until(async {
                let host =
                    Host::with_driver("admission-test", "application", driver.clone()).unwrap();
                let reject = Rc::new(Flag::new(true));
                let rejected = reject.clone();
                let factory_driver = driver.clone();
                host.install(
                    NativeFacet::new(
                        "fixture.session-publication-admission",
                        vec![],
                        move |_, setup| {
                            let database = database.clone();
                            let reject = rejected.clone();
                            let driver = factory_driver.clone();
                            let committed_definition = setup.state("committed")?;
                            let commit_projection_definition = setup.state("commit-projection")?;
                            setup.serve_with_options(
                                AdmissionProbe,
                                move |_, _, declaration| {
                                    let database = database.clone();
                                    let resource = declaration.core_mut().resource(
                                        "sqlite",
                                        ResourceDeclaration::new(move |_| {
                                            let database = database.clone();
                                            async move { Store::open(&database) }.boxed_local()
                                        })
                                        .with_close(
                                            |store| async move { store.close() }.boxed_local(),
                                        ),
                                    )?;
                                    let committed = declaration.state(
                                        committed_definition.value(StoreHead {
                                            store_id: "initializer".into(),
                                            commit_sequence: 0,
                                        })?,
                                    )?;
                                    let reject = reject.clone();
                                    let projected = declaration.derive(
                                        &commit_projection_definition,
                                        (committed.readable(),),
                                        move |(head,)| {
                                            if head.commit_sequence > 1 && reject.get() {
                                                return Err(RuntimeError::invalid(
                                                    "fixture_reactive_refusal",
                                                    "reject State before adoption",
                                                ));
                                            }
                                            Ok(head.commit_sequence)
                                        },
                                    )?;
                                    let publication = Publication::new(
                                        resource.clone(),
                                        committed.clone(),
                                        driver.clone(),
                                    );
                                    publication.attach(declaration)?;
                                    Ok(Probe {
                                        service: Service {
                                            resource,
                                            committed,
                                            publication,
                                        },
                                        projected,
                                    })
                                },
                                core::ServeOptions {
                                    lifetime: core::Lifetime::Host,
                                    ..Default::default()
                                },
                            )
                        },
                    )
                    .unwrap(),
                    core::LoadOptions::default(),
                )
                .unwrap()
                .await
                .unwrap();
                let probe = host
                    .get(AdmissionProbe, AdmissionProbeInput {})
                    .await
                    .unwrap();
                probe
                    .write()
                    .await
                    .expect("a confirmed append does not depend on State publication");
                assert_eq!(
                    probe.committed().value().unwrap().commit_sequence,
                    1,
                    "derived failure must prevent adoption"
                );
                assert_eq!(
                    probe
                        .read_open()
                        .await
                        .expect("SQLite open must survive reactive refusal")
                        .head,
                    1
                );
                assert_eq!(
                    probe
                        .read_events()
                        .await
                        .expect("SQLite events must survive reactive refusal")
                        .events
                        .len(),
                    1
                );
                assert_eq!(
                    probe
                        .read_projection()
                        .await
                        .expect("SQLite projection must survive reactive refusal")
                        .entries
                        .len(),
                    1
                );
                let status = probe.publication_status().await.unwrap();
                assert!(status.pending);
                assert!(status.failures >= 4);
                assert!(
                    status
                        .last_error
                        .as_deref()
                        .unwrap()
                        .contains("fixture_reactive_refusal")
                );
                if !recover {
                    // Let a retry fail on its own, then close while more work is pending.
                    tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                    let after = probe.publication_status().await.unwrap();
                    assert!(after.pending);
                    assert!(after.failures > status.failures);
                    tokio::time::timeout(std::time::Duration::from_secs(2), host.close())
                        .await
                        .expect("managed retry cannot hold Host shutdown open")
                        .unwrap();
                    let reopened = Store::open(&directory.path().join("sessions.sqlite"))
                        .expect("shutdown releases ownership despite pending repair");
                    assert_eq!(reopened.head().unwrap().commit_sequence, 2);
                    reopened.close().unwrap();
                    return;
                }
                reject.set(false);
                tokio::time::timeout(std::time::Duration::from_secs(2), async {
                    while probe.committed().value().unwrap().commit_sequence != 2 {
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                })
                .await
                .expect("State reconciles without another read or write");
                let status = probe.publication_status().await.unwrap();
                assert!(!status.pending);
                assert_eq!(status.durable, status.published);
                assert_eq!(
                    probe.read_open().await.unwrap().head,
                    1,
                    "repair never appends events"
                );
                host.close().await.unwrap();
            }));
    }
}
