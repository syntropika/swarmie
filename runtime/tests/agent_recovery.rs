//! Product composition: PluginManager, durable agents and history over one owner.

#[path = "support/agent_provider.rs"]
mod provider;

use futures::{
    FutureExt,
    executor::{LocalPool, block_on},
};
use serde_json::json;
use std::{
    num::{NonZeroU32, NonZeroU64},
    path::Path,
    rc::Rc,
    time::Duration,
};
use swarmie_agent_loop::{
    conversation::{self as history, History, Record},
    *,
};
use swarmie_conversations::{
    self as conversations, ConversationId, EntryDraft, Read, WriteReceipt,
};
use swarmie_durable_tasks::{self as tasks, TaskStorage};
use swarmie_plugin_manager::{
    Catalog, CorePlugin, InstalledPlugin, PluginChange, PluginId, PluginStatus, PluginStorage,
};
use swarmie_providers::Progress;
use swarmie_runtime::Runtime;
use swarmie_storage::sqlite::{SqliteOwner, SqliteTaskStorage};
use swarmie_tools::{self as tools, CatalogMethod, CatalogService, Selection, ToolDefinition};
use tembra::{Host, Orb, Reference, Service};

const CHILD_DIRECTORY: &str = "SWARMIE_AGENT_RECOVERY_CHILD_DIRECTORY";
const WAIT: &str = "fixture.recovery-wait";
const PLUGINS: [&str; 7] = [
    "tasks",
    "conversations",
    "model",
    "counter",
    "tools",
    "agent",
    "wait",
];

#[derive(Clone, Copy, PartialEq)]
enum CrashPoint {
    None,
    ToolEffect,
    AgentConfirmation,
}

fn terminal_observer(crash: CrashPoint) -> Orb {
    use swarmie_agent_loop::hooks::{ExecutionStatus, HookName, Hooks};
    Orb::new("fixture.recovery-terminal", 1)
        .requires(tembra::core::Requirement {
            orb: history::durable::SERVICE.into(),
            api_version: 1,
            methods: Default::default(),
            hooks: [HookName::AfterTurnAgent.as_str().into()].into(),
        })
        .contribute(
            history::durable::SERVICE,
            &Hooks::default().after_turn_agent,
            0,
            move |_, outcome| async move {
                if crash == CrashPoint::AgentConfirmation
                    && matches!(outcome.status, ExecutionStatus::Completed(_))
                {
                    // The shared engine has confirmed its final history/checkpoint,
                    // but the scheduler has not yet received the task result.
                    std::process::exit(0);
                }
                Ok(())
            },
        )
}

fn seed(directory: &Path) {
    provider::initialize(&directory.join("external.sqlite"));
    let inventory = SqliteOwner::new(directory.join("runtime.sqlite"))
        .inventory()
        .unwrap();
    block_on(inventory.commit(
        0,
        PLUGINS.map(|name| PluginChange::Put(plugin(name))).to_vec(),
    ))
    .unwrap();
    block_on(inventory.close()).unwrap();
}

fn expire_crashed_assignment(storage: &SqliteTaskStorage, task: &tasks::TaskRecord) {
    let now = block_on(storage.candidates(0, 1)).unwrap().now;
    let remaining = task
        .assignment
        .as_ref()
        .unwrap()
        .expires_at
        .saturating_sub(now);
    block_on(storage.close()).unwrap();
    // Respect the storage authority's lease, without fabricating a release for
    // the crashed executor or changing its persisted assignment.
    block_on(futures_timer::Delay::new(Duration::from_millis(
        remaining + 1,
    )));
}

fn plugin(name: &str) -> InstalledPlugin {
    InstalledPlugin {
        id: PluginId::new(name).unwrap(),
        version: NonZeroU32::new(1).unwrap(),
        enabled: true,
    }
}

