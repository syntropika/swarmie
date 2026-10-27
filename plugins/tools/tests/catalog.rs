use futures::{FutureExt, executor::LocalPool};
use serde_json::json;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};
use swarmie_tools::*;
use tembra::{Code, Host, Orb, Reference, Service};

fn definition(name: &str, provider: &str) -> ToolDefinition {
    ToolDefinition::new(
        ToolSpec {
            name: ToolName::new(name).unwrap(),
            description: "Read a value".into(),
            input_schema: json!({"type":"object"}),
        },
        ToolProvider::new(provider, vec!["main".into()]).unwrap(),
    )
    .unwrap()
}

fn call(name: &str) -> ToolCall {
    ToolCall {
        id: ToolCallId::new("call-1").unwrap(),
        name: ToolName::new(name).unwrap(),
        arguments: json!({}),
    }
}

fn invoke<A: 'static, R: 'static>(
    pool: &mut LocalPool,
    host: &Host,
    reference: &Rc<Reference>,
    method: &str,
    args: A,
) -> tembra::Result<R> {
    let reference = reference.clone();
    let method = method.to_owned();
    pool.run_until(host.root("alice", move |ctx| {
        async move { reference.call(&ctx, &method, args).await }.boxed_local()
    }))
}

#[test]
fn leases_are_scoped_and_stale_owners_cannot_remove_a_replacement() {
    assert!(ToolProvider::new("fixture.files", vec![]).is_err());
    let catalog = Catalog::default();
    let original = definition("files.read", "fixture.files");
    let old = catalog.register(original.clone()).unwrap();
    assert_eq!(
        catalog.register(original.clone()).err().unwrap().code,
        Code::Conflict
    );
    assert_eq!(catalog.snapshot().unwrap().tools, vec![original.clone()]);
    old.withdraw();
    let replacement = catalog.register(original).unwrap();
    drop(old);
    assert_eq!(catalog.snapshot().unwrap().tools.len(), 1);
    drop(replacement);
    assert!(catalog.snapshot().unwrap().tools.is_empty());
}

#[test]
fn managed_claims_preserve_provider_state_across_roots_and_close_with_the_catalog() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let catalog = Catalog::new([definition("files.read", "fixture.session")]).unwrap();
    let opened = Rc::new(Cell::new(0));
    let closing = Rc::new(Cell::new(0));
    let open_count = opened.clone();
    let close_count = closing.clone();
    let (send, receive) = futures::channel::oneshot::channel::<()>();
    let gate = Rc::new(RefCell::new(Some(receive)));
    host.install(vec![
        Orb::new("fixture.session", 1).service(
            Service::new("fixture.session", move |d| {
                open_count.set(open_count.get() + 1);
                let closing = close_count.clone();
                let gate = gate.clone();
                d.resource(
                    "session",
                    1,
                    || Ok((Cell::new(false), RefCell::new(None))),
                    move |_, _| {
                        let receive = gate.borrow_mut().take();
                        closing.set(closing.get() + 1);
                        async move {
                            if let Some(receive) = receive {
                                receive.await.unwrap();
                            }
                            Ok(())
                        }
                        .boxed_local()
                    },
                )
            })
            .method(
                ToolMethod::Validate.as_str(),
                |env, _, (_spec, _call): (ToolSpec, ToolCall)| async move {
                    env.value()?.0.set(true);
                    Ok(ToolDecision::Allow)
                },
            )
            .method(
                ToolMethod::Execute.as_str(),
                |env, _, (_operation, _call): (OperationId, ToolCall)| async move {
                    let value = env.value()?;
                    assert!(
                        value.0.get(),
                        "validation must belong to this physical instance"
                    );
                    value.1.replace(Some(ToolResult::Success(json!("read"))));
                    Err::<ToolResult, _>(tembra::Error::new(
                        Code::Unavailable,
                        "fixture.lost_reply",
                        "receipt was recorded",
                    ))
                },
            )
            .method(
                ToolMethod::Recover.as_str(),
                |env, _, (_operation, _call): (OperationId, ToolCall)| async move {
                    Ok(env
                        .value()?
                        .1
                        .borrow()
                        .clone()
                        .map(Recovery::Completed)
                        .unwrap_or(Recovery::Unknown))
                },
            ),
        ),
        orb(host.clone(), catalog),
        client(host.clone()),
    ])
    .unwrap();
    let reference = Rc::new(
        host.get("fixture.client", vec!["main".into()], "alice")
            .unwrap(),
    );
    let tool = definition("files.read", "fixture.session");
    let _: ToolPreparation = invoke(
        &mut pool,
        &host,
        &reference,
        CatalogMethod::Prepare.as_str(),
        (tool.spec, call("files.read")),
    )
    .unwrap();
    let operation = OperationId::new();
    let failed = invoke::<_, ToolResult>(
        &mut pool,
        &host,
        &reference,
        ToolMethod::Execute.as_str(),
        (operation, call("files.read"), tool.provider.clone()),
    );
    assert_eq!(failed.unwrap_err().code, Code::Unavailable);
    let recovered: Recovery<ToolResult> = invoke(
        &mut pool,
        &host,
        &reference,
        ToolMethod::Recover.as_str(),
        (operation, call("files.read"), tool.provider),
    )
    .unwrap();
    assert!(matches!(recovered, Recovery::Completed(_)));
    assert_eq!(opened.get(), 1);
    assert_eq!(closing.get(), 0);
    reference.release().unwrap();
    pool.run_until_stalled();
    assert_eq!(closing.get(), 1);
    send.send(()).unwrap();
    pool.run_until(host.close()).unwrap();
    assert_eq!(host.kernel().diagnostics().resources, 0);
}

