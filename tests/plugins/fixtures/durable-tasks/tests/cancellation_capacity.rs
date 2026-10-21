//! Definite outbox refusal signals live Tembra operations without inventing durable intent.

use std::{
    cell::{Cell, RefCell},
    future::Future,
    rc::Rc,
    time::Duration,
};

use swarmie_durable_tasks_fixture::{
    hooks::{self, Observer},
    outbox_consumer::{self, Consumer},
};
use swarmie_task_contracts::*;
use tembra::{core, prelude::*};
use tembra_native::TokioDriver;

#[derive(Default)]
struct Control {
    entered: Cell<bool>,
    cancelled: Cell<bool>,
    lease: RefCell<Option<core::InvocationLease>>,
}

impl Control {
    fn release(&self) {
        self.lease.borrow_mut().take();
    }
}

#[tembra::contract(local, name = "fixture.capacity-cancellation", version = 1)]
trait CapacityHandler {
    #[portable]
    async fn step(&self, invocation: TaskInvocation) -> Result<TaskTransition>;
}

struct Handler {
    control: Rc<Control>,
    owner: core::InvocationOwner,
}

fn task(id: &str, phase: &str) -> TaskSpec {
    TaskSpec {
        id: id.into(),
        kind: "fixture.capacity-cancellation".into(),
        definition_version: 1,
        input: serde_json::Value::Null,
        checkpoint: serde_json::Value::Null,
        phase: phase.into(),
    }
}

impl CapacityHandlerProvider for Handler {
    async fn step(&self, invocation: TaskInvocation, context: Context) -> Result<TaskTransition> {
        let TaskInvocation::Run { task: record, .. } = invocation else {
            return Err(RuntimeError::invalid(
                "fixture_mode",
                "migration is unsupported",
            ));
        };
        if record.phase == "spawn" {
            return Ok(TaskTransition::Wait {
                checkpoint: serde_json::Value::Null,
                phase: "done".into(),
                children: vec![task(&format!("{}:child", record.id), "held")],
                task_ids: vec![],
                policy: WaitPolicy::AllSettled,
            });
        }
        *self.control.lease.borrow_mut() = Some(self.owner.retain(context.core())?);
        self.control.entered.set(true);
        let error = context.cancellation().cancelled().await;
        self.control.cancelled.set(true);
        Err(error)
    }
}

fn handler(control: Rc<Control>) -> Result<NativeFacet> {
    NativeFacet::new("fixture.capacity-cancellation", vec![], move |_, setup| {
        let control = control.clone();
        setup.serve_with_options(
            CapacityHandler,
            move |_, _, declaration| {
                declaration.core_mut().register_method(
                    &task_definitions()?,
                    "fixture.capacity-cancellation",
                    serde_json::to_value(TaskDefinition {
                        kind: "fixture.capacity-cancellation".into(),
                        version: 1,
                    })
                    .unwrap(),
                    "step",
                )?;
                Ok(Handler {
                    control: control.clone(),
                    owner: declaration.core_mut().invocation_owner(),
                })
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )
    })
}

fn run<F, Fut>(test: F)
where
    F: FnOnce(Rc<TokioDriver>) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let driver = TokioDriver::new();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(driver.run_until(async {
            tokio::time::timeout(Duration::from_secs(15), test(driver.clone()))
                .await
                .expect("capacity cancellation timed out")
                .unwrap();
        }));
}

struct Rig {
    host: Host,
    tasks: DurableTasksRef,
    consumer: Consumer,
    control: Rc<Control>,
    observer: Rc<Observer>,
    _directory: tempfile::TempDir,
}

async fn open(driver: Rc<TokioDriver>, events: u64) -> Result<Rig> {
    let directory = tempfile::tempdir().unwrap();
    let host = Host::with_driver("capacity-cancellation", "test", driver.clone())?;
    host.install(
        swarmie_task_storage_sqlite::facet(&swarmie_task_storage_sqlite::Configuration {
            outbox_legacy_owners: Default::default(),
            database: directory.path().join("tasks.sqlite"),
            outbox: swarmie_task_storage_sqlite::TaskOutboxLimits {
                max_events: events,
                max_bytes: 1024 * 1024,
                max_consumers: 1,
            },
        })?,
        core::LoadOptions::default(),
    )?
    .await?;
    let storage = host.get(TaskStorage, TaskStorageInput {}).await?;
    let consumer = outbox_consumer::install(&host, "fixture.capacity-publisher", storage).await?;
    consumer
        .register_outbox_consumer(TaskOutboxConsumer {
            id: "publisher".into(),
        })
        .await?;
    let control = Rc::new(Control::default());
    let observer = Rc::new(Observer::default());
    host.install(handler(control.clone())?, core::LoadOptions::default())?
        .await?;
    host.install(
        hooks::facet("audit", 0, observer.clone())?,
        core::LoadOptions::default(),
    )?
    .await?;
    host.install(
        swarmie_durable_tasks::facet(
            &swarmie_durable_tasks::Configuration {
                poll_interval_ms: 2,
                ..Default::default()
            },
            driver,
        )?,
        core::LoadOptions::default(),
    )?
    .await?;
    let tasks = host.get(DurableTasks, DurableTasksInput {}).await?;
    Ok(Rig {
        host,
        tasks,
        consumer,
        control,
        observer,
        _directory: directory,
    })
}