fn tool() -> ToolDefinition {
    ToolDefinition::new(
        ToolSpec {
            name: ToolName::new("counter.add").unwrap(),
            description: "Increment the external fixture counter".into(),
            input_schema: json!({"type":"object","properties":{"amount":{"const":1}}}),
        },
        ToolProvider::new(provider::TOOL, vec!["main".into()]).unwrap(),
    )
    .unwrap()
}

fn wait_definition() -> tasks::TaskDefinition {
    tasks::TaskDefinition::new(WAIT, NonZeroU32::new(1).unwrap()).unwrap()
}

fn waiter() -> Orb {
    Orb::new(WAIT, 1).service(Service::new(WAIT, |_| Ok(())).method(
        tasks::TaskMethod::Execute.as_str(),
        |_, ctx, execution: tasks::Execution| async move {
            assert_eq!(ctx.principal()?, "alice");
            let key: tasks::SignalKey = serde_json::from_value(execution.input()).unwrap();
            if let Some(checkpoint) = execution.checkpoint() {
                assert_eq!(checkpoint, json!({"waiting":true}));
                let delivery = execution.signal().unwrap();
                assert_eq!(delivery.key, key);
                assert_eq!(delivery.value, json!(7));
                Ok(tasks::TaskOutcome::Completed(delivery.value))
            } else {
                Ok(tasks::TaskOutcome::Waiting {
                    checkpoint: json!({"waiting":true}),
                    condition: tasks::WaitFor::Signal(key),
                })
            }
        },
    ))
}

fn open(
    directory: &Path,
    streaming: bool,
    crash: CrashPoint,
    unknown: bool,
) -> (LocalPool, Runtime, [Rc<Reference>; 3]) {
    let mut pool = LocalPool::new();
    let owner = SqliteOwner::new(directory.join("runtime.sqlite"));
    let storage = owner.clone();
    let conversation_storage = owner.clone();
    let spawner = pool.spawner();
    let external = directory.join("external.sqlite");
    let counter = external.clone();
    let catalog = Catalog::new([
        CorePlugin::new(plugin("tasks").id, plugin("tasks").version, move |ctx| {
            let mut options = tasks::Options::new(
                spawner.clone(),
                NonZeroU64::new(1_000).unwrap(),
                Rc::new(|duration| {
                    async move { futures_timer::Delay::new(duration).await }.boxed_local()
                }),
            );
            options.max_executions = 1;
            Ok(vec![tasks::orb(
                ctx.host,
                storage.task_factory(),
                tasks::Definitions::from_plugins(ctx.index),
                options,
            )])
        }),
        CorePlugin::new(
            plugin("conversations").id,
            plugin("conversations").version,
            move |_| {
                Ok(vec![conversations::orb(
                    conversation_storage.conversation_factory(),
                )])
            },
        ),
        CorePlugin::new(plugin("model").id, plugin("model").version, move |_| {
            Ok(vec![provider::model(external.clone())])
        }),
        CorePlugin::new(plugin("counter").id, plugin("counter").version, move |_| {
            Ok(vec![provider::tool(
                counter.clone(),
                crash == CrashPoint::ToolEffect,
                unknown,
            )])
        })
        .contributes(tool()),
        CorePlugin::new(plugin("tools").id, plugin("tools").version, |ctx| {
            Ok(vec![tools::orb(
                ctx.host,
                tools::Catalog::from_plugins(ctx.index),
            )])
        }),
        CorePlugin::new(plugin("agent").id, plugin("agent").version, move |ctx| {
            let host = ctx.host;
            Ok(vec![
                history::durable::orb(move |declaration| {
                    let agent = Agent::new(
                        Rc::new(ModelService::bind(
                            &declaration,
                            host.clone(),
                            provider::MODEL,
                            vec!["main".into()],
                        )?),
                        Rc::new(CatalogService::bind(
                            &declaration,
                            host.clone(),
                            tools::SERVICE,
                            vec![tools::INSTANCE_KEY.into()],
                        )?),
                    );
                    let agent = if streaming {
                        agent.with_progress(Progress::bind(&declaration)?)
                    } else {
                        agent
                    };
                    Ok((
                        agent,
                        History::bind(
                            &declaration,
                            host.clone(),
                            conversations::SERVICE,
                            vec![conversations::INSTANCE_KEY.into()],
                        )?,
                    ))
                }),
                terminal_observer(crash),
            ])
        })
        .requires(["tasks", "conversations", "tools", "model"].map(|name| plugin(name).id))
        .contributes(history::durable::definition()),
        CorePlugin::new(plugin("wait").id, plugin("wait").version, |_| {
            Ok(vec![waiter()])
        })
        .contributes(wait_definition()),
    ])
    .unwrap();
    let runtime = pool
        .run_until(Runtime::open(
            pool.spawner(),
            owner.inventory_factory(),
            catalog,
        ))
        .unwrap();
    let snapshot = pool.run_until(runtime.plugins()).unwrap();
    assert_eq!(snapshot.plugins.len(), PLUGINS.len());
    assert!(
        snapshot
            .plugins
            .iter()
            .all(|p| matches!(p.observed, PluginStatus::Mounted { .. }) && !p.restart_required)
    );
    // The embedding owns its singleton claims for its entire lifetime. Dropping
    // a last claim between two calls would request disposal of that dependency.
    let claims = [
        reference(runtime.host(), tasks::SERVICE, "host"),
        reference(runtime.host(), conversations::SERVICE, "host"),
        reference(runtime.host(), tools::SERVICE, "alice"),
    ];
    for claim in &claims {
        pool.run_until(claim.ready()).unwrap();
    }
    (pool, runtime, claims)
}

