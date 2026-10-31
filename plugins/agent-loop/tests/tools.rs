#[path = "support/request.rs"]
mod input;
#[path = "support/scenario.rs"]
mod scenario;

use futures::{FutureExt, executor::LocalPool};
use std::rc::Rc;
use swarmie_agent_loop::*;
use swarmie_tools::{self as tools, Catalog, CatalogService, ToolDefinition};
use tembra::{Host, Orb, Reference, Service};

fn provider(name: &str, state: Rc<scenario::Scenario>) -> Orb {
    Orb::new(name, 1).service(
        Service::new(name, move |_| Ok(state.clone()))
            .method(
                ToolMethod::Validate.as_str(),
                |env, ctx, (spec, call): (ToolSpec, ToolCall)| async move {
                    env.validate(&ctx, &spec, &call).await
                },
            )
            .method(
                ToolMethod::Execute.as_str(),
                |env, ctx, (id, call): (OperationId, ToolCall)| async move {
                    env.execute(&ctx, ToolExecution::new(id, &call)).await
                },
            )
            .method(
                ToolMethod::Recover.as_str(),
                |env, ctx, (id, call): (OperationId, ToolCall)| async move {
                    Tools::recover(env.as_ref().as_ref(), &ctx, ToolExecution::new(id, &call)).await
                },
            ),
    )
}

fn definition(provider: &str) -> ToolDefinition {
    ToolDefinition::new(
        input::request().tools.remove(0),
        ToolProvider::new(provider, vec!["main".into()]).unwrap(),
    )
    .unwrap()
}

fn invoke<A: 'static, R: 'static>(
    pool: &mut LocalPool,
    host: &Host,
    reference: &Rc<Reference>,
    method: &str,
    args: A,
) -> R {
    let reference = reference.clone();
    let method = method.to_owned();
    pool.run_until(host.root("alice", move |ctx| {
        async move { reference.call(&ctx, &method, args).await }.boxed_local()
    }))
    .unwrap()
}

#[test]
fn pending_recovery_keeps_its_confirmed_provider_when_the_name_is_registered_elsewhere() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let original = Rc::new(scenario::Scenario::default());
    original.uncertain_tool.set(true);
    let other = Rc::new(scenario::Scenario::default());
    let catalog = Catalog::default();
    let lease = catalog.register(definition("fixture.original")).unwrap();
    let binding = host.clone();
    let model = original.clone();
    host.install(vec![
        tools::orb(host.clone(), catalog.clone()),
        provider("fixture.original", original.clone()),
        provider("fixture.other", other.clone()),
        orb(move |decl| {
            Ok(Agent::new(
                model.clone(),
                Rc::new(CatalogService::bind(
                    &decl,
                    binding.clone(),
                    tools::SERVICE,
                    vec![tools::INSTANCE_KEY.into()],
                )?),
            ))
        }),
    ])
    .unwrap();
    let reference = Rc::new(host.get(SERVICE, vec!["main".into()], "alice").unwrap());
    let first: ExecutionResult = invoke(
        &mut pool,
        &host,
        &reference,
        Method::Execute.as_str(),
        input::request(),
    );
    let saved = serde_json::to_value(first.unwrap_err().state.unwrap()).unwrap();
    assert_eq!(saved["phase"]["provider"]["service"], "fixture.original");
    let state: State = serde_json::from_value(saved).unwrap();
    drop(lease);
    let _replacement = catalog.register(definition("fixture.other")).unwrap();
    let resumed: ExecutionResult =
        invoke(&mut pool, &host, &reference, Method::Resume.as_str(), state);
    assert_eq!(resumed.unwrap().reason, FinishReason::Complete);
    assert_eq!(original.effects.borrow().len(), 1);
    assert_eq!(original.reconciled_tools.borrow().len(), 1);
    assert!(other.effects.borrow().is_empty());
    assert!(other.reconciled_tools.borrow().is_empty());
    pool.run_until(host.close()).unwrap();
    assert_eq!(host.kernel().diagnostics().resources, 0);
}

#[test]
fn safe_retry_cannot_switch_provider_and_can_continue_after_the_original_returns() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let catalog = Catalog::default();
    let old = Rc::new(scenario::Scenario::default());
    old.not_started.set(true);
    let new = Rc::new(scenario::Scenario::default());
    let lease = catalog.register(definition("fixture.old")).unwrap();
    let binding = host.clone();
    let model = old.clone();
    host.install(vec![
        tools::orb(host.clone(), catalog.clone()),
        provider("fixture.old", old.clone()),
        provider("fixture.new", new.clone()),
        orb(move |decl| {
            Ok(Agent::new(
                model.clone(),
                Rc::new(CatalogService::bind(
                    &decl,
                    binding.clone(),
                    tools::SERVICE,
                    vec![tools::INSTANCE_KEY.into()],
                )?),
            ))
        }),
    ])
    .unwrap();
    let reference = Rc::new(host.get(SERVICE, vec!["main".into()], "alice").unwrap());
    let first: ExecutionResult = invoke(
        &mut pool,
        &host,
        &reference,
        Method::Execute.as_str(),
        input::request(),
    );
    let state = *first.unwrap_err().state.unwrap();
    drop(lease);
    let replacement = catalog.register(definition("fixture.new")).unwrap();
    old.safe_retry.set(true);
    let refused: ExecutionResult =
        invoke(&mut pool, &host, &reference, Method::Resume.as_str(), state);
    let failure = refused.unwrap_err();
    assert_eq!(failure.error.code, tembra::Code::Unavailable);
    assert!(old.effects.borrow().is_empty());
    assert!(new.effects.borrow().is_empty());
    assert!(new.validations.borrow().is_empty());
    let state = *failure.state.unwrap();
    drop(replacement);
    let _returned = catalog.register(definition("fixture.old")).unwrap();
    old.safe_retry.set(true);
    let resumed: ExecutionResult =
        invoke(&mut pool, &host, &reference, Method::Resume.as_str(), state);
    resumed.unwrap();
    assert_eq!(old.effects.borrow().len(), 1);
    assert_eq!(old.tool_inputs.borrow().len(), 2);
    assert!(new.effects.borrow().is_empty());
    assert!(new.validations.borrow().is_empty());
    pool.run_until(host.close()).unwrap();
}

