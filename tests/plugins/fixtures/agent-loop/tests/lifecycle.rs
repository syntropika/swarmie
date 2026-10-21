//! Native hook strategies, JavaScript reload and managed middleware recovery.

use futures::FutureExt;
use serde_json::json;
use std::{cell::RefCell, path::Path, rc::Rc, time::Duration};
use swarmie_agent_contracts::*;
use swarmie_agent_loop_fixture::{hooks, model::Reply, tool::ToolConfiguration};
use swarmie_model_contracts::{Message as ModelMessage, Role};
use swarmie_task_contracts::{TaskOutcome, TaskStatus};
use tembra::{RuntimeError, core};
use tembra_hooks::{BailDecision, HookArgs};
pub mod support;
use support::*;

#[test]
fn waterfall_context_options_and_tool_arguments_use_priority_order() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::double(1), Reply::text("done")],
            Default::default(),
            true,
        )
        .await?;
        let order = Rc::new(RefCell::new(Vec::new()));
        for (key, priority, value) in [("late", 5, "late"), ("early", -5, "early")] {
            let order = order.clone();
            fixture
                .host
                .install(
                    hooks::contribution(
                        prepare_context()?,
                        key,
                        priority,
                        move |args: HookArgs<PreparedContext, PreparedContext>, _| {
                            let mut input = args.into_parts().0;
                            order.borrow_mut().push(value);
                            input
                                .messages
                                .push(ModelMessage::text(Role::Developer, value));
                            async move { Ok(input) }.boxed_local()
                        },
                    )?,
                    core::LoadOptions::default(),
                )?
                .await?;
        }
        fixture
            .host
            .install(
                hooks::contribution(
                    before_model_call()?,
                    "options",
                    0,
                    |args: HookArgs<ModelCall, ModelCall>, _| {
                        let mut input = args.into_parts().0;
                        input.request.options.temperature = Some(0.25);
                        async move { Ok(input) }.boxed_local()
                    },
                )?,
                core::LoadOptions::default(),
            )?
            .await?;
        fixture
            .host
            .install(
                hooks::contribution(
                    before_tool_call()?,
                    "arguments",
                    0,
                    |args: HookArgs<ToolCall, ToolCall>, _| {
                        let mut input = args.into_parts().0;
                        input.arguments = json!({"value":8});
                        async move { Ok(input) }.boxed_local()
                    },
                )?,
                core::LoadOptions::default(),
            )?
            .await?;
        fixture.start("transforms", Default::default()).await?;
        let record = fixture.terminal("transforms").await?;
        assert!(
            matches!(record.task.outcome, Some(TaskOutcome::Completed { .. })),
            "{record:?}"
        );
        assert_eq!(&*order.borrow(), &["early", "late", "early", "late"]);
        assert!(
            fixture
                .model
                .requests
                .borrow()
                .iter()
                .all(|request| request.options.temperature == Some(0.25))
        );
        assert_eq!(fixture.tool.calls.borrow()[0].arguments, json!({"value":8}));
        assert_eq!(
            fixture.policy.calls.borrow()[0].arguments,
            json!({"value":8})
        );
        fixture.host.close().await
    });
}

#[test]
fn optional_veto_and_required_policy_both_prevent_tool_effects() {
    for optional in [false, true] {
        run(|driver| async move {
            let directory = tempfile::tempdir().unwrap();
            let fixture = open(
                driver,
                directory.path(),
                vec![Reply::double(21), Reply::text("denied")],
                Default::default(),
                optional,
            )
            .await?;
            if optional {
                fixture
                    .host
                    .install(
                        hooks::contribution(
                            authorize_tool_call()?,
                            "deny",
                            0,
                            |_: HookArgs<ToolCall, BailDecision<ToolDenial>>, _| {
                                async {
                                    Ok(BailDecision::Stop(ToolDenial {
                                        reason: "fixture optional veto".into(),
                                    }))
                                }
                                .boxed_local()
                            },
                        )?,
                        core::LoadOptions::default(),
                    )?
                    .await?;
            }
            fixture.start("denied", Default::default()).await?;
            let record = fixture.terminal("denied").await?;
            assert!(record.task.is_terminal());
            assert!(fixture.tool.calls.borrow().is_empty());
            fixture.host.close().await
        });
    }
}