fn reference(host: &Host, service: &str, principal: &str) -> Rc<Reference> {
    Rc::new(host.get(service, vec!["main".into()], principal).unwrap())
}

fn call<A: 'static, R: 'static>(
    pool: &mut LocalPool,
    host: &Host,
    reference: &Rc<Reference>,
    principal: &str,
    method: &str,
    arguments: A,
) -> R {
    let reference = reference.clone();
    let method = method.to_owned();
    pool.run_until(host.root(principal, move |ctx| {
        async move { reference.call(&ctx, &method, arguments).await }.boxed_local()
    }))
    .unwrap()
}

fn resume(pool: &mut LocalPool, runtime: &Runtime) {
    let scheduler = reference(runtime.host(), tasks::SERVICE, "host");
    call::<_, ()>(
        pool,
        runtime.host(),
        &scheduler,
        "host",
        tasks::Method::Resume.as_str(),
        (),
    );
    pool.run_until_stalled();
}

fn task(pool: &mut LocalPool, runtime: &Runtime, id: tasks::TaskId) -> tasks::TaskRecord {
    let scheduler = reference(runtime.host(), tasks::SERVICE, "host");
    call::<_, Option<tasks::TaskRecord>>(
        pool,
        runtime.host(),
        &scheduler,
        "alice",
        tasks::Method::Get.as_str(),
        (id,),
    )
    .unwrap()
}

fn create_agent(pool: &mut LocalPool, runtime: &Runtime, with_tools: bool) -> tasks::TaskId {
    let host = runtime.host();
    let conversation = reference(host, conversations::SERVICE, "host");
    let created: WriteReceipt = call(
        pool,
        host,
        &conversation,
        "alice",
        conversations::Method::Create.as_str(),
        (ConversationId::new(),),
    );
    let written: WriteReceipt = call(
        pool,
        host,
        &conversation,
        "alice",
        conversations::Method::Append.as_str(),
        (
            created.head,
            vec![
                EntryDraft::encode(Record::Message(Message::User {
                    content: "Add one.".into(),
                }))
                .unwrap(),
            ],
        ),
    );
    let catalog = reference(host, tools::SERVICE, "alice");
    let selection: Selection = call(
        pool,
        host,
        &catalog,
        "alice",
        CatalogMethod::List.as_str(),
        (),
    );
    assert_eq!(selection.tools.len(), 1);
    let request = history::Request {
        at: written.head,
        instructions: "Use the supplied tools.".into(),
        tools: if with_tools {
            selection.tools.into_iter().map(|tool| tool.spec).collect()
        } else {
            vec![]
        },
        max_turns: NonZeroU32::new(3).unwrap(),
    };
    let scheduler = reference(host, tasks::SERVICE, "host");
    call(
        pool,
        host,
        &scheduler,
        "alice",
        tasks::Method::Create.as_str(),
        (tasks::TaskSpec {
            id: tasks::TaskId::new(),
            definition: history::durable::definition(),
            input: serde_json::to_value(request).unwrap(),
        },),
    )
}

