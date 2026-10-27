#[path = "support/request.rs"]
mod input;
#[path = "support/scenario.rs"]
mod scenario;

use futures::{FutureExt, executor::LocalPool};
use serde_json::json;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use swarmie_agent_loop::{hooks::*, *};
use swarmie_models::mock::{MockModel, Step};
use tembra::{Code, Host, Orb, Service};

#[path = "support/hook_scenario.rs"]
mod hook_scenario;
use hook_scenario::*;

fn managed(model: Rc<MockModel>, tools: Rc<scenario::Scenario>) -> Orb {
    orb(move |_| Ok(Agent::new(model.clone(), tools.clone())))
}

#[test]
fn all_seven_hooks_transform_before_confirmation_and_observe_in_order() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let model = Rc::new(MockModel::new([
        Step::Reply(response(true)),
        Step::Reply(response(false)),
    ]));
    let tools = Rc::new(scenario::Scenario::default());
    let hooks = Hooks::default();
    let seen = Rc::new(RefCell::new(vec![]));
    let a = seen.clone();
    let b = seen.clone();
    let c = seen.clone();
    let d = seen.clone();
    let e = seen.clone();
    let f = seen.clone();
    let g = seen.clone();
    let extensions = contributor(SERVICE)
        .contribute(SERVICE, &hooks.before_turn_agent, 0, move |ctx, mut v| {
            assert_eq!(ctx.principal().unwrap(), "alice");
            a.borrow_mut().push("agent");
            v.request.context.instructions.push_str(" initial");
            async { Ok(v) }
        })
        .contribute(SERVICE, &hooks.before_turn, 0, move |_, mut v| {
            b.borrow_mut().push("turn");
            v.request.context.instructions.push_str(" turn");
            async { Ok(v) }
        })
        .contribute(SERVICE, &hooks.prepare_context, 10, move |_, mut v| {
            c.borrow_mut().push("context");
            assert!(v.instructions.ends_with(" turn"));
            v.instructions.push_str(" projected");
            async { Ok(v) }
        })
        .contribute(SERVICE, &hooks.before_tool_call, 10, move |_, mut v| {
            d.borrow_mut().push("tool");
            v.call.arguments = json!({"amount":1});
            async { Ok(v) }
        })
        .contribute(SERVICE, &hooks.after_tool_call, 0, move |_, mut v| {
            e.borrow_mut().push("tool-result");
            assert_eq!(v.call.arguments, json!({"amount":1}));
            assert_eq!(v.result, ToolResult::Success(json!(1)));
            v.result = ToolResult::Success(json!(42));
            async { Ok(v) }
        })
        .contribute(SERVICE, &hooks.after_turn, 0, move |_, v| {
            f.borrow_mut().push("turn-end");
            assert!(v.turn > 0);
            async { Ok(()) }
        })
        .contribute(SERVICE, &hooks.after_turn_agent, 0, move |ctx, v| {
            g.borrow_mut().push("agent-end");
            assert_eq!(ctx.principal().unwrap(), "alice");
            assert_eq!(v.status, ExecutionStatus::Completed(FinishReason::Complete));
            async { Ok(()) }
        });
    host.install(vec![managed(model.clone(), tools.clone()), extensions])
        .unwrap();
    let reference = Rc::new(host.get(SERVICE, vec!["main".into()], "alice").unwrap());
    let output: ExecutionResult = invoke(
        &mut pool,
        &host,
        &reference,
        Method::Execute.as_str(),
        input::request(),
    );
    let output = output.unwrap();
    assert_eq!(
        &*seen.borrow(),
        &[
            "agent",
            "turn",
            "context",
            "tool",
            "tool-result",
            "turn-end",
            "turn",
            "context",
            "turn-end",
            "agent-end"
        ]
    );
    let calls = model.calls();
    assert!(
        calls[0]
            .request
            .context
            .instructions
            .ends_with(" initial turn projected")
    );
    assert!(
        calls[1]
            .request
            .context
            .instructions
            .ends_with(" turn projected")
    );
    assert!(!calls[1].request.context.instructions.contains("initial"));
    assert!(
        matches!(&calls[1].request.context.messages[2], Message::Tool { result: ToolResult::Success(v), .. } if v == &json!(42))
    );
    assert!(
        matches!(&output.messages[0], Message::Assistant(v) if v.tool_calls[0].arguments == json!({"amount":99}))
    );
    assert_eq!(tools.effects.borrow().len(), 1);
    close(&mut pool, &host);
}

