//! Faults affect only context selection, leaving admission and historical receipt reads valid.

use std::{
    cell::{Cell as Counter, RefCell},
    rc::Rc,
};
use swarmie_session_contracts::*;
use swarmie_task_contracts::{TaskQuery, TaskStorageRef};
use tembra::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    None,
    OpenStore,
    OpenSession,
    PageStore,
    PageSession,
}

#[derive(Clone)]
pub struct Probe {
    pub injected: Rc<Counter<bool>>,
    pub storage: Rc<RefCell<Option<TaskStorageRef>>>,
    armed: Rc<Counter<bool>>,
    fault: Fault,
}

impl Probe {
    pub fn new(fault: Fault) -> Self {
        Self {
            injected: Rc::new(Counter::new(false)),
            storage: Default::default(),
            armed: Rc::new(Counter::new(false)),
            fault,
        }
    }
}

pub fn facet(backend: SessionStoreRef, initial: StoreHead, probe: Probe) -> Result<NativeFacet> {
    NativeFacet::new("fixture.context-destination", vec![], move |_, setup| {
        let backend = backend.clone();
        let initial = initial.clone();
        let probe = probe.clone();
        let committed_definition = setup.state("committed")?;
        setup.serve(SessionStore, move |_, _, declaration| {
            Ok(Proxy {
                backend: backend.clone(),
                committed: declaration
                    .state(committed_definition.value(initial.clone())?)?
                    .readable(),
                probe: probe.clone(),
            })
        })
    })
}

struct Proxy {
    backend: SessionStoreRef,
    committed: Readable<StoreHead>,
    probe: Probe,
}

impl SessionStoreProvider for Proxy {
    fn committed(&self) -> Readable<StoreHead> {
        self.committed.clone()
    }

    async fn publication_status(&self, _context: Context) -> Result<PublicationStatus> {
        self.backend.publication_status().await
    }

    async fn create(&self, request: CreateSession, _context: Context) -> Result<SessionRecord> {
        self.backend.create(request).await
    }

    async fn open(
        &self,
        request: SessionLookup,
        _context: Context,
    ) -> Result<Option<SessionRecord>> {
        let mut record = self.backend.open(request).await?;
        if self.probe.armed.replace(false)
            && !self.probe.injected.replace(true)
            && let Some(record) = &mut record
        {
            match self.probe.fault {
                Fault::OpenStore => record.store_id = "foreign-store".into(),
                Fault::OpenSession => record.session_id = "foreign-session".into(),
                _ => unreachable!("only open faults arm the next call"),
            }
        }
        Ok(record)
    }

    async fn append(&self, request: AppendEvents, _context: Context) -> Result<AppendReceipt> {
        self.backend.append(request).await
    }

    async fn events(&self, query: EventQuery, _context: Context) -> Result<EventPage> {
        let mut page = self.backend.events(query.clone()).await?;
        if query.turn_id.is_some()
            && query.through_sequence.is_none()
            && matches!(self.probe.fault, Fault::OpenStore | Fault::OpenSession)
            && !self.probe.injected.get()
        {
            let storage = self.probe.storage.borrow().clone();
            if let Some(storage) = storage {
                // The final current-turn history read precedes context open in prepare-model.
                // Inspect actual durable phase rather than relying on call counts or timing.
                if storage
                    .scan(TaskQuery::default())
                    .await?
                    .records
                    .iter()
                    .any(|task| task.phase == "prepare-model")
                {
                    self.probe.armed.set(true);
                }
            }
        }
        if query.through_sequence.is_some()
            && query.cursor.is_some()
            && matches!(self.probe.fault, Fault::PageStore | Fault::PageSession)
        {
            self.probe.injected.set(true);
            match self.probe.fault {
                Fault::PageStore => page.store_id = "foreign-store".into(),
                Fault::PageSession => page.session_id = "foreign-session".into(),
                _ => unreachable!("only page faults enter this branch"),
            }
            for event in &mut page.events {
                if event.event.kind == MESSAGE {
                    event.event.payload["content"] = serde_json::json!("foreign context");
                }
            }
        }
        Ok(page)
    }

    async fn project(&self, request: ProjectionQuery, _context: Context) -> Result<ProjectionPage> {
        self.backend.project(request).await
    }
}