fn verify(streaming: bool, test_name: &str) {
    if let Some(directory) = std::env::var_os(CHILD_DIRECTORY) {
        let directory = Path::new(&directory);
        seed(directory);
        let (mut pool, runtime, _claims) =
            open(directory, streaming, CrashPoint::ToolEffect, false);
        let scheduler = reference(runtime.host(), tasks::SERVICE, "host");
        let wait: tasks::TaskId = call(
            &mut pool,
            runtime.host(),
            &scheduler,
            "alice",
            tasks::Method::Create.as_str(),
            (tasks::TaskSpec {
                id: tasks::TaskId::new(),
                definition: wait_definition(),
                input: serde_json::to_value(tasks::SignalKey::new()).unwrap(),
            },),
        );
        // Finish the wait transition first. Capacity one must then admit the agent.
        resume(&mut pool, &runtime);
        assert!(matches!(
            task(&mut pool, &runtime, wait).status,
            tasks::TaskStatus::Waiting(_)
        ));
        let id = create_agent(&mut pool, &runtime, true);
        pool.run_until_stalled();
        let status: tasks::SchedulerStatus = call(
            &mut pool,
            runtime.host(),
            &scheduler,
            "host",
            tasks::Method::Inspect.as_str(),
            (),
        );
        panic!(
            "fixture must exit inside the first external tool dispatch: {:?}, {status:?}",
            task(&mut pool, &runtime, id)
        );
    }

    let directory = tempfile::tempdir().unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test_name])
        .env(CHILD_DIRECTORY, directory.path())
        .status()
        .unwrap();
    assert!(status.success());
    let external = directory.path().join("external.sqlite");
    assert_eq!(provider::effects(&external), 1);
    assert_eq!(provider::count(&external, "tool.execute"), 1);
    let storage = SqliteTaskStorage::open(directory.path().join("runtime.sqlite")).unwrap();
    let records: Vec<_> = block_on(storage.list(None, tasks::TaskPageRequest::default()))
        .unwrap()
        .into_iter()
        .map(|summary| block_on(storage.get(summary.id)).unwrap().unwrap())
        .collect();
    assert_eq!(records.len(), 2);
    let pending = records
        .iter()
        .find(|r| r.spec.definition == history::durable::definition())
        .unwrap();
    let waiting = records
        .iter()
        .find(|r| r.spec.definition == wait_definition())
        .unwrap();
    assert!(matches!(
        waiting.status,
        tasks::TaskStatus::Waiting(tasks::WaitState::Signal(_))
    ));
    assert!(waiting.assignment.is_none());
    assert_eq!(pending.status, tasks::TaskStatus::Ready);
    assert_eq!(
        pending.checkpoint.as_ref().unwrap()["phase"]["kind"],
        "tool_pending"
    );
    expire_crashed_assignment(&storage, pending);

    let (mut pool, runtime, claims) = open(directory.path(), streaming, CrashPoint::None, true);
    assert_eq!(
        task(&mut pool, &runtime, pending.spec.id).revision,
        pending.revision
    );
    assert_eq!(
        provider::count(&external, "tool.recover"),
        0,
        "startup remains paused"
    );
    let independent = create_agent(&mut pool, &runtime, false);
    resume(&mut pool, &runtime);
    let unknown = task(&mut pool, &runtime, pending.spec.id);
    assert_eq!(unknown.status, tasks::TaskStatus::Ready);
    assert_eq!(unknown.checkpoint, pending.checkpoint);
    assert!(unknown.assignment.is_none());
    assert_eq!(provider::count(&external, "tool.execute"), 1);
    assert_eq!(provider::count(&external, "tool.recover"), 1);
    assert!(
        matches!(
            task(&mut pool, &runtime, independent).status,
            tasks::TaskStatus::Completed(_)
        ),
        "an unresolved agent cannot block unrelated work"
    );
    pool.run_until(runtime.close()).unwrap();
    drop((pool, claims));

    let (mut pool, runtime, claims) = open(directory.path(), streaming, CrashPoint::None, false);
    resume(&mut pool, &runtime);
    let completed = task(&mut pool, &runtime, pending.spec.id);
    let tasks::TaskStatus::Completed(result) = &completed.status else {
        panic!("agent failed: {completed:?}");
    };
    let output: Output = serde_json::from_value(result.clone()).unwrap();
    assert_eq!(output.reason, FinishReason::Complete);
    assert!(completed.assignment.is_none());
    let checkpoint: history::Checkpoint =
        serde_json::from_value(completed.checkpoint.clone().unwrap()).unwrap();
    let conversation = reference(runtime.host(), conversations::SERVICE, "host");
    let page: conversations::Page = call(
        &mut pool,
        runtime.host(),
        &conversation,
        "alice",
        conversations::Method::Read.as_str(),
        Read::new(checkpoint.head()),
    );
    let messages: Vec<_> = page
        .entries
        .iter()
        .filter_map(|entry| match entry.decode::<Record>().unwrap() {
            Record::Message(message) => Some(message),
            _ => None,
        })
        .collect();
    assert_eq!(messages.len(), 4);
    assert!(matches!(&messages[0], Message::User { .. }));
    assert!(matches!(&messages[1], Message::Assistant(message) if message.tool_calls.len() == 1));
    assert!(matches!(&messages[2], Message::Tool { .. }));
    assert!(
        matches!(&messages[3], Message::Assistant(message) if message.content == "Done" && message.tool_calls.is_empty())
    );
    assert!(
        !serde_json::to_string(&page)
            .unwrap()
            .contains("transient preview")
    );
    let current: Option<conversations::Conversation> = call(
        &mut pool,
        runtime.host(),
        &conversation,
        "alice",
        conversations::Method::Get.as_str(),
        (checkpoint.head().conversation,),
    );
    assert_eq!(current.unwrap().head, checkpoint.head());
    let scheduler = reference(runtime.host(), tasks::SERVICE, "host");
    let key: tasks::SignalKey = serde_json::from_value(waiting.spec.input.clone()).unwrap();
    call::<_, ()>(
        &mut pool,
        runtime.host(),
        &scheduler,
        "alice",
        tasks::Method::Signal.as_str(),
        (waiting.spec.id, key, json!(7)),
    );
    pool.run_until_stalled();
    assert_eq!(
        task(&mut pool, &runtime, waiting.spec.id).status,
        tasks::TaskStatus::Completed(json!(7))
    );
    assert_eq!(provider::effects(&external), 1);
    assert_eq!(provider::count(&external, "tool.execute"), 1);
    assert_eq!(provider::count(&external, "tool.recover"), 2);
    assert_eq!(provider::count(&external, "model.recover"), 0);
    let method = if streaming {
        "model.stream"
    } else {
        "model.complete"
    };
    assert_eq!(provider::count(&external, method), 3);
    let hidden: Option<tasks::TaskRecord> = call(
        &mut pool,
        runtime.host(),
        &scheduler,
        "bob",
        tasks::Method::Get.as_str(),
        (pending.spec.id,),
    );
    assert!(hidden.is_none());
    let host = runtime.host().clone();
    pool.run_until(runtime.close()).unwrap();
    assert_eq!(
        (
            host.kernel().diagnostics().calls,
            host.kernel().diagnostics().resources
        ),
        (0, 0)
    );
    drop((pool, scheduler, conversation, host, claims));

    let (mut pool, runtime, _claims) = open(directory.path(), streaming, CrashPoint::None, false);
    resume(&mut pool, &runtime);
    assert_eq!(task(&mut pool, &runtime, pending.spec.id), completed);
    assert_eq!(provider::count(&external, method), 3);
    assert_eq!(provider::count(&external, "tool.execute"), 1);
    assert_eq!(provider::count(&external, "tool.recover"), 2);
    let conversation = reference(runtime.host(), conversations::SERVICE, "host");
    let persisted: conversations::Page = call(
        &mut pool,
        runtime.host(),
        &conversation,
        "alice",
        conversations::Method::Read.as_str(),
        Read::new(checkpoint.head()),
    );
    assert_eq!(persisted, page);
    pool.run_until(runtime.close()).unwrap();
}