#[test]
fn waterfall_retains_earlier_arguments_and_first_block_stops_later_contributors() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let model = Rc::new(MockModel::new([
        Step::Reply(response(true)),
        Step::Reply(response(false)),
    ]));
    let tools = Rc::new(scenario::Scenario::default());
    let hooks = Hooks::default();
    let extensions = contributor(SERVICE)
        .contribute(
            SERVICE,
            &hooks.before_tool_call,
            -10,
            |_, mut v| async move {
                v.call.arguments = json!({"amount":1});
                Ok(v)
            },
        )
        .contribute(SERVICE, &hooks.before_tool_call, 0, |_, mut v| async move {
            assert_eq!(v.call.arguments, json!({"amount":1}));
            v.decision = ToolDecision::Block("denied".into());
            Ok(v)
        })
        .contribute(SERVICE, &hooks.before_tool_call, 1, |_, _| async {
            panic!("blocked chain continued")
        });
    host.install(vec![managed(model, tools.clone()), extensions])
        .unwrap();
    let reference = Rc::new(host.get(SERVICE, vec!["main".into()], "alice").unwrap());
    let output: ExecutionResult = invoke(
        &mut pool,
        &host,
        &reference,
        Method::Execute.as_str(),
        input::request(),
    );
    assert!(
        matches!(&output.unwrap().messages[1], Message::Tool { result: ToolResult::Blocked(v), .. } if v == "denied")
    );
    assert!(tools.effects.borrow().is_empty());
    close(&mut pool, &host);
}