#[cfg(all(feature = "durable-tasks", feature = "conversations"))]
#[test]
fn history_and_task_recovery_reopen_with_the_saved_route_after_catalog_withdrawal() {
    use std::num::NonZeroU64;
    use swarmie_agent_loop::conversation::{self as history, History, Record};
    use swarmie_conversations::{self as conversations, EntryDraft, Read, WriteReceipt};
    use swarmie_durable_tasks::{self as tasks, TaskRecord, TaskStatus};
    use swarmie_storage::sqlite::SqliteOwner;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.sqlite");
    let mut pool = LocalPool::new();
    let scenario = Rc::new(scenario::Scenario::default());
    scenario.uncertain_tool.set(true);
    let mut task = tasks::TaskId::new();
    for restart in [false, true] {
        let host = Host::new(pool.spawner());
        let owner = SqliteOwner::new(&path);
        let catalog = if restart {
            Catalog::default()
        } else {
            Catalog::new([definition("fixture.tools")]).unwrap()
        };
        let binding = host.clone();
        let model = scenario.clone();
        host.install(vec![
            provider("fixture.tools", scenario.clone()),
            tools::orb(host.clone(), catalog),
            conversations::orb(owner.conversation_factory()),
            tasks::orb(
                host.clone(),
                owner.task_factory(),
                tasks::Definitions::new([history::durable::definition()]).unwrap(),
                tasks::Options::new(
                    pool.spawner(),
                    NonZeroU64::new(60_000).unwrap(),
                    Rc::new(|duration| {
                        async move { futures_timer::Delay::new(duration).await }.boxed_local()
                    }),
                ),
            ),
            history::durable::orb(move |decl| {
                Ok((
                    Agent::new(
                        model.clone(),
                        Rc::new(CatalogService::bind(
                            &decl,
                            binding.clone(),
                            tools::SERVICE,
                            vec![tools::INSTANCE_KEY.into()],
                        )?),
                    ),
                    History::bind(
                        &decl,
                        binding.clone(),
                        conversations::SERVICE,
                        vec![conversations::INSTANCE_KEY.into()],
                    )?,
                ))
            }),
        ])
        .unwrap();
        let scheduler = Rc::new(
            host.get(tasks::SERVICE, vec![tasks::INSTANCE_KEY.into()], "host")
                .unwrap(),
        );
        let history = Rc::new(
            host.get(
                conversations::SERVICE,
                vec![conversations::INSTANCE_KEY.into()],
                "host",
            )
            .unwrap(),
        );
        if !restart {
            let created: WriteReceipt = invoke(
                &mut pool,
                &host,
                &history,
                conversations::Method::Create.as_str(),
                (conversations::ConversationId::new(),),
            );
            let base = input::request();
            let written: WriteReceipt = invoke(
                &mut pool,
                &host,
                &history,
                conversations::Method::Append.as_str(),
                (
                    created.head,
                    base.context
                        .messages
                        .into_iter()
                        .map(|m| EntryDraft::encode(Record::Message(m)).unwrap())
                        .collect::<Vec<_>>(),
                ),
            );
            task = invoke(
                &mut pool,
                &host,
                &scheduler,
                tasks::Method::Create.as_str(),
                (tasks::TaskSpec {
                    id: task,
                    definition: history::durable::definition(),
                    input: serde_json::to_value(history::Request::new(
                        written.head,
                        base.context.instructions,
                        base.tools,
                        base.max_turns,
                    ))
                    .unwrap(),
                },),
            );
        }
        let scheduling = scheduler.clone();
        pool.run_until(host.root("host", move |ctx| {
            async move {
                scheduling
                    .call::<_, ()>(&ctx, tasks::Method::Resume.as_str(), ())
                    .await
            }
            .boxed_local()
        }))
        .unwrap();
        pool.run_until_stalled();
        let record: Option<TaskRecord> = invoke(
            &mut pool,
            &host,
            &scheduler,
            tasks::Method::Get.as_str(),
            (task,),
        );
        let record = record.unwrap();
        assert_eq!(scenario.effects.borrow().len(), 1);
        let saved: history::Checkpoint =
            serde_json::from_value(record.checkpoint.unwrap()).unwrap();
        let page: conversations::Page = invoke(
            &mut pool,
            &host,
            &history,
            conversations::Method::Read.as_str(),
            Read::new(saved.head()),
        );
        assert_eq!(page.entries.iter().filter(|entry|matches!(entry.decode::<Record>().unwrap(),Record::PreparedTool{provider:Some(ref p),..} if p.service=="fixture.tools")).count(),1);
        if restart {
            assert!(matches!(record.status, TaskStatus::Completed(_)));
            assert_eq!(scenario.reconciled_tools.borrow().len(), 1);
        } else {
            assert_eq!(record.status, TaskStatus::Ready);
        }
        pool.run_until(host.close()).unwrap();
        let d = host.kernel().diagnostics();
        assert_eq!((d.roots, d.calls, d.resources), (0, 0, 0));
    }
}