#[test]
fn model_and_hook_failures_never_invoke_tools() {
    for hook_failure in [false, true] {
        run(|driver| async move {
            let directory = tempfile::tempdir().unwrap();
            let fixture = open(
                driver,
                directory.path(),
                vec![Reply::Error],
                Default::default(),
                true,
            )
            .await?;
            if hook_failure {
                fixture
                    .host
                    .install(
                        hooks::contribution(
                            prepare_context()?,
                            "failure",
                            0,
                            |_: HookArgs<PreparedContext, PreparedContext>, _| {
                                async {
                                    Err(RuntimeError::invalid(
                                        "fixture_hook_failure",
                                        "expected hook failure",
                                    ))
                                }
                                .boxed_local()
                            },
                        )?,
                        core::LoadOptions::default(),
                    )?
                    .await?;
            }
            fixture.start("error", Default::default()).await?;
            let record = fixture.terminal("error").await?;
            assert!(!matches!(
                record.task.outcome,
                Some(TaskOutcome::Completed { .. })
            ));
            assert!(fixture.tool.calls.borrow().is_empty());
            fixture.host.close().await
        });
    }
}

#[test]
fn javascript_hook_hot_reload_changes_later_turns_without_replacing_the_loop() {
    use tembra_executor_js::{BundledJsArtifact, Executor, ExecutorOptions};
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver.clone(),
            directory.path(),
            vec![Reply::text("first"), Reply::text("second")],
            Default::default(),
            true,
        )
        .await?;
        let executor = Executor::new(driver, ExecutorOptions::default())?;
        swarmie_agent_loop_default::register_agent_hooks(&executor)?;
        let artifact = |version: &str| {
            let path = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../../target/fixtures/agent-context")
                .join(version)
                .join("artifact.json");
            let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
                panic!(
                    "build hook artifacts before acceptance: {}: {error}",
                    path.display()
                )
            });
            BundledJsArtifact::from_json(&text)
        };
        let first = executor
            .prepare(
                fixture.host.core().id(),
                artifact("1.0.0")?,
                "default".into(),
            )
            .await?;
        fixture
            .host
            .core()
            .install_executable(first, core::LoadOptions::default())?
            .await?;
        fixture
            .start("javascript-first", Default::default())
            .await?;
        let first = fixture.terminal("javascript-first").await?;
        assert!(
            matches!(first.task.outcome, Some(TaskOutcome::Completed { .. })),
            "{first:?}"
        );
        assert!(
            serde_json::to_string(&fixture.model.requests.borrow()[0])
                .unwrap()
                .contains("dynamic-context-v1")
        );
        let second = executor
            .prepare(
                fixture.host.core().id(),
                artifact("1.1.0")?,
                "default".into(),
            )
            .await?;
        let report = fixture
            .host
            .core()
            .reload(vec![second], core::ReloadOptions::default())
            .await?;
        assert!(
            report
                .changed()
                .iter()
                .any(|id| id == "fixture.agent-context")
        );
        fixture
            .start("javascript-second", Default::default())
            .await?;
        let second = fixture.terminal("javascript-second").await?;
        assert!(
            matches!(second.task.outcome, Some(TaskOutcome::Completed { .. })),
            "{second:?}"
        );
        let request = serde_json::to_string(&fixture.model.requests.borrow()[1]).unwrap();
        assert!(request.contains("dynamic-context-v2"));
        assert!(!request.contains("dynamic-context-v1"));
        fixture.host.close().await
    });
}