#[test]
fn pending_model_recovery_reuses_projection_without_rerunning_preparation() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let model = Rc::new(MockModel::new([Step::LoseReply(response(false))]));
    let hooks = Hooks::default();
    let count = Rc::new(Cell::new(0));
    let counter = count.clone();
    host.install(vec![
        managed(model.clone(), Rc::new(scenario::Scenario::default())),
        contributor(SERVICE).contribute(SERVICE, &hooks.prepare_context, 0, move |_, mut v| {
            counter.set(counter.get() + 1);
            v.instructions.push_str(" prepared");
            async { Ok(v) }
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
    assert!(
        matches!(state.phase(), Phase::ModelPending { request, .. } if request.context.instructions.ends_with(" prepared"))
    );
    let second: ExecutionResult =
        invoke(&mut pool, &host, &reference, Method::Resume.as_str(), state);
    second.unwrap();
    assert_eq!(count.get(), 1);
    assert_eq!(model.calls().len(), 1);
    assert_eq!(model.recoveries().len(), 1);
    close(&mut pool, &host);
}

#[test]
fn control_failure_and_tool_widening_fail_before_provider_dispatch() {
    for widen in [false, true] {
        let mut pool = LocalPool::new();
        let host = Host::new(pool.spawner());
        let model = Rc::new(MockModel::new([]));
        let hooks = Hooks::default();
        let outcomes = Rc::new(RefCell::new(vec![]));
        let terminal = outcomes.clone();
        host.install(vec![
            managed(model.clone(), Rc::new(scenario::Scenario::default())),
            contributor(SERVICE)
                .contribute(SERVICE, &hooks.before_turn, 0, move |_, mut v| async move {
                    if widen {
                        v.request.tools[0].description = "changed".into();
                        Ok(v)
                    } else {
                        Err(tembra::Error::new(
                            Code::Forbidden,
                            "fixture.hook",
                            "denied",
                        ))
                    }
                })
                .contribute(SERVICE, &hooks.after_turn_agent, 0, move |_, v| {
                    terminal.borrow_mut().push(v.status);
                    async { Ok(()) }
                }),
        ])
        .unwrap();
        let reference = Rc::new(host.get(SERVICE, vec!["main".into()], "alice").unwrap());
        let result: ExecutionResult = invoke(
            &mut pool,
            &host,
            &reference,
            Method::Execute.as_str(),
            input::request(),
        );
        let error = result.unwrap_err();
        assert_eq!(
            error.error.code,
            if widen {
                Code::Invalid
            } else {
                Code::Forbidden
            }
        );
        assert!(matches!(error.state.unwrap().phase(), Phase::PrepareTurn));
        assert_eq!(outcomes.borrow().len(), 1);
        assert!(model.calls().is_empty());
        close(&mut pool, &host);
    }
}

#[test]
fn observation_errors_do_not_replace_confirmed_output_and_remain_inspectable() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let hooks = Hooks::default();
    let model = Rc::new(MockModel::new([Step::Reply(response(false))]));
    let slot = Rc::new(RefCell::new(None));
    let captured = slot.clone();
    let tools = Rc::new(scenario::Scenario::default());
    let native = Hooks::default().declare(Orb::new(SERVICE, 1)).service(
        Service::new(SERVICE, move |declaration| {
            let agent = Agent::new(model.clone(), tools.clone()).bind_hooks(&declaration)?;
            captured.replace(Some(agent.clone()));
            Ok(agent)
        })
        .method(
            Method::Execute.as_str(),
            |agent, ctx, request: Request| async move { Ok(agent.execute(&ctx, request).await) },
        ),
    );
    host.install(vec![
        native,
        contributor(SERVICE)
            .contribute(SERVICE, &hooks.after_turn, 0, |_, _| async {
                Err(tembra::Error::invalid("fixture.turn-observer", "failed"))
            })
            .contribute(SERVICE, &hooks.after_turn_agent, 0, |_, v| async move {
                assert!(matches!(v.status, ExecutionStatus::Completed(_)));
                Err(tembra::Error::invalid("fixture.final-observer", "failed"))
            }),
    ])
    .unwrap();
    let reference = Rc::new(host.get(SERVICE, vec!["main".into()], "alice").unwrap());
    let output: ExecutionResult = invoke(
        &mut pool,
        &host,
        &reference,
        Method::Execute.as_str(),
        input::request(),
    );
    assert_eq!(output.unwrap().reason, FinishReason::Complete);
    let failure = slot
        .borrow()
        .as_ref()
        .unwrap()
        .observation_failure()
        .unwrap();
    assert_eq!(failure.hook, HookName::AfterTurnAgent);
    assert_eq!(failure.cause.reason, "fixture.final-observer");
    close(&mut pool, &host);
}

#[test]
fn cancelled_attempt_observes_with_original_identity_and_survives_temporary_dependency_release() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let hooks = Hooks::default();
    let seen = Rc::new(Cell::new(0));
    let first = seen.clone();
    let second = seen.clone();
    let model = Rc::new(MockModel::new([Step::CancelReply(response(false))]));
    let source_model = model.clone();
    let source = Hooks::default().declare(Orb::new(SERVICE, 1)).service(
        Service::new(SERVICE, move |declaration| {
            Agent::new(source_model.clone(), Rc::new(scenario::Scenario::default()))
                .bind_hooks(&declaration)
        })
        .method(
            Method::Execute.as_str(),
            |agent, ctx, request: Request| async move {
                let result = agent.execute(&ctx, request).await;
                assert_eq!(ctx.check().unwrap_err().code, Code::Cancelled);
                Ok(result)
            },
        ),
    );
    host.install(vec![
        source,
        Orb::new("fixture.probe", 1).service(
            Service::new("fixture.probe", |_| Ok(()))
                .method("read", |_, ctx, (): ()| async move { ctx.check() }),
        ),
        contributor(SERVICE)
            .contribute(SERVICE, &hooks.after_turn_agent, 0, move |ctx, v| {
                let first = first.clone();
                async move {
                    ctx.check()?;
                    assert_eq!(ctx.principal()?, "alice");
                    assert_eq!(ctx.frame(), 0);
                    assert!(matches!(
                        v.status,
                        ExecutionStatus::Interrupted(Cause {
                            code: Code::Cancelled,
                            ..
                        })
                    ));
                    let reference = ctx.get(
                        "fixture.probe",
                        vec!["temporary".into()],
                        Default::default(),
                    )?;
                    reference.call::<_, ()>(&ctx, "read", ()).await?;
                    reference.release()?;
                    ctx.check()?;
                    first.set(first.get() + 1);
                    Ok(())
                }
            })
            .contribute(SERVICE, &hooks.after_turn_agent, 1, move |ctx, _| {
                ctx.check().unwrap();
                second.set(second.get() + 1);
                async { Ok(()) }
            }),
    ])
    .unwrap();
    let reference = Rc::new(host.get(SERVICE, vec!["main".into()], "alice").unwrap());
    // A cancelled logical waiter may return before the physically owned observation.
    let reference2 = reference.clone();
    let result = pool.run_until(host.root("alice", move |ctx| {
        async move {
            reference2
                .call::<_, ExecutionResult>(&ctx, Method::Execute.as_str(), input::request())
                .await
        }
        .boxed_local()
    }));
    let error = match result {
        Ok(result) => result.unwrap_err().error,
        Err(error) => error,
    };
    assert_eq!(error.code, Code::Cancelled);
    pool.run_until_stalled();
    assert_eq!(seen.get(), 2);
    assert_eq!(model.calls().len(), 1);
    close(&mut pool, &host);
}

#[test]
fn result_hook_failure_preserves_pending_receipt_and_transformed_arguments_on_resume() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let hooks = Hooks::default();
    let tools = Rc::new(scenario::Scenario::default());
    let model = Rc::new(MockModel::new([
        Step::Reply(response(true)),
        Step::Reply(response(false)),
    ]));
    let count = Rc::new(Cell::new(0));
    let prepared = count.clone();
    let fail = Rc::new(Cell::new(true));
    let once = fail.clone();
    host.install(vec![
        managed(model.clone(), tools.clone()),
        contributor(SERVICE)
            .contribute(SERVICE, &hooks.before_tool_call, 0, move |_, mut v| {
                prepared.set(prepared.get() + 1);
                v.call.arguments = json!({"amount":1});
                async { Ok(v) }
            })
            .contribute(SERVICE, &hooks.after_tool_call, 0, move |_, v| {
                let reject = once.replace(false);
                async move {
                    if reject {
                        Err(tembra::Error::invalid("fixture.result-transform", "failed"))
                    } else {
                        Ok(v)
                    }
                }
            }),
    ])
    .unwrap();
    let reference = Rc::new(host.get(SERVICE, vec!["main".into()], "alice").unwrap());
    let result: ExecutionResult = invoke(
        &mut pool,
        &host,
        &reference,
        Method::Execute.as_str(),
        input::request(),
    );
    let state = *result.unwrap_err().state.unwrap();
    assert!(
        matches!(state.phase(),Phase::ToolPending { call, .. } if call.arguments==json!({"amount":1}))
    );
    let mut forged = serde_json::to_value(&state).unwrap();
    forged["phase"]["call"]["id"] = json!("forged-call");
    let forged: State = serde_json::from_value(forged).unwrap();
    let rejected: ExecutionResult = invoke(
        &mut pool,
        &host,
        &reference,
        Method::Resume.as_str(),
        forged,
    );
    assert_eq!(rejected.unwrap_err().error.code, Code::Invalid);
    assert!(tools.reconciled_tools.borrow().is_empty());
    let output: ExecutionResult =
        invoke(&mut pool, &host, &reference, Method::Resume.as_str(), state);
    output.unwrap();
    assert_eq!(count.get(), 1);
    assert_eq!(tools.effects.borrow().len(), 1);
    assert_eq!(tools.reconciled_tools.borrow().len(), 1);
    assert_eq!(
        *tools.tool_inputs.borrow(),
        *tools.recovered_inputs.borrow()
    );
    close(&mut pool, &host);
}

