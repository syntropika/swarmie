//! Context policies choose bounded source intervals before transcript materialization.

use futures::FutureExt;
use serde_json::json;
use std::{cell::Cell, rc::Rc};
use swarmie_agent_contracts::{ContextRange, ContextSelection, MAX_CONTEXT_EVENTS, select_context};
use swarmie_agent_loop_fixture::{hooks, model::Reply};
use swarmie_session_contracts::{AppendEvents, CreateSession};
use swarmie_task_contracts::TaskOutcome;
use tembra::{Result, core};
use tembra_hooks::HookArgs;

pub mod conversation_support;
pub mod support;
use conversation_support::*;
use support::*;

#[test]
fn selection_can_choose_prefix_and_suffix_before_oversized_history_is_materialized() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::text("selected context")],
            Default::default(),
            true,
        )
        .await?;
        fixture
            .sessions
            .create(CreateSession {
                session_id: "large".into(),
                metadata: json!({}),
            })
            .await?;
        let events = (0..4200)
            .map(|index| {
                event(
                    "alpha",
                    &format!("old-{index}"),
                    &format!("history {index}"),
                )
            })
            .collect::<Vec<_>>();
        for (index, batch) in events.chunks(128).enumerate() {
            fixture
                .sessions
                .append(AppendEvents {
                    session_id: "large".into(),
                    batch_id: format!("history-{index}"),
                    expected_head: None,
                    events: batch.to_vec(),
                })
                .await?;
        }
        seed(
            &fixture.sessions,
            "large",
            "alpha",
            "current-input",
            "latest input",
        )
        .await?;
        let refused = fixture
            .agent
            .start(request("large", "alpha", "unselected", "current-input"))
            .await?;
        settle(&fixture.tasks, &refused).await?;
        let refused = fixture
            .agent
            .inspect(lookup("large", "alpha", "unselected"))
            .await?
            .unwrap();
        assert!(
            matches!(refused.task.outcome, Some(TaskOutcome::Failed { .. })),
            "oversized default context must fail explicitly"
        );
        assert!(
            fixture.model.requests.borrow().is_empty(),
            "default selection must not silently truncate history"
        );
        let observed = Rc::new(Cell::new(0));
        let selected = observed.clone();
        fixture
            .host
            .install(
                hooks::contribution(
                    select_context()?,
                    "source-window",
                    0,
                    move |args: HookArgs<ContextSelection, ContextSelection>, _| {
                        let mut plan = args.input().clone();
                        assert_eq!(plan.turn.conversation_id, "alpha");
                        assert_eq!(plan.input_sequence, 4201);
                        assert!(plan.through_sequence >= 4201);
                        plan.ranges = vec![
                            ContextRange {
                                after_sequence: 0,
                                through_sequence: 1,
                                current_turn_only: false,
                            },
                            ContextRange {
                                after_sequence: 4200,
                                through_sequence: 4201,
                                current_turn_only: false,
                            },
                        ];
                        plan.budget.max_events = 2;
                        selected.set(selected.get() + 1);
                        async move { Ok(plan) }.boxed_local()
                    },
                )?,
                core::LoadOptions::default(),
            )?
            .await?;
        let admitted = fixture
            .agent
            .start(request("large", "alpha", "selected", "current-input"))
            .await?;
        settle(&fixture.tasks, &admitted).await?;
        let admitted = fixture
            .agent
            .inspect(lookup("large", "alpha", "selected"))
            .await?
            .unwrap();
        assert!(
            matches!(admitted.task.outcome, Some(TaskOutcome::Completed { .. })),
            "{admitted:?}"
        );
        assert_eq!(observed.get(), 1);
        let requests = fixture.model.requests.borrow().clone();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            texts(&requests[0].messages),
            vec!["history 0", "latest input"]
        );
        drop(requests);
        fixture.host.close().await
    });
}

fn invalid_policy(case: &str, change: impl Fn(&mut ContextSelection) + 'static) -> Result<()> {
    let case = case.to_owned();
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::text("must not execute")],
            Default::default(),
            true,
        )
        .await?;
        seed(&fixture.sessions, "selection", "alpha", "input", "input").await?;
        fixture
            .host
            .install(
                hooks::contribution(
                    select_context()?,
                    &case,
                    0,
                    move |args: HookArgs<ContextSelection, ContextSelection>, _| {
                        let mut plan = args.input().clone();
                        change(&mut plan);
                        async move { Ok(plan) }.boxed_local()
                    },
                )?,
                core::LoadOptions::default(),
            )?
            .await?;
        let record = fixture
            .agent
            .start(request("selection", "alpha", "turn", "input"))
            .await?;
        settle(&fixture.tasks, &record).await?;
        let record = fixture
            .agent
            .inspect(lookup("selection", "alpha", "turn"))
            .await?
            .unwrap();
        assert!(
            matches!(record.task.outcome, Some(TaskOutcome::Failed { .. })),
            "{record:?}"
        );
        assert!(fixture.model.requests.borrow().is_empty());
        fixture.host.close().await
    });
    Ok(())
}

#[test]
fn selection_cannot_expand_budget_escape_conversation_or_drop_input() -> Result<()> {
    // Each helper owns a fresh real Host, so refused policies cannot affect another case.
    invalid_policy("expand-budget", |plan| {
        plan.budget.max_events = MAX_CONTEXT_EVENTS + 1
    })?;
    invalid_policy("change-conversation", |plan| {
        plan.turn.conversation_id = "beta".into()
    })?;
    invalid_policy("drop-input", |plan| plan.ranges.clear())?;
    invalid_policy("overlap", |plan| plan.ranges.push(plan.ranges[0].clone()))?;
    Ok(())
}

#[test]
fn explicit_suffix_selection_retains_same_conversation_summary_created_after_input() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::text("summary used")],
            Default::default(),
            true,
        )
        .await?;
        seed(
            &fixture.sessions,
            "suffix",
            "alpha",
            "input",
            "admitted input",
        )
        .await?;
        seed(
            &fixture.sessions,
            "suffix",
            "alpha",
            "summary",
            "persisted summary after input",
        )
        .await?;
        fixture
            .host
            .install(
                hooks::contribution(
                    select_context()?,
                    "explicit-suffix",
                    0,
                    |args: HookArgs<ContextSelection, ContextSelection>, _| {
                        let mut plan = args.input().clone();
                        plan.ranges = vec![ContextRange {
                            after_sequence: 0,
                            through_sequence: 2,
                            current_turn_only: false,
                        }];
                        async move { Ok(plan) }.boxed_local()
                    },
                )?,
                core::LoadOptions::default(),
            )?
            .await?;
        let record = fixture
            .agent
            .start(request("suffix", "alpha", "turn", "input"))
            .await?;
        settle(&fixture.tasks, &record).await?;
        let requests = fixture.model.requests.borrow().clone();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            texts(&requests[0].messages),
            vec!["admitted input", "persisted summary after input"]
        );
        fixture.host.close().await
    });
}