#[test]
fn invalid_transformed_arguments_are_refused_before_policy_or_effect() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::double(1), Reply::text("invalid arguments")],
            Default::default(),
            true,
        )
        .await?;
        fixture
            .host
            .install(
                hooks::contribution(
                    before_tool_call()?,
                    "invalid-arguments",
                    0,
                    |args: HookArgs<ToolCall, ToolCall>, _| {
                        let mut call = args.into_parts().0;
                        call.arguments = json!({"value":"not an integer"});
                        async move { Ok(call) }.boxed_local()
                    },
                )?,
                core::LoadOptions::default(),
            )?
            .await?;
        fixture
            .start("invalid-arguments", Default::default())
            .await?;
        fixture.terminal("invalid-arguments").await?;
        assert!(fixture.tool.calls.borrow().is_empty());
        assert!(fixture.policy.calls.borrow().is_empty());
        fixture.host.close().await
    });
}

#[test]
fn effective_tool_terminal_denies_even_when_required_policy_allows() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::double(1), Reply::text("terminal refused")],
            ToolConfiguration {
                deny: true,
                ..Default::default()
            },
            true,
        )
        .await?;
        fixture.start("terminal-denied", Default::default()).await?;
        fixture.terminal("terminal-denied").await?;
        assert!(!fixture.policy.calls.borrow().is_empty());
        assert!(fixture.tool.calls.borrow().is_empty());
        fixture.host.close().await
    });
}

#[test]
fn hook_cannot_rewrite_turn_identity_to_cross_session_boundaries() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::text("unused")],
            Default::default(),
            true,
        )
        .await?;
        fixture
            .host
            .install(
                hooks::contribution(
                    prepare_context()?,
                    "rewrite-identity",
                    0,
                    |args: HookArgs<PreparedContext, PreparedContext>, _| {
                        let mut context = args.into_parts().0;
                        context.turn.session_id = "another-session".into();
                        async move { Ok(context) }.boxed_local()
                    },
                )?,
                core::LoadOptions::default(),
            )?
            .await?;
        fixture.start("identity", Default::default()).await?;
        let record = fixture.terminal("identity").await?;
        assert!(!matches!(
            record.task.outcome,
            Some(TaskOutcome::Completed { .. })
        ));
        assert!(fixture.model.requests.borrow().is_empty());
        fixture.host.close().await
    });
}

#[test]
fn middleware_can_transform_next_result_and_retry_after_confirmed_provider_result() {
    use swarmie_model_contracts::GenerateResult;
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::text("provider answer")],
            Default::default(),
            true,
        )
        .await?;
        let fail_once = Rc::new(std::cell::Cell::new(true));
        let failing = fail_once.clone();
        fixture
            .host
            .install(
                hooks::contribution(
                    wrap_model_call()?,
                    "wrap-model",
                    0,
                    move |args: HookArgs<ModelCall, GenerateResult>, _| {
                        let failing = failing.clone();
                        async move {
                            let (mut call, next) = args.into_parts();
                            call.request.options.temperature = Some(0.5);
                            let mut result = next
                                .expect("middleware has the managed terminal")
                                .call(call)
                                .await?;
                            if failing.replace(false) {
                                return Err(RuntimeError::invalid(
                                    "fixture_middleware_failed",
                                    "fail after the provider result is durably confirmed",
                                ));
                            }
                            if let GenerateResult::Response { response } = &mut result {
                                response.message =
                                    ModelMessage::text(Role::Assistant, "middleware answer");
                            }
                            Ok(result)
                        }
                        .boxed_local()
                    },
                )?,
                core::LoadOptions::default(),
            )?
            .await?;
        fixture.start("middleware", Default::default()).await?;
        loop {
            let state = fixture
                .agent
                .inspect(TurnLookup {
                    turn_id: "middleware".into(),
                })
                .await?
                .unwrap();
            if state.task.status == TaskStatus::Parked {
                break;
            }
            assert!(!state.task.is_terminal());
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert_eq!(fixture.model.requests.borrow().len(), 1);
        assert_eq!(
            fixture.model.requests.borrow()[0].options.temperature,
            Some(0.5)
        );
        fixture
            .agent
            .continue_turn(ContinueTurn {
                turn_id: "middleware".into(),
                resolution: None,
            })
            .await?;
        let completed = fixture.terminal("middleware").await?;
        assert!(
            matches!(completed.task.outcome, Some(TaskOutcome::Completed { .. })),
            "{completed:?}"
        );
        assert_eq!(
            fixture.model.requests.borrow().len(),
            1,
            "provider receipt must prevent regeneration after middleware failure"
        );
        let events = fixture.events("middleware").await?;
        let provider = events
            .iter()
            .find(|entry| entry.event.kind == "swarmie.agent.model-provider-confirmed")
            .unwrap();
        let effective = events
            .iter()
            .find(|entry| entry.event.kind == "swarmie.agent.model-confirmed")
            .unwrap();
        assert!(
            provider
                .event
                .payload
                .to_string()
                .contains("provider answer")
        );
        assert!(
            effective
                .event
                .payload
                .to_string()
                .contains("middleware answer")
        );
        fixture.host.close().await
    });
}

