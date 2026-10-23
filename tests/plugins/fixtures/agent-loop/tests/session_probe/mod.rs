//! A SessionStore proxy that attributes current-turn history reads to the durable phase
//! invocation performing them and can cancel a turn as soon as a chosen receipt is durable.

use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};
use swarmie_agent_contracts::{AgentLoopRef, TurnLookup};
use swarmie_session_contracts::*;
use swarmie_task_contracts::{TaskQuery, TaskStatus, TaskStorageRef};
use tembra::prelude::*;

/// Cancels one turn through the Agent Contract after an appended batch with this event kind.
#[derive(Clone)]
pub struct CancelAfter {
    pub kind: String,
    pub turn: TurnLookup,
    pub agent: AgentLoopRef,
}

#[derive(Clone, Default)]
pub struct Probe {
    /// Durable source that names the running phase invocation of each history read.
    pub storage: Rc<RefCell<Option<TaskStorageRef>>>,
    /// First pages of current-turn history reads, keyed by running task revision and phase.
    pub reads: Rc<RefCell<BTreeMap<(u64, String), u32>>>,
    pub cancel_after: Rc<RefCell<Option<CancelAfter>>>,
    /// Whether the requested cancellation was committed, once it was attempted.
    pub cancelled: Rc<RefCell<Option<Result<bool>>>>,
    pub armed: Rc<Cell<bool>>,
    /// Successful append requests, retaining exact stable batch composition.
    pub batches: Rc<RefCell<Vec<AppendEvents>>>,
    /// Return one lost-response error after this kind was durably appended.
    pub fail_after: Rc<RefCell<Option<String>>>,
    pub repeat_effect_fences: Rc<Cell<bool>>,
    /// Hold one durable append reply until the owning phase deadline cancels it.
    pub pause_after: Rc<RefCell<Option<String>>>,
}

impl Probe {
    /// Drops the References held for the proxy so Host retirement owns every claim.
    pub fn release(&self) {
        self.storage.borrow_mut().take();
        self.cancel_after.borrow_mut().take();
    }
}

pub fn facet(backend: SessionStoreRef, initial: StoreHead, probe: Probe) -> Result<NativeFacet> {
    NativeFacet::new("fixture.session-probe", vec![], move |_, setup| {
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
    async fn identity(&self, _context: Context) -> Result<SessionStoreIdentity> {
        self.backend.identity().await
    }

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
        self.backend.open(request).await
    }

    async fn append(&self, request: AppendEvents, context: Context) -> Result<AppendReceipt> {
        let kinds = request
            .events
            .iter()
            .map(|event| event.kind.clone())
            .collect::<Vec<_>>();
        let receipt = self.backend.append(request.clone()).await?;
        if self.probe.repeat_effect_fences.get()
            && kinds
                .iter()
                .any(|kind| kind == "swarmie.agent.model-started")
        {
            assert_eq!(
                self.backend.append(request.clone()).await?,
                receipt,
                "an identical fence retry retains its original sequence range"
            );
        }
        self.probe.batches.borrow_mut().push(request);
        let pause = self.probe.pause_after.borrow().clone();
        if pause.as_ref().is_some_and(|kind| kinds.contains(kind)) {
            self.probe.pause_after.borrow_mut().take();
            return Err(context.cancellation().cancelled().await);
        }
        let fail = self.probe.fail_after.borrow().clone();
        if fail.as_ref().is_some_and(|kind| kinds.contains(kind)) {
            self.probe.fail_after.borrow_mut().take();
            return Err(RuntimeError::unavailable(
                "fixture_lost_append_response",
                "receipt committed but its reply was lost",
            ));
        }
        let cancellation = self.probe.cancel_after.borrow().clone();
        if let Some(cancellation) = cancellation
            && kinds.contains(&cancellation.kind)
            && !self.probe.armed.replace(true)
        {
            // The receipt is durable before cancellation commits; its caller observes both.
            let committed = cancellation
                .agent
                .cancel(cancellation.turn)
                .await
                .map(|record| record.task.cancellation_requested);
            *self.probe.cancelled.borrow_mut() = Some(committed);
        }
        Ok(receipt)
    }

    async fn events(&self, query: EventQuery, _context: Context) -> Result<EventPage> {
        // Context selection reads frozen intervals; a root's turn history read is unbounded.
        if query.turn_id.is_some() && query.through_sequence.is_none() && query.cursor.is_none() {
            let storage = self.probe.storage.borrow().clone();
            if let Some(storage) = storage {
                for task in storage.scan(TaskQuery::default()).await?.records {
                    if task.status == TaskStatus::Running {
                        *self
                            .probe
                            .reads
                            .borrow_mut()
                            .entry((task.revision, task.phase))
                            .or_default() += 1;
                    }
                }
            }
        }
        self.backend.events(query).await
    }

    async fn project(&self, request: ProjectionQuery, _context: Context) -> Result<ProjectionPage> {
        self.backend.project(request).await
    }
}