#[test]
fn orb_owned_metadata_is_registered_once_and_routes_as_each_caller() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let catalog = Catalog::default();
    let registration = catalog.clone();
    host.install(vec![
        Orb::new("fixture.shared", 1)
            .create(move |d| {
                registration.register_owned(&d, definition("files.read", "fixture.shared"))
            })
            .service(
                Service::new("fixture.shared", |_| Ok(()))
                    .method(
                        ToolMethod::Validate.as_str(),
                        |_, _, (_spec, _call): (ToolSpec, ToolCall)| async {
                            Ok(ToolDecision::Allow)
                        },
                    )
                    .method(
                        ToolMethod::Execute.as_str(),
                        |_, ctx, (_operation, _call): (OperationId, ToolCall)| async move {
                            Ok(ToolResult::Success(json!(ctx.principal()?)))
                        },
                    ),
            ),
        orb(host.clone(), catalog.clone()),
    ])
    .unwrap();
    pool.run_until_stalled();
    assert_eq!(catalog.snapshot().unwrap().tools.len(), 1);
    for principal in ["alice", "bob"] {
        let reference = host
            .get(SERVICE, vec![INSTANCE_KEY.into()], principal)
            .unwrap();
        let result: ToolResult = pool
            .run_until(host.root(principal, move |ctx| {
                async move {
                    reference
                        .call(
                            &ctx,
                            CatalogMethod::Execute.as_str(),
                            (
                                OperationId::new(),
                                call("files.read"),
                                definition("files.read", "fixture.shared").provider,
                            ),
                        )
                        .await
                }
                .boxed_local()
            }))
            .unwrap();
        assert_eq!(result, ToolResult::Success(json!(principal)));
        assert_eq!(catalog.snapshot().unwrap().tools.len(), 1);
    }
    pool.run_until(host.close()).unwrap();
    assert!(catalog.snapshot().unwrap().tools.is_empty());
}

