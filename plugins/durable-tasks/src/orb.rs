use crate::{
    Definitions, Options, StorageFactory, TaskId, TaskPageRequest, TaskSpec,
    authority::{Authority, Reason, error, storage_error},
    execution::WriteRequest,
    scheduler::Engine,
};
use futures::FutureExt;
use std::rc::Rc;
use tembra::{Code, Host, Orb, Service};

pub const SERVICE: &str = "swarmie.tasks";
pub const HOST_PRINCIPAL: &str = "host";
pub const INSTANCE_KEY: &str = "main";
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Method {
    Create,
    Signal,
    Get,
    List,
    Resume,
    Wake,
    Inspect,
    /// `(TaskId,)` returns [`crate::TaskObservation`] for the task owner or host.
    Observe,
    /// `(TaskId,)` waits for local settlement or interruption. Cancellation releases
    /// only this observer. It does not cancel the task or reconcile external effects.
    Wait,
    /// `(TaskId,)` returns `()` and clears only this owner's local hold. The caller
    /// must choose recovery explicitly and reconcile unknown effects before retry.
    /// This operation does not enable a paused scheduler.
    WakeTask,
}

impl Method {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Signal => "signal",
            Self::Get => "get",
            Self::List => "list",
            Self::Resume => "resume",
            Self::Wake => "wake",
            Self::Inspect => "inspect",
            Self::Observe => "observe",
            Self::Wait => "wait",
            Self::WakeTask => "wake_task",
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum WriteMethod {
    Commit,
}

impl WriteMethod {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Commit => "commit",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskMethod {
    Execute,
}

impl TaskMethod {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Execute => "execute",
        }
    }
}

#[derive(Clone, Copy)]
enum ResourceName {
    Storage,
    Engine,
}

impl ResourceName {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Storage => "tasks.storage",
            Self::Engine => "tasks.engine",
        }
    }
}

/// Declares a capability lease and an engine-owned scheduler resource.
/// Startup is paused. Resume enables scheduling and returns without owning its lifetime.
/// Close signals the engine, joins physical handlers/writes, then releases its lease.
///
/// # Errors
/// Activation refuses invalid owner access or unavailable storage. Missing task
/// definitions remain scheduler metadata rather than failing unrelated tasks.
pub fn orb(host: Host, storage: StorageFactory, definitions: Definitions, options: Options) -> Orb {
    let service = Service::new(SERVICE, move |declaration| {
        let address = declaration.address()?;
        if address.principal != HOST_PRINCIPAL
            || !address.key.iter().map(String::as_str).eq([INSTANCE_KEY])
        {
            return Err(error(
                Code::Forbidden,
                Reason::Forbidden,
                "tasks has one host instance",
            ));
        }
        let factory = storage.clone();
        let stored = declaration.resource_async(
            ResourceName::Storage.as_str(),
            1,
            &[],
            move |_| async move { factory().await.map_err(storage_error) }.boxed_local(),
            |storage, _| async move { storage.close().await.map_err(storage_error) }.boxed_local(),
        )?;
        let (host, definitions, options) = (host.clone(), definitions.clone(), options.clone());
        declaration.resource_async(
            ResourceName::Engine.as_str(),
            1,
            &[stored.key()],
            move |signal| {
                async move {
                    let authority = Rc::new(Authority::new(stored.value()?.as_ref().clone()));
                    authority
                        .storage
                        .candidates(0, 1)
                        .await
                        .map_err(storage_error)?;
                    signal.check()?;
                    Engine::start(host, authority, definitions, options, signal)
                }
                .boxed_local()
            },
            |engine, _| async move { engine.close().await }.boxed_local(),
        )
    })
    .lifetime(tembra::core::Lifetime::Host, 0)
    .method(
        Method::Create.as_str(),
        |engine, context, (spec,): (TaskSpec,)| async move {
            engine.value()?.create(context, spec).await
        },
    )
    .method(
        Method::Signal.as_str(),
        |engine, context, (id, key, value): (TaskId, crate::SignalKey, serde_json::Value)| async move {
            engine.value()?.signal(context, id, key, value).await
        },
    )
    .method(
        Method::Get.as_str(),
        |engine, context, (id,): (TaskId,)| async move {
            context.check()?;
            let principal = context.principal()?;
            let task = engine
                .value()?
                .authority
                .storage
                .get(id)
                .await
                .map_err(storage_error)?;
            Ok(task.filter(|task| principal == HOST_PRINCIPAL || task.principal == principal))
        },
    )
    .method(
        Method::List.as_str(),
        |engine, context, (page,): (TaskPageRequest,)| async move {
            context.check()?;
            let principal = context.principal()?;
            engine
                .value()?
                .authority
                .storage
                .list(
                    if principal == HOST_PRINCIPAL {
                        None
                    } else {
                        Some(principal)
                    },
                    TaskPageRequest {
                        limit: page.limit.min(TaskPageRequest::MAX_LIMIT),
                        ..page
                    },
                )
                .await
                .map_err(storage_error)
        },
    )
    .method(
        Method::Resume.as_str(),
        |engine, context, (): ()| async move { engine.value()?.resume(&context) },
    )
    .method(
        Method::Wake.as_str(),
        |engine, context, (): ()| async move { engine.value()?.wake(&context) },
    )
    .method(
        Method::Observe.as_str(),
        |engine, context, (id,): (TaskId,)| async move {
            engine.value()?.observe(&context, id).await
        },
    )
    .method(
        Method::Wait.as_str(),
        |engine, context, (id,): (TaskId,)| async move {
            engine.value()?.wait(&context, id).await
        },
    )
    .method(
        Method::WakeTask.as_str(),
        |engine, context, (id,): (TaskId,)| async move {
            engine.value()?.wake_task(&context, id).await
        },
    )
    .method(
        Method::Inspect.as_str(),
        |engine, context, (): ()| async move {
            context.check()?;
            if context.principal()? != HOST_PRINCIPAL {
                return Err(error(
                    Code::Forbidden,
                    Reason::Forbidden,
                    "scheduler inspection requires host authority",
                ));
            }
            Ok(engine.value()?.status())
        },
    )
    .method(
        WriteMethod::Commit.as_str(),
        |_, context, request: WriteRequest| async move { request.apply(context).await },
    );
    Orb::new(SERVICE, 1).service(service)
}
