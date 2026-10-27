mod support;

use futures::{FutureExt, executor::LocalPool};
use std::rc::Rc;
use support::{Scenario, agent, request};
use swarmie_agent_loop::*;
use tembra::Host;

#[test]
fn standalone_loop_calls_other_orbs_without_a_manager_or_task_store() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let scenario = Rc::new(Scenario::default());
    let create = support::factory(&host, scenario.clone());
    host.install(vec![orb(create)]).unwrap();
    let reference = host.get(SERVICE, vec!["main".into()], "alice").unwrap();
    let output: Output = pool
        .run_until(host.root("alice", move |ctx| {
            async move {
                reference
                    .call::<_, ExecutionResult>(&ctx, Method::Execute.as_str(), request())
                    .await
            }
            .boxed_local()
        }))
        .unwrap()
        .unwrap();
    assert_eq!(output.reason, FinishReason::Complete);
    assert_eq!(output.messages.len(), 3);
    assert_eq!(
        output.usage,
        Usage {
            input_tokens: 4,
            output_tokens: 6
        }
    );
    assert_eq!(scenario.models.borrow().len(), 2);
    assert_eq!(scenario.effects.borrow().len(), 1);
    pool.run_until(host.close()).unwrap();
    assert_eq!(host.kernel().diagnostics().resources, 0);
}

#[test]
fn denied_tools_never_dispatch_and_known_errors_are_model_input() {
    for blocked in [true, false] {
        let mut pool = LocalPool::new();
        let host = Host::new(pool.spawner());
        let scenario = Rc::new(Scenario::default());
        scenario.block.set(blocked);
        scenario.known_failure.set(!blocked);
        let agent = agent(&host, scenario.clone());
        let output = pool
            .run_until(host.root("alice", move |ctx| {
                async move { Ok(agent.execute(&ctx, request()).await) }.boxed_local()
            }))
            .unwrap()
            .unwrap();
        assert!(
            matches!(
                &output.messages[1],
                Message::Tool {
                    result: ToolResult::Blocked(_),
                    ..
                }
            ) == blocked
        );
        assert!(
            matches!(
                &output.messages[1],
                Message::Tool {
                    result: ToolResult::Failed(_),
                    ..
                }
            ) != blocked
        );
        assert_eq!(scenario.effects.borrow().len(), usize::from(!blocked));
        pool.run_until(host.close()).unwrap();
    }
}

#[test]
fn turn_limit_finishes_the_tool_batch_without_another_model_call() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let scenario = Rc::new(Scenario::default());
    let agent = agent(&host, scenario.clone());
    let mut request = request();
    request.max_turns = std::num::NonZeroU32::new(1).unwrap();
    let output = pool
        .run_until(host.root("alice", move |ctx| {
            async move { Ok(agent.execute(&ctx, request).await) }.boxed_local()
        }))
        .unwrap()
        .unwrap();
    assert_eq!(output.reason, FinishReason::TurnLimit);
    assert_eq!(output.messages.len(), 2);
    assert_eq!(scenario.models.borrow().len(), 1);
    assert_eq!(scenario.effects.borrow().len(), 1);
    pool.run_until(host.close()).unwrap();
}

#[test]
fn cancellation_after_a_model_response_prevents_tool_dispatch() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let scenario = Rc::new(Scenario::default());
    scenario.cancel_model.set(true);
    let agent = agent(&host, scenario.clone());
    let error = pool
        .run_until(host.root("alice", move |ctx| {
            async move { Ok(agent.execute(&ctx, request()).await) }.boxed_local()
        }))
        .unwrap()
        .unwrap_err();
    assert_eq!(error.error.code, tembra::Code::Cancelled);
    assert_eq!(scenario.models.borrow().len(), 1);
    assert!(scenario.effects.borrow().is_empty());
    pool.run_until(host.close()).unwrap();
}

#[test]
fn malformed_history_and_duplicate_tool_definitions_are_refused_before_dispatch() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let scenario = Rc::new(Scenario::default());
    for duplicate in [true, false] {
        let agent = agent(&host, scenario.clone());
        let mut input = request();
        if duplicate {
            input.tools.push(input.tools[0].clone());
        } else {
            input.context.messages.push(Message::Tool {
                call: ToolCallId::new("unrequested").unwrap(),
                result: ToolResult::Success(serde_json::Value::Null),
            });
        }
        let error = pool
            .run_until(host.root("alice", move |ctx| {
                async move { Ok(agent.execute(&ctx, input).await) }.boxed_local()
            }))
            .unwrap()
            .unwrap_err();
        assert_eq!(error.error.code, tembra::Code::Invalid);
    }
    assert!(scenario.models.borrow().is_empty());
    assert!(scenario.effects.borrow().is_empty());
    pool.run_until(host.close()).unwrap();
}