#[test]
fn predispatch_activation_failure_is_a_confirmed_blocked_result() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let tool = definition("files.read", "fixture.unavailable");
    let catalog = Catalog::new([tool.clone()]).unwrap();
    host.install(vec![
        orb(host.clone(), catalog),
        Orb::new("fixture.unavailable", 1).service(Service::new("fixture.unavailable", |_| {
            Err::<(), _>(tembra::Error::new(
                Code::Unavailable,
                "fixture.activation",
                "provider cannot open",
            ))
        })),
    ])
    .unwrap();
    let reference = Rc::new(
        host.get(SERVICE, vec![INSTANCE_KEY.into()], "alice")
            .unwrap(),
    );
    let result: ToolResult = invoke(
        &mut pool,
        &host,
        &reference,
        CatalogMethod::Execute.as_str(),
        (OperationId::new(), call("files.read"), tool.provider),
    )
    .unwrap();
    assert!(matches!(result, ToolResult::Blocked(_)));
    pool.run_until(host.close()).unwrap();
}

#[test]
fn new_user_generation_can_add_an_invisible_notice_without_changing_messages() {
    let catalog = Catalog::default();
    let files = catalog
        .register(definition("files.read", "fixture.files"))
        .unwrap();
    let _web = catalog
        .register(definition("web.fetch", "fixture.web"))
        .unwrap();
    let previous = catalog.snapshot().unwrap();
    let frozen = serde_json::to_value(&previous).unwrap();
    assert_eq!(previous.specs().len(), 2);
    assert_eq!(
        previous
            .select(&[ToolName::new("web.fetch").unwrap()])
            .unwrap()
            .tools
            .len(),
        1
    );
    drop(files);
    let context = AgentContext {
        instructions: "Help the user".into(),
        messages: vec![swarmie_providers::Message::User {
            content: "Read again".into(),
        }],
    };
    // Registry changes alone never mutate an already prepared request.
    assert_eq!(serde_json::to_value(&previous).unwrap(), frozen);
    let current = catalog.snapshot().unwrap();
    let prepared = current.prepare_context(&previous, context.clone());
    assert_eq!(prepared.messages, context.messages);
    assert!(prepared.instructions.contains("files.read"));
    assert!(!prepared.instructions.contains("web.fetch"));
    assert_eq!(current.prepare_context(&current, context.clone()), context);
}

#[test]
fn tembra_owner_cleanup_withdraws_even_if_a_resource_value_is_retained() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let catalog = Catalog::default();
    let scoped = catalog.clone();
    let retained = Rc::new(RefCell::new(None));
    let slot = retained.clone();
    host.install(vec![
        Orb::new("fixture.registration", 1).service(
            Service::new("fixture.registration", move |declaration| {
                scoped.register_owned(&declaration, definition("files.read", "fixture.files"))
            })
            .method("hold", move |resource, _, (): ()| {
                let slot = slot.clone();
                async move {
                    slot.replace(Some(resource.value()?));
                    Ok(())
                }
            }),
        ),
    ])
    .unwrap();
    let owner = Rc::new(
        host.get("fixture.registration", vec!["main".into()], "alice")
            .unwrap(),
    );
    invoke::<_, ()>(&mut pool, &host, &owner, "hold", ()).unwrap();
    assert_eq!(catalog.snapshot().unwrap().tools.len(), 1);
    owner.release().unwrap();
    pool.run_until_stalled();
    assert!(catalog.snapshot().unwrap().tools.is_empty());
    let _replacement = catalog
        .register(definition("files.read", "fixture.new-files"))
        .unwrap();
    retained.borrow_mut().take();
    assert_eq!(catalog.snapshot().unwrap().tools.len(), 1);
    pool.run_until(host.close()).unwrap();
    assert_eq!(host.kernel().diagnostics().resources, 0);
}

#[derive(Default)]
struct Provider {
    calls: Cell<usize>,
    recoveries: Cell<usize>,
    lose_reply: Cell<bool>,
    validation_gate: RefCell<Option<futures::channel::oneshot::Receiver<()>>>,
    receipts: RefCell<BTreeMap<OperationId, (String, ToolCall, ToolResult)>>,
}

