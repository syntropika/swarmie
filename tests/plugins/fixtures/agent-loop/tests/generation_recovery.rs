//! Live-object recovery after the generation's own graph rejects terminal publication.

use futures::FutureExt;
use std::time::Duration;

use swarmie_agent_contracts::*;
use swarmie_agent_loop_fixture::{
    gate::{self, AgentGate, AgentGateInput},
    model::Reply,
};
use swarmie_model_contracts::{ContentPart, GenerateResult, ProviderErrorKind, Role};
use swarmie_task_contracts::{TaskOutcome, TaskStatus};
use tembra::{Driver, core};

pub mod support;
use support::*;

fn lookup(turn: &str) -> TurnLookup {
    TurnLookup {
        session_id: format!("session-{turn}"),
        conversation_id: "conversation".into(),
        turn_id: turn.into(),
    }
}

#[test]
fn rejected_terminal_publication_recovers_success_and_provider_error_without_resubmission() {
    for failure in [false, true] {
        run(|driver| async move {
            let directory = tempfile::tempdir().unwrap();
            let fixture = open(
                driver,
                directory.path(),
                vec![if failure {
                    Reply::Error
                } else {
                    Reply::text("recovered answer")
                }],
                Default::default(),
                true,
            )
            .await?;
            fixture.model.reject_terminal.set(true);
            fixture.start("recover", Default::default()).await?;
            let completed = fixture.terminal("recover").await?;
            assert_eq!(fixture.model.requests.borrow().len(), 1);
            assert!(fixture.model.publication_failures.get() > 0);
            assert!(fixture.model.recovery_calls.get() > 0);
            assert!(
                matches!(&completed.task.outcome, Some(TaskOutcome::Failed { .. })) == failure,
                "{completed:?}"
            );
            let events = fixture.events("recover").await?;
            let confirmed = events
                .iter()
                .find(|entry| entry.event.kind == "swarmie.agent.model-provider-confirmed")
                .expect("recovered result was durably committed");
            assert_eq!(
                confirmed.event.payload["result"]["type"],
                if failure { "error" } else { "response" }
            );
            assert!(completed.task.checkpoint["response_event"].is_string());
            fixture.host.close().await?;
            assert!(fixture.model.dropped.get() > 0);
            assert_eq!(fixture.model.premature_drop.get(), 0);
            Ok(())
        });
    }
}

#[test]
fn successful_start_and_ack_with_stale_running_state_still_use_retained_completion() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::text("stale projection answer")],
            Default::default(),
            true,
        )
        .await?;
        fixture.model.reject_terminal.set(true);
        fixture.model.suppress_publication_error.set(true);
        fixture.start("stale", Default::default()).await?;
        let completed = fixture.terminal("stale").await?;
        assert!(
            matches!(completed.task.outcome, Some(TaskOutcome::Completed { .. })),
            "{completed:?}"
        );
        assert_eq!(fixture.model.requests.borrow().len(), 1);
        assert!(fixture.model.publication_failures.get() > 0);
        assert!(fixture.model.recovery_calls.get() > 1);
        fixture.host.close().await?;
        assert_eq!(fixture.model.premature_drop.get(), 0);
        Ok(())
    });
}

#[test]
fn cancelled_stream_retains_reference_until_recovered_physical_settlement_is_committed() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::Hang],
            Default::default(),
            true,
        )
        .await?;
        fixture
            .host
            .install(gate::facet()?, core::LoadOptions::default())?
            .await?;
        let gate = fixture.host.get(AgentGate, AgentGateInput {}).await?;
        *fixture.model.physical_gate.borrow_mut() = Some(gate.clone());
        fixture.model.reject_terminal.set(true);
        fixture.start("stream-cancel", Default::default()).await?;
        while fixture.model.requests.borrow().is_empty() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        fixture.agent.cancel(lookup("stream-cancel")).await?;
        while gate.entered().value()? == 0 {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let active = fixture
            .agent
            .inspect(lookup("stream-cancel"))
            .await?
            .unwrap();
        assert!(!active.task.is_terminal());
        assert_eq!(
            fixture.model.dropped.get(),
            0,
            "physical work still owns the live object"
        );
        gate.release_gate().await?;
        let completed = fixture.terminal("stream-cancel").await?;
        assert!(
            matches!(completed.task.outcome, Some(TaskOutcome::Aborted { .. })),
            "{completed:?}"
        );
        let events = fixture.events("stream-cancel").await?;
        let confirmed = events
            .iter()
            .find(|entry| entry.event.kind == "swarmie.agent.model-provider-confirmed")
            .expect("cleanup persisted recovered cancellation before releasing its claim");
        let result: GenerateResult =
            serde_json::from_value(confirmed.event.payload["result"].clone()).unwrap();
        assert!(
            matches!(result, GenerateResult::Error { error } if error.kind == ProviderErrorKind::Cancelled)
        );
        assert!(fixture.model.publication_failures.get() > 0);
        assert_eq!(fixture.model.requests.borrow().len(), 1);
        fixture.host.close().await?;
        assert_eq!(fixture.model.premature_drop.get(), 0);
        Ok(())
    });
}