#[test]
fn confirmed_final_history_recovers_before_task_completion_without_dispatch() {
    const NAME: &str = "confirmed_final_history_recovers_before_task_completion_without_dispatch";
    if let Some(directory) = std::env::var_os(CHILD_DIRECTORY) {
        let directory = Path::new(&directory);
        seed(directory);
        let (mut pool, runtime, _claims) =
            open(directory, true, CrashPoint::AgentConfirmation, false);
        let id = create_agent(&mut pool, &runtime, true);
        resume(&mut pool, &runtime);
        panic!(
            "expected terminal observer to exit before task result: {:?}",
            task(&mut pool, &runtime, id)
        );
    }
    let directory = tempfile::tempdir().unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", NAME])
        .env(CHILD_DIRECTORY, directory.path())
        .status()
        .unwrap();
    assert!(status.success());
    let storage = SqliteTaskStorage::open(directory.path().join("runtime.sqlite")).unwrap();
    let records = block_on(storage.list(None, tasks::TaskPageRequest::default())).unwrap();
    assert_eq!(records.len(), 1);
    let pending = block_on(storage.get(records[0].id)).unwrap().unwrap();
    assert_eq!(pending.status, tasks::TaskStatus::Ready);
    assert_eq!(
        pending.checkpoint.as_ref().unwrap()["phase"]["kind"],
        "finished"
    );
    expire_crashed_assignment(&storage, &pending);
    let external = directory.path().join("external.sqlite");
    assert_eq!(provider::count(&external, "model.stream"), 2);
    assert_eq!(provider::effects(&external), 1);
    let (mut pool, runtime, _claims) = open(directory.path(), true, CrashPoint::None, false);
    resume(&mut pool, &runtime);
    let completed = task(&mut pool, &runtime, pending.spec.id);
    assert!(matches!(completed.status, tasks::TaskStatus::Completed(_)));
    assert_eq!(completed.checkpoint, pending.checkpoint);
    assert!(completed.assignment.is_none());
    assert_eq!(provider::count(&external, "model.stream"), 2);
    assert_eq!(provider::count(&external, "model.recover"), 0);
    assert_eq!(provider::count(&external, "tool.execute"), 1);
    assert_eq!(provider::count(&external, "tool.recover"), 0);
    pool.run_until(runtime.close()).unwrap();
}

#[test]
fn durable_product_recovers_external_effect_after_process_exit() {
    verify(
        false,
        "durable_product_recovers_external_effect_after_process_exit",
    );
}

#[test]
fn streaming_durable_product_recovers_external_effect_after_process_exit() {
    verify(
        true,
        "streaming_durable_product_recovers_external_effect_after_process_exit",
    );
}