fn provider(name: &str, state: Rc<Provider>) -> Orb {
    Orb::new(name, 1).service(
        Service::new(name, move |_| Ok(state.clone()))
            .method(
                ToolMethod::Validate.as_str(),
                |state, ctx, (_spec, _call): (ToolSpec, ToolCall)| async move {
                    let gate = state.validation_gate.borrow_mut().take();
                    if let Some(gate) = gate {
                        let _ = gate.await;
                    }
                    if ctx.principal()? != "alice" {
                        return Err(tembra::Error::new(
                            Code::Forbidden,
                            "fixture.owner",
                            "not authorized",
                        ));
                    }
                    Ok(ToolDecision::Allow)
                },
            )
            .method(
                ToolMethod::Execute.as_str(),
                |state, ctx, (operation, call): (OperationId, ToolCall)| async move {
                    if ctx.principal()? != "alice" {
                        return Err(tembra::Error::new(
                            Code::Forbidden,
                            "fixture.owner",
                            "not authorized",
                        ));
                    }
                    state.calls.set(state.calls.get() + 1);
                    let result = ToolResult::Success(json!("read"));
                    state
                        .receipts
                        .borrow_mut()
                        .insert(operation, (ctx.principal()?, call, result.clone()));
                    if state.lose_reply.replace(false) {
                        return Err(tembra::Error::new(
                            Code::Unavailable,
                            "fixture.lost-reply",
                            "lost reply",
                        ));
                    }
                    Ok(result)
                },
            )
            .method(
                ToolMethod::Recover.as_str(),
                |state, ctx, (operation, call): (OperationId, ToolCall)| async move {
                    state.recoveries.set(state.recoveries.get() + 1);
                    let receipt = state.receipts.borrow().get(&operation).cloned();
                    match receipt {
                        Some((principal, saved, result))
                            if principal == ctx.principal()? && saved == call =>
                        {
                            Ok(Recovery::Completed(result))
                        }
                        Some(_) => Err(tembra::Error::new(
                            Code::Forbidden,
                            "fixture.receipt",
                            "receipt mismatch",
                        )),
                        None => Ok(Recovery::Unknown),
                    }
                },
            ),
    )
}

fn client(host: Host) -> Orb {
    Orb::new("fixture.client", 1).service(Service::new("fixture.client", move |declaration| CatalogService::bind(&declaration, host.clone(), SERVICE, vec![INSTANCE_KEY.into()]))
        .method(CatalogMethod::List.as_str(), |client, ctx, (): ()| async move { client.list(&ctx).await })
        .method(CatalogMethod::Prepare.as_str(), |client, ctx, (spec, call): (ToolSpec, ToolCall)| async move { client.prepare(&ctx, &spec, &call).await })
        .method(ToolMethod::Execute.as_str(), |client, ctx, (operation, call, provider): (OperationId, ToolCall, ToolProvider)| async move { client.execute(&ctx, ToolExecution { operation, call: &call, provider: Some(&provider) }).await })
        .method(ToolMethod::Recover.as_str(), |client, ctx, (operation, call, provider): (OperationId, ToolCall, ToolProvider)| async move { client.recover(&ctx, ToolExecution { operation, call: &call, provider: Some(&provider) }).await }))
}