#[test]
fn opaque_v2_signed_tool_continuations_keep_order_while_private_reasoning_stays_out_of_transcript()
{
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver,
            directory.path(),
            vec![Reply::OpaqueTool, Reply::text("42")],
            Default::default(),
            true,
        )
        .await?;
        fixture.model.reject_terminal.set(true);
        fixture.start("opaque", Default::default()).await?;
        let completed = fixture.terminal("opaque").await?;
        assert!(
            matches!(completed.task.outcome, Some(TaskOutcome::Completed { .. })),
            "{completed:?}"
        );
        assert_eq!(fixture.model.requests.borrow().len(), 2);
        assert_eq!(fixture.tool.calls.borrow().len(), 1);
        let replay = fixture.model.requests.borrow()[1]
            .messages
            .iter()
            .find(|message| message.role == Role::Assistant)
            .expect("persisted assistant content was reconstructed for the next model step")
            .clone();
        assert_eq!(replay.content.len(), 4);
        assert!(
            matches!(&replay.content[0], ContentPart::Extension { extension } if extension.version == 2 && extension.value["value"]["signature"] == "signed-opaque-value")
        );
        assert!(matches!(&replay.content[1], ContentPart::Text { text } if text == "before tool"));
        assert!(
            matches!(&replay.content[2], ContentPart::ToolCall { call } if call.extensions[0].value["value"][0] == "opaque-tool-signature")
        );
        assert!(
            matches!(&replay.content[3], ContentPart::Extension { extension } if extension.version == 2 && extension.value["value"]["data"] == "redacted-opaque-value")
        );
        assert!(
            !replay
                .content
                .iter()
                .any(|part| matches!(part, ContentPart::Reasoning { .. }))
        );
        assert!(!replay.content.iter().any(
            |part| matches!(part, ContentPart::Text { text } if text.contains("private thought"))
        ));
        let events = fixture.events("opaque").await?;
        let message = events
            .iter()
            .find(|entry| {
                entry.event.kind == "swarmie.message" && entry.event.payload["role"] == "assistant"
            })
            .unwrap();
        let persisted: Vec<ContentPart> =
            serde_json::from_value(message.event.payload["content"].clone()).unwrap();
        assert_eq!(persisted.len(), 4);
        assert!(
            persisted
                .iter()
                .all(|part| !matches!(part, ContentPart::Reasoning { .. }))
        );
        assert_eq!(fixture.model.premature_drop.get(), 0);
        fixture.host.close().await
    });
}

#[test]
fn host_retirement_joins_stream_resource_and_reopening_never_resubmits() {
    run(|driver| async move {
        let directory = tempfile::tempdir().unwrap();
        let fixture = open(
            driver.clone(),
            directory.path(),
            vec![Reply::Hang],
            Default::default(),
            true,
        )
        .await?;
        fixture.model.reject_terminal.set(true);
        let (release_worker, worker) = futures::channel::oneshot::channel();
        *fixture.model.close_barrier.borrow_mut() = Some(worker);
        fixture.start("host-close", Default::default()).await?;
        while fixture.model.requests.borrow().is_empty() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let closing_host = fixture.host.clone();
        let (closed, completed_close) = futures::channel::oneshot::channel();
        driver.spawn(
            async move {
                let _ = closed.send(closing_host.close().await);
            }
            .boxed_local(),
        );
        while fixture.model.backend_closes.get() == 0 {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert_eq!(
            fixture.model.dropped.get(),
            0,
            "the Resource still owns the physical stream worker"
        );
        release_worker.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), completed_close)
            .await
            .expect("retirement must not poll an invalidated generation forever")
            .unwrap()?;
        assert_eq!(fixture.model.requests.borrow().len(), 1);
        assert_eq!(fixture.model.backend_closes.get(), 1);
        assert!(fixture.model.close_publication_failures.get() > 0);
        assert!(fixture.model.dropped.get() > 0);
        assert_eq!(fixture.model.premature_drop.get(), 0);
        drop(fixture);
        let reopened = open(
            driver,
            directory.path(),
            vec![Reply::text("must remain unused")],
            Default::default(),
            true,
        )
        .await?;
        loop {
            let record = reopened.agent.inspect(lookup("host-close")).await?.unwrap();
            if record.task.status == TaskStatus::Parked {
                break;
            }
            assert!(
                !record.task.is_terminal(),
                "destroying a live object cannot invent a confirmed provider result"
            );
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        assert!(
            reopened.model.requests.borrow().is_empty(),
            "the persisted admission marker prevents another provider submission after retirement"
        );
        let events = reopened.events("host-close").await?;
        assert!(
            events
                .iter()
                .any(|entry| entry.event.kind == "swarmie.agent.model-started")
        );
        assert!(
            !events
                .iter()
                .any(|entry| entry.event.kind == "swarmie.agent.model-provider-confirmed")
        );
        reopened.host.close().await
    });
}