#[test]
fn dropping_the_logical_waiter_keeps_the_physical_tool_and_terminal_observation_owned() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let hooks = Hooks::default();
    let model = Rc::new(MockModel::new([
        Step::Reply(ModelResponse {
            message: AssistantMessage {
                content: String::new(),
                tool_calls: vec![ToolCall {
                    id: ToolCallId::new("call-1").unwrap(),
                    name: ToolName::new("counter.add").unwrap(),
                    arguments: json!({"amount":1}),
                }],
            },
            usage: Usage::default(),
        }),
        Step::Reply(response(false)),
    ]));
    let tools = Rc::new(scenario::Scenario::default());
    let (release, gate) = futures::channel::oneshot::channel();
    tools.tool_gate.replace(Some(gate));
    let observed = Rc::new(Cell::new(false));
    let terminal = observed.clone();
    host.install(vec![
        managed(model.clone(), tools.clone()),
        contributor(SERVICE).contribute(SERVICE, &hooks.after_turn_agent, 0, move |ctx, v| {
            ctx.check().unwrap();
            assert!(matches!(
                v.status,
                ExecutionStatus::Interrupted(Cause { code:Code::Expired, ref reason, .. }) if reason == "root"
            ), "unexpected outcome: {v:?}");
            assert_ne!(ctx.invocation().unwrap().root, v.source.root);
            terminal.set(true);
            async { Ok(()) }
        }),
    ])
    .unwrap();
    let reference = Rc::new(host.get(SERVICE, vec!["main".into()], "alice").unwrap());
    let mut waiting = Box::pin(host.root("alice", move |ctx| {
        async move {
            reference
                .call::<_, ExecutionResult>(&ctx, Method::Execute.as_str(), input::request())
                .await
        }
        .boxed_local()
    }));
    pool.run_until(async {
        assert!(futures::poll!(&mut waiting).is_pending());
    });
    pool.run_until_stalled();
    assert!(tools.tool_entered.get());
    drop(waiting);
    pool.run_until_stalled();
    assert!(!observed.get());
    assert!(host.kernel().diagnostics().calls > 0);
    release.send(()).unwrap();
    pool.run_until_stalled();
    assert!(observed.get());
    assert_eq!(tools.effects.borrow().len(), 1);
    assert_eq!(model.calls().len(), 1);
    assert_eq!(model.remaining(), 1);
    close(&mut pool, &host);
}

