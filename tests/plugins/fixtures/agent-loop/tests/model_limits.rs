//! Effective requests and middleware outcomes remain subject to admitted turn limits.

use std::time::Duration;

use futures::FutureExt;
use swarmie_agent_contracts::*;
use swarmie_agent_loop_fixture::{hooks, model::Reply};
use swarmie_model_contracts::{ContentPart, FinishReason, GenerateResult, ToolCall};
use swarmie_task_contracts::{TaskOutcome, TaskStatus};
use tembra::core;
use tembra_hooks::HookArgs;

pub mod support;
use support::*;

#[test]
fn middleware_cannot_expand_the_admitted_provider_output_cap() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::text("must remain unused")],
            Default::default(),
            true,
        )
        .await?;
        fixture
            .host
            .install(
                hooks::contribution(
                    wrap_model_call()?,
                    "expand-budget",
                    0,
                    |args: HookArgs<ModelCall, GenerateResult>, _| {
                        async move {
                            let (mut call, next) = args.into_parts();
                            call.request.options.max_output_tokens = Some(100);
                            next.unwrap().call(call).await
                        }
                        .boxed_local()
                    },
                )?,
                core::LoadOptions::default(),
            )?
            .await?;
        fixture
            .start(
                "request-cap",
                TurnLimits {
                    max_output_tokens: 2,
                    ..Default::default()
                },
            )
            .await?;
        loop {
            let state = fixture
                .agent
                .inspect(Harness::lookup("request-cap"))
                .await?
                .unwrap();
            if state.task.status == TaskStatus::Parked {
                break;
            }
            assert!(!state.task.is_terminal());
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert!(fixture.model.requests.borrow().is_empty());
        fixture.host.close().await
    });
}

#[test]
fn middleware_tool_calls_and_reported_usage_cannot_expand_execution_budgets() {
    for bound in ["steps", "tools", "tokens"] {
        run(|driver| async move {
            let directory = tempfile::tempdir().unwrap();
            let fixture = open(
                driver,
                directory.path(),
                vec![Reply::text("provider result")],
                Default::default(),
                true,
            )
            .await?;
            fixture
                .host
                .install(
                    hooks::contribution(
                        wrap_model_call()?,
                        "replace-outcome",
                        0,
                        move |args: HookArgs<ModelCall, GenerateResult>, _| {
                            async move {
                                let (call, next) = args.into_parts();
                                let operation = call.request.request_id.clone();
                                let mut result = next.unwrap().call(call).await?;
                                if let GenerateResult::Response { response } = &mut result {
                                    if bound == "tokens" {
                                        response.usage.output_tokens = Some(100);
                                    } else {
                                        response.finish_reason = FinishReason::ToolCalls;
                                        response.message.content =
                                            (0..if bound == "tools" { 2 } else { 1 })
                                                .map(|index| ContentPart::ToolCall {
                                                    call: ToolCall {
                                                        id: format!("{operation}:{index}"),
                                                        name: "fixture.double".into(),
                                                        arguments: serde_json::json!({"value":21}),
                                                        extensions: vec![],
                                                    },
                                                })
                                                .collect();
                                    }
                                }
                                Ok(result)
                            }
                            .boxed_local()
                        },
                    )?,
                    core::LoadOptions::default(),
                )?
                .await?;
            let limits = TurnLimits {
                max_steps: if bound == "steps" { 1 } else { 8 },
                max_tool_calls: if bound == "tools" { 1 } else { 8 },
                max_output_tokens: 2,
                ..Default::default()
            };
            fixture.start(bound, limits).await?;
            let completed = fixture.terminal(bound).await?;
            assert!(
                matches!(completed.task.outcome, Some(TaskOutcome::Failed { .. })),
                "{completed:?}"
            );
            assert_eq!(fixture.model.requests.borrow().len(), 1);
            assert_eq!(
                fixture.tool.calls.borrow().len(),
                if bound == "tokens" { 0 } else { 1 }
            );
            assert!(completed.task.checkpoint["output_tokens"].as_u64().unwrap() <= 2);
            fixture.host.close().await
        });
    }
}

#[test]
fn middleware_awaiting_after_confirmed_provider_result_remains_deadline_bound() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::text("confirmed")],
            Default::default(),
            true,
        )
        .await?;
        fixture
            .host
            .install(
                hooks::contribution(
                    wrap_model_call()?,
                    "deadline",
                    0,
                    |args: HookArgs<ModelCall, GenerateResult>, context| {
                        async move {
                            let (call, next) = args.into_parts();
                            let _confirmed = next.unwrap().call(call).await?;
                            Err(context.cancellation().cancelled().await)
                        }
                        .boxed_local()
                    },
                )?,
                core::LoadOptions::default(),
            )?
            .await?;
        fixture
            .start(
                "middleware-deadline",
                TurnLimits {
                    max_duration_ms: 2_000,
                    ..Default::default()
                },
            )
            .await?;
        let completed = fixture.terminal("middleware-deadline").await?;
        assert!(
            matches!(completed.task.outcome, Some(TaskOutcome::Failed { .. })),
            "{completed:?}"
        );
        assert_eq!(fixture.model.requests.borrow().len(), 1);
        assert!(
            fixture
                .events("middleware-deadline")
                .await?
                .iter()
                .any(|entry| entry.event.kind == "swarmie.agent.model-provider-confirmed")
        );
        fixture.host.close().await
    });
}