async fn until(mut condition: impl FnMut() -> bool) {
    while !condition() {
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

async fn read(storage: &Consumer) -> Result<TaskOutboxPage> {
    storage
        .read_outbox(TaskOutboxQuery {
            consumer_id: "publisher".into(),
            after: None,
            limit: 256,
        })
        .await
}

async fn drain(storage: &Consumer) -> Result<()> {
    let page = read(storage).await?;
    storage
        .acknowledge_outbox(TaskOutboxAcknowledgement {
            consumer_id: "publisher".into(),
            through: page.next.expect("filled outbox"),
        })
        .await?;
    let retention = storage
        .compact_outbox(TaskOutboxCompaction { limit: 4096 })
        .await?;
    assert_eq!(retention.retained_events, 0);
    Ok(())
}

fn cancellation_notifications(observer: &Observer) -> usize {
    observer
        .notifications
        .borrow()
        .iter()
        .filter(|notification| notification.hook == "swarmie.task-cancellation-requested")
        .count()
}

#[test]
fn full_outbox_signals_active_root_but_retry_is_required_for_durable_cancellation() {
    run(|driver| async move {
        let rig = open(driver, 2).await?;
        rig.tasks.create(task("root", "held")).await?;
        until(|| rig.control.entered.get()).await;
        let before = read(&rig.consumer).await?;
        assert_eq!(before.events.len(), 2);
        let error = rig
            .tasks
            .cancel(TaskLookup { id: "root".into() })
            .await
            .unwrap_err();
        assert_eq!(error.reason, "durable_tasks_cancellation_unconfirmed");
        assert!(error.message.contains("intent was not persisted"));
        assert!(error.message.contains("1 active invocations"));
        until(|| rig.control.cancelled.get()).await;
        let unconfirmed = rig
            .tasks
            .get(TaskLookup { id: "root".into() })
            .await?
            .unwrap();
        assert_eq!(unconfirmed.status, TaskStatus::Running);
        assert!(!unconfirmed.cancellation_requested);
        assert_eq!(read(&rig.consumer).await?.events, before.events);
        assert_eq!(cancellation_notifications(&rig.observer), 0);
        assert!(
            !rig.observer
                .notifications
                .borrow()
                .iter()
                .any(|notification| notification.hook == "swarmie.task-phase-settled")
        );
        drain(&rig.consumer).await?;
        let confirmed = rig.tasks.cancel(TaskLookup { id: "root".into() }).await?;
        assert!(confirmed.cancellation_requested);
        rig.control.release();
        assert!(matches!(
            rig.tasks.wait(TaskLookup { id: "root".into() }).await?,
            TaskOutcome::Aborted { .. }
        ));
        until(|| cancellation_notifications(&rig.observer) > 0).await;
        let after = read(&rig.consumer).await?;
        assert_eq!(after.events.len(), 2);
        assert!(after.events[0].cancellation_requested);
        assert_eq!(after.events[1].current_status, TaskStatus::Terminal);
        assert_eq!(after.events[1].outcome, Some(TaskOutcomeKind::Aborted));
        rig.host.close().await
    });
}

#[test]
fn full_outbox_parent_cancellation_signals_owned_child_without_inventing_family_intent() {
    run(|driver| async move {
        let rig = open(driver, 5).await?;
        rig.tasks.create(task("parent", "spawn")).await?;
        until(|| rig.control.entered.get()).await;
        let before = read(&rig.consumer).await?;
        assert_eq!(before.events.len(), 5);
        let error = rig
            .tasks
            .cancel(TaskLookup {
                id: "parent".into(),
            })
            .await
            .unwrap_err();
        assert_eq!(error.reason, "durable_tasks_cancellation_unconfirmed");
        assert!(error.message.contains("1 active invocations"));
        until(|| rig.control.cancelled.get()).await;
        for (id, status) in [
            ("parent", TaskStatus::Waiting),
            ("parent:child", TaskStatus::Running),
        ] {
            let record = rig.tasks.get(TaskLookup { id: id.into() }).await?.unwrap();
            assert_eq!(record.status, status);
            assert!(!record.cancellation_requested);
        }
        assert_eq!(read(&rig.consumer).await?.events, before.events);
        assert_eq!(cancellation_notifications(&rig.observer), 0);
        drain(&rig.consumer).await?;
        assert!(
            rig.tasks
                .cancel(TaskLookup {
                    id: "parent".into()
                })
                .await?
                .cancellation_requested
        );
        rig.control.release();
        for id in ["parent:child", "parent"] {
            assert!(matches!(
                rig.tasks.wait(TaskLookup { id: id.into() }).await?,
                TaskOutcome::Aborted { .. }
            ));
        }
        rig.host.close().await
    });
}