#[test]
fn complete_observation_lifecycle_orders_tool_steps_and_notifies_turn_end_once() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::double(21), Reply::text("42")],
            Default::default(),
            true,
        )
        .await?;
        let order = Rc::new(RefCell::new(Vec::<String>::new()));
        let observed = order.clone();
        fixture
            .host
            .install(
                hooks::contribution(
                    on_turn_start()?,
                    "observe-turn-start",
                    0,
                    move |args: HookArgs<TurnContext, ()>, _| {
                        observed
                            .borrow_mut()
                            .push(format!("turn-start:{}", args.input().step));
                        async { Ok(()) }.boxed_local()
                    },
                )?,
                core::LoadOptions::default(),
            )?
            .await?;
        let observed = order.clone();
        fixture
            .host
            .install(
                hooks::contribution(
                    after_model_call()?,
                    "observe-model",
                    0,
                    move |args: HookArgs<ModelObservation, ()>, _| {
                        observed
                            .borrow_mut()
                            .push(format!("after-model:{}", args.input().call.turn.step));
                        async { Ok(()) }.boxed_local()
                    },
                )?,
                core::LoadOptions::default(),
            )?
            .await?;
        let observed = order.clone();
        fixture
            .host
            .install(
                hooks::contribution(
                    after_tool_call()?,
                    "observe-tool",
                    0,
                    move |args: HookArgs<ToolObservation, ()>, _| {
                        observed
                            .borrow_mut()
                            .push(format!("after-tool:{}", args.input().call.turn.step));
                        async { Ok(()) }.boxed_local()
                    },
                )?,
                core::LoadOptions::default(),
            )?
            .await?;
        let observed = order.clone();
        fixture
            .host
            .install(
                hooks::contribution(
                    on_step_end()?,
                    "observe-step-end",
                    0,
                    move |args: HookArgs<StepEnd, ()>, _| {
                        observed
                            .borrow_mut()
                            .push(format!("step-end:{}", args.input().turn.step));
                        async { Ok(()) }.boxed_local()
                    },
                )?,
                core::LoadOptions::default(),
            )?
            .await?;
        let observed = order.clone();
        fixture
            .host
            .install(
                hooks::contribution(
                    on_turn_end()?,
                    "observe-turn-end",
                    0,
                    move |args: HookArgs<TurnEnd, ()>, _| {
                        assert!(matches!(
                            args.input().outcome,
                            TaskOutcome::Completed { .. }
                        ));
                        observed
                            .borrow_mut()
                            .push(format!("turn-end:{}", args.input().turn.step));
                        async { Ok(()) }.boxed_local()
                    },
                )?,
                core::LoadOptions::default(),
            )?
            .await?;
        fixture
            .start("observation-order", Default::default())
            .await?;
        let completed = fixture.terminal("observation-order").await?;
        assert!(
            matches!(completed.task.outcome, Some(TaskOutcome::Completed { .. })),
            "{completed:?}"
        );
        // Repeated terminal reconciliation must not replay the normal completion observer.
        fixture
            .agent
            .inspect(TurnLookup {
                turn_id: "observation-order".into(),
            })
            .await?;
        assert_eq!(
            &*order.borrow(),
            &[
                "turn-start:0",
                "after-model:0",
                "after-tool:0",
                "step-end:0",
                "after-model:1",
                "step-end:1",
                "turn-end:1"
            ]
        );
        fixture.host.close().await
    });
}