#[test]
fn request_tool_narrowing_survives_pending_model_recovery() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let mut first = response(true);
    first.message.tool_calls[0].arguments = json!({"amount":1});
    let mut second = first.clone();
    second.message.tool_calls[0].id = ToolCallId::new("call-2").unwrap();
    let model = Rc::new(MockModel::new([
        Step::Reply(first),
        Step::LoseReply(second),
        Step::Reply(response(false)),
    ]));
    let tools = Rc::new(scenario::Scenario::default());
    let preparations = Rc::new(Cell::new(0));
    let count = preparations.clone();
    let hooks = Hooks::default();
    host.install(vec![
        managed(model.clone(), tools.clone()),
        contributor(SERVICE).contribute(SERVICE, &hooks.before_turn, 0, move |_, mut v| {
            count.set(count.get() + 1);
            if v.turn == 2 {
                v.request.tools.clear();
            }
            async { Ok(v) }
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
    assert!(saved["available_tools"].as_array().unwrap().is_empty());
    let state: State = serde_json::from_value(saved).unwrap();
    let output: ExecutionResult =
        invoke(&mut pool, &host, &reference, Method::Resume.as_str(), state);
    let output = output.unwrap();
    assert!(matches!(
        &output.messages[3],
        Message::Tool {
            result: ToolResult::Blocked(_),
            ..
        }
    ));
    assert_eq!(tools.tool_inputs.borrow().len(), 1);
    assert_eq!(tools.effects.borrow().len(), 1);
    assert_eq!(preparations.get(), 3);
    assert!(model.calls()[1].request.tools.is_empty());
    assert_eq!(model.recoveries().len(), 1);
    close(&mut pool, &host);
}

#[test]
fn safe_retry_uses_prepared_arguments_without_repeating_tool_hooks() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let hooks = Hooks::default();
    let tools = Rc::new(scenario::Scenario::default());
    tools.not_started.set(true);
    let model = Rc::new(MockModel::new([
        Step::Reply(response(true)),
        Step::Reply(response(false)),
    ]));
    let preparations = Rc::new(Cell::new(0));
    let count = preparations.clone();
    host.install(vec![
        managed(model, tools.clone()),
        contributor(SERVICE).contribute(SERVICE, &hooks.before_tool_call, 0, move |_, mut v| {
            count.set(count.get() + 1);
            v.call.arguments = json!({"amount":1});
            async { Ok(v) }
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
    assert!(tools.effects.borrow().is_empty());
    tools.safe_retry.set(true);
    let result: ExecutionResult =
        invoke(&mut pool, &host, &reference, Method::Resume.as_str(), state);
    result.unwrap();
    assert_eq!(preparations.get(), 1);
    let dispatched = tools.tool_inputs.borrow();
    assert_eq!(dispatched.len(), 2);
    assert_eq!(dispatched[0].arguments, json!({"amount":1}));
    assert_eq!(dispatched[0], dispatched[1]);
    assert_eq!(&*tools.recovered_inputs.borrow(), &dispatched[..1]);
    assert_eq!(tools.effects.borrow().len(), 1);
    close(&mut pool, &host);
}

#[test]
fn cancellation_in_after_turn_preserves_the_confirmed_final_result() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let hooks = Hooks::default();
    let model = Rc::new(MockModel::new([Step::Reply(response(false))]));
    let completed = Rc::new(Cell::new(false));
    let terminal = Rc::new(Cell::new(false));
    let result_slot = completed.clone();
    let observed = terminal.clone();
    let source = Hooks::default().declare(Orb::new(SERVICE, 1)).service(
        Service::new(SERVICE, move |declaration| {
            Agent::new(model.clone(), Rc::new(scenario::Scenario::default()))
                .bind_hooks(&declaration)
        })
        .method(
            Method::Execute.as_str(),
            move |agent, ctx, request: Request| {
                let result_slot = result_slot.clone();
                async move {
                    let result = agent.execute(&ctx, request).await;
                    assert_eq!(ctx.check().unwrap_err().code, Code::Cancelled);
                    result_slot.set(
                        result
                            .as_ref()
                            .is_ok_and(|v| v.reason == FinishReason::Complete),
                    );
                    Ok(result)
                }
            },
        ),
    );
    host.install(vec![
        source,
        contributor(SERVICE)
            .contribute(SERVICE, &hooks.after_turn, 0, |ctx, _| async move {
                ctx.cancel()
            })
            .contribute(SERVICE, &hooks.after_turn_agent, 0, move |ctx, v| {
                ctx.check().unwrap();
                assert_eq!(v.status, ExecutionStatus::Completed(FinishReason::Complete));
                observed.set(true);
                async { Ok(()) }
            }),
    ])
    .unwrap();
    let reference = host.get(SERVICE, vec!["main".into()], "alice").unwrap();
    // Logical cancellation may hide the inner result from the waiting caller.
    let _ = pool.run_until(host.root("alice", move |ctx| {
        async move {
            reference
                .call::<_, ExecutionResult>(&ctx, Method::Execute.as_str(), input::request())
                .await
        }
        .boxed_local()
    }));
    pool.run_until_stalled();
    assert!(completed.get());
    assert!(terminal.get());
    close(&mut pool, &host);
}

#[test]
fn hooks_cannot_change_tool_identity_or_result_turn() {
    #[derive(Clone, Copy)]
    enum Mutation {
        BeforeId,
        BeforeName,
        AfterCall,
        AfterTurn,
    }

    for mutation in [
        Mutation::BeforeId,
        Mutation::BeforeName,
        Mutation::AfterCall,
        Mutation::AfterTurn,
    ] {
        let mut pool = LocalPool::new();
        let host = Host::new(pool.spawner());
        let model = Rc::new(MockModel::new([Step::Reply(response(true))]));
        let tools = Rc::new(scenario::Scenario::default());
        let hooks = Hooks::default();
        host.install(vec![
            managed(model, tools.clone()),
            contributor(SERVICE)
                .contribute(
                    SERVICE,
                    &hooks.before_tool_call,
                    0,
                    move |_, mut v| async move {
                        v.call.arguments = json!({"amount":1});
                        match mutation {
                            Mutation::BeforeId => v.call.id = ToolCallId::new("renamed").unwrap(),
                            Mutation::BeforeName => {
                                v.call.name = ToolName::new("other.tool").unwrap()
                            }
                            _ => {}
                        }
                        Ok(v)
                    },
                )
                .contribute(
                    SERVICE,
                    &hooks.after_tool_call,
                    0,
                    move |_, mut v| async move {
                        match mutation {
                            Mutation::AfterCall => v.call.arguments = json!({"amount":2}),
                            Mutation::AfterTurn => v.turn += 1,
                            _ => {}
                        }
                        Ok(v)
                    },
                ),
        ])
        .unwrap();
        let reference = Rc::new(host.get(SERVICE, vec!["main".into()], "alice").unwrap());
        let output: ExecutionResult = invoke(
            &mut pool,
            &host,
            &reference,
            Method::Execute.as_str(),
            input::request(),
        );
        let failure = output.unwrap_err();
        assert_eq!(failure.error.code, Code::Invalid);
        match mutation {
            Mutation::BeforeId | Mutation::BeforeName => {
                assert!(tools.tool_inputs.borrow().is_empty());
                assert!(matches!(
                    failure.state.unwrap().phase(),
                    Phase::PrepareTools { .. }
                ));
            }
            Mutation::AfterCall | Mutation::AfterTurn => {
                assert_eq!(tools.effects.borrow().len(), 1);
                assert!(matches!(
                    failure.state.unwrap().phase(),
                    Phase::ToolPending { .. }
                ));
            }
        }
        close(&mut pool, &host);
    }
}