#[test]
fn tools_route_across_orbs_and_recover_the_original_provider_after_withdrawal() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let catalog = Catalog::default();
    let files = definition("files.read", "fixture.files");
    let lease = catalog.register(files.clone()).unwrap();
    let original = Rc::new(Provider::default());
    let replacement = Rc::new(Provider::default());
    original.lose_reply.set(true);
    host.install(vec![
        orb(host.clone(), catalog.clone()),
        provider("fixture.files", original.clone()),
        provider("fixture.replacement", replacement.clone()),
        client(host.clone()),
    ])
    .unwrap();
    let reference = Rc::new(
        host.get("fixture.client", vec!["main".into()], "alice")
            .unwrap(),
    );
    let selected: Selection = invoke(
        &mut pool,
        &host,
        &reference,
        CatalogMethod::List.as_str(),
        (),
    )
    .unwrap();
    assert_eq!(selected.tools, vec![files.clone()]);
    let prepared: ToolPreparation = invoke(
        &mut pool,
        &host,
        &reference,
        CatalogMethod::Prepare.as_str(),
        (files.spec.clone(), call("files.read")),
    )
    .unwrap();
    assert_eq!(prepared.decision, ToolDecision::Allow);
    assert_eq!(prepared.provider, Some(files.provider.clone()));
    let operation = OperationId::new();
    let failed: tembra::Result<ToolResult> = invoke(
        &mut pool,
        &host,
        &reference,
        ToolMethod::Execute.as_str(),
        (operation, call("files.read"), files.provider.clone()),
    );
    assert_eq!(failed.unwrap_err().code, Code::Unavailable);
    drop(lease);
    let _new = catalog
        .register(definition("files.read", "fixture.replacement"))
        .unwrap();
    let result: Recovery<ToolResult> = invoke(
        &mut pool,
        &host,
        &reference,
        ToolMethod::Recover.as_str(),
        (operation, call("files.read"), files.provider.clone()),
    )
    .unwrap();
    assert!(matches!(result, Recovery::Completed(_)));
    assert_eq!(original.calls.get(), 1);
    assert_eq!(original.recoveries.get(), 1);
    assert_eq!(replacement.calls.get(), 0);
    assert_eq!(replacement.recoveries.get(), 0);
    // A fresh dispatch cannot reuse the withdrawn route, even with saved metadata.
    let blocked: ToolResult = invoke(
        &mut pool,
        &host,
        &reference,
        ToolMethod::Execute.as_str(),
        (OperationId::new(), call("files.read"), files.provider),
    )
    .unwrap();
    assert!(matches!(blocked, ToolResult::Blocked(_)));
    pool.run_until(host.close()).unwrap();
    let d = host.kernel().diagnostics();
    assert_eq!((d.roots, d.calls, d.resources), (0, 0, 0));
}

#[test]
fn registry_metadata_never_grants_another_principal_provider_authority() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let catalog = Catalog::default();
    let files = definition("files.read", "fixture.files");
    let _lease = catalog.register(files.clone()).unwrap();
    let state = Rc::new(Provider::default());
    host.install(vec![
        orb(host.clone(), catalog),
        provider("fixture.files", state.clone()),
        client(host.clone()),
    ])
    .unwrap();
    let reference = host
        .get("fixture.client", vec!["main".into()], "bob")
        .unwrap();
    let result: tembra::Result<ToolPreparation> = pool.run_until(host.root("bob", move |ctx| {
        async move {
            reference
                .call(
                    &ctx,
                    CatalogMethod::Prepare.as_str(),
                    (files.spec, call("files.read")),
                )
                .await
        }
        .boxed_local()
    }));
    assert_eq!(result.unwrap_err().code, Code::Forbidden);
    assert_eq!(state.calls.get(), 0);
    pool.run_until(host.close()).unwrap();
}

#[test]
fn withdrawing_during_validation_blocks_admission_before_dispatch() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let catalog = Catalog::default();
    let files = definition("files.read", "fixture.files");
    let lease = catalog.register(files.clone()).unwrap();
    let state = Rc::new(Provider::default());
    let (release, gate) = futures::channel::oneshot::channel();
    state.validation_gate.replace(Some(gate));
    host.install(vec![
        orb(host.clone(), catalog),
        provider("fixture.files", state.clone()),
        client(host.clone()),
    ])
    .unwrap();
    let reference = host
        .get("fixture.client", vec!["main".into()], "alice")
        .unwrap();
    let mut waiting = Box::pin(host.root("alice", move |ctx| {
        async move {
            reference
                .call::<_, ToolPreparation>(
                    &ctx,
                    CatalogMethod::Prepare.as_str(),
                    (files.spec, call("files.read")),
                )
                .await
        }
        .boxed_local()
    }));
    pool.run_until(async {
        assert!(futures::poll!(&mut waiting).is_pending());
    });
    pool.run_until_stalled();
    drop(lease);
    release.send(()).unwrap();
    let prepared = pool.run_until(waiting).unwrap();
    assert!(matches!(prepared.decision, ToolDecision::Block(_)));
    assert_eq!(state.calls.get(), 0);
    pool.run_until(host.close()).unwrap();
}
