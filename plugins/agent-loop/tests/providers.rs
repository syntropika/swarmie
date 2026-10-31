#[path = "support/request.rs"]
mod requests;
#[path = "support/scenario.rs"]
mod scenario;

use futures::{FutureExt, executor::LocalPool};
use requests::request;
use scenario::Scenario;
use std::rc::Rc;
use swarmie_agent_loop::*;
use swarmie_providers::mock::{MockModel, Step};
use tembra::{Code, Host, Orb, Service};

fn response(text: &str, tools: bool) -> ModelResponse {
    ModelResponse {
        protection: swarmie_providers::SubmissionProtection::Unprotected,
        message: AssistantMessage {
            provider: None,
            content: text.into(),
            tool_calls: if tools {
                vec![ToolCall {
                    id: ToolCallId::new("call-1").unwrap(),
                    name: ToolName::new("counter.add").unwrap(),
                    arguments: serde_json::json!({"amount":1}),
                }]
            } else {
                vec![]
            },
        },
        usage: Usage {
            input_tokens: 2,
            output_tokens: 3,
        },
    }
}

#[test]
fn ordinary_failure_keeps_confirmed_messages_and_resumes_without_repeating_a_tool() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let model = Rc::new(MockModel::new([
        Step::Reply(response("", true)),
        Step::CancelReply(response("Done", false)),
    ]));
    let tools = Rc::new(Scenario::default());
    let agent = Agent::new(model.clone(), tools.clone());
    let first = agent.clone();
    let error = pool
        .run_until(host.root("alice", move |ctx| {
            async move { Ok(first.execute(&ctx, request()).await) }.boxed_local()
        }))
        .unwrap()
        .unwrap_err();
    assert_eq!(error.error.code, Code::Cancelled);
    let wire = serde_json::to_value(&error).unwrap();
    let error: ExecutionFailure = serde_json::from_value(wire).unwrap();
    let state = *error.state.unwrap();
    assert_eq!(state.messages().len(), 2);
    let Phase::ModelPending { operation, .. } = state.phase() else {
        panic!("missing pending operation")
    };
    let operation = *operation;
    let output = pool
        .run_until(host.root("alice", move |ctx| {
            async move { Ok(agent.resume(&ctx, state).await) }.boxed_local()
        }))
        .unwrap()
        .unwrap();
    assert_eq!(output.messages.len(), 3);
    assert_eq!(
        output.usage,
        Usage {
            input_tokens: 4,
            output_tokens: 6
        }
    );
    assert_eq!(tools.effects.borrow().len(), 1);
    assert_eq!(model.calls().len(), 2);
    assert_eq!(model.recoveries(), vec![operation]);
    assert_eq!(model.remaining(), 0);
    pool.run_until(host.close()).unwrap();
}

#[test]
fn ordinary_mock_failure_preserves_the_original_error_code_and_reason() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let cause = tembra::Error::new(Code::Invalid, "mock.invalid_request", "request rejected");
    let model = Rc::new(MockModel::new([Step::Fail(cause.clone())]));
    let agent = Agent::new(model, Rc::new(Scenario::default()));
    let error = pool
        .run_until(host.root("alice", move |ctx| {
            async move { Ok(agent.execute(&ctx, request()).await) }.boxed_local()
        }))
        .unwrap()
        .unwrap_err();
    assert_eq!(error.error, cause);
    assert!(matches!(
        error.state.unwrap().phase(),
        Phase::ModelPending { .. }
    ));
    pool.run_until(host.close()).unwrap();
}

#[test]
fn invalid_resume_input_has_no_progress_and_never_reaches_providers() {
    for managed in [false, true] {
        let mut pool = LocalPool::new();
        let host = Host::new(pool.spawner());
        let model = Rc::new(MockModel::new([]));
        let tools = Rc::new(Scenario::default());
        let agent = Agent::new(model.clone(), tools.clone());
        let state = State::start(request()).unwrap();
        let mut wire = serde_json::to_value(state).unwrap();
        wire["turn"] = serde_json::json!(u32::MAX);
        let state: State = serde_json::from_value(wire).unwrap();
        let error = if managed {
            host.install(vec![orb(move |_| Ok(agent.clone()))]).unwrap();
            let reference = host.get(SERVICE, vec!["main".into()], "alice").unwrap();
            pool.run_until(host.root("alice", move |ctx| {
                async move {
                    reference
                        .call::<_, ExecutionResult>(&ctx, Method::Resume.as_str(), state)
                        .await
                }
                .boxed_local()
            }))
            .unwrap()
            .unwrap_err()
        } else {
            pool.run_until(host.root("alice", move |ctx| {
                async move { Ok(agent.resume(&ctx, state).await) }.boxed_local()
            }))
            .unwrap()
            .unwrap_err()
        };
        assert_eq!(error.error.code, Code::Invalid);
        assert_eq!(error.error.reason, FailureReason::InvalidInput.as_str());
        assert!(error.state.is_none());
        assert!(model.calls().is_empty());
        assert!(model.recoveries().is_empty());
        assert!(tools.effects.borrow().is_empty());
        assert!(tools.reconciled_tools.borrow().is_empty());
        pool.run_until(host.close()).unwrap();
        assert_eq!(host.kernel().diagnostics().calls, 0);
        assert_eq!(host.kernel().diagnostics().resources, 0);
    }
}

#[test]
fn independent_provider_bindings_in_the_same_orb_route_to_their_own_models() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let a = Rc::new(MockModel::new([Step::Reply(response("a", false))]));
    let b = Rc::new(MockModel::new([Step::Reply(response("b", false))]));
    let owner = host.clone();
    let consumer = Orb::new("fixture.router", 1).service(
        Service::new("fixture.router", move |declaration| {
            Ok((
                ModelService::bind(
                    &declaration,
                    owner.clone(),
                    "fixture.model_a",
                    vec!["main".into()],
                )?,
                ModelService::bind(
                    &declaration,
                    owner.clone(),
                    "fixture.model_b",
                    vec!["main".into()],
                )?,
            ))
        })
        .method("read", |env, ctx, (): ()| async move {
            let operation: OperationId =
                serde_json::from_str("\"a435e980-ef81-4615-8869-c21485f965ee\"").unwrap();
            let request = ModelRequest {
                context: AgentContext::default(),
                tools: vec![],
            };
            Ok((
                env.0
                    .complete(&ctx, operation, &request)
                    .await?
                    .message
                    .content,
                env.1
                    .complete(&ctx, operation, &request)
                    .await?
                    .message
                    .content,
            ))
        }),
    );
    host.install(vec![
        swarmie_providers::mock::orb("fixture.model_a", a),
        swarmie_providers::mock::orb("fixture.model_b", b),
        consumer,
    ])
    .unwrap();
    let reference = host
        .get("fixture.router", vec!["main".into()], "alice")
        .unwrap();
    let result: (String, String) = pool
        .run_until(host.root("alice", move |ctx| {
            async move { reference.call(&ctx, "read", ()).await }.boxed_local()
        }))
        .unwrap();
    assert_eq!(result, ("a".into(), "b".into()));
    pool.run_until(host.close()).unwrap();
}

#[test]
fn managed_ordinary_orb_returns_recoverable_failure_as_typed_result() {
    for cancelled in [false, true] {
        let mut pool = LocalPool::new();
        let host = Host::new(pool.spawner());
        let model = Rc::new(MockModel::new([if cancelled {
            Step::CancelReply(response("Done", false))
        } else {
            Step::LoseReply(response("Done", false))
        }]));
        let agent = Agent::new(model.clone(), Rc::new(Scenario::default()));
        host.install(vec![orb(move |_| Ok(agent.clone()))]).unwrap();
        let reference = Rc::new(host.get(SERVICE, vec!["main".into()], "alice").unwrap());
        let first = reference.clone();
        let result: ExecutionResult = pool
            .run_until(host.root("alice", move |ctx| {
                async move { first.call(&ctx, Method::Execute.as_str(), request()).await }
                    .boxed_local()
            }))
            .unwrap();
        let state = *result.unwrap_err().state.unwrap();
        let result: ExecutionResult = pool
            .run_until(host.root("alice", move |ctx| {
                async move { reference.call(&ctx, Method::Resume.as_str(), state).await }
                    .boxed_local()
            }))
            .unwrap();
        assert_eq!(result.unwrap().messages.len(), 1);
        assert_eq!(model.calls().len(), 1);
        pool.run_until(host.close()).unwrap();
    }
}

#[test]
fn mock_receipt_checks_owner_and_exact_request_without_consuming_another_step() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let model = Rc::new(MockModel::new([Step::Reply(response("Done", false))]));
    let operation: OperationId =
        serde_json::from_str("\"a435e980-ef81-4615-8869-c21485f965ee\"").unwrap();
    let request = ModelRequest {
        context: AgentContext::default(),
        tools: vec![],
    };
    let first = model.clone();
    let input = request.clone();
    pool.run_until(host.root("alice", move |ctx| {
        async move { first.complete(&ctx, operation, &input).await }.boxed_local()
    }))
    .unwrap();
    let foreign = model.clone();
    let input = request.clone();
    let error = pool
        .run_until(host.root("bob", move |ctx| {
            async move { foreign.recover(&ctx, operation, &input).await }.boxed_local()
        }))
        .unwrap_err();
    assert_eq!(error.code, Code::Forbidden);
    let changed = model.clone();
    let mut input = request.clone();
    input.context.instructions.push_str("changed");
    let error = pool
        .run_until(host.root("alice", move |ctx| {
            async move { changed.recover(&ctx, operation, &input).await }.boxed_local()
        }))
        .unwrap_err();
    assert_eq!(error.code, Code::Conflict);
    let recover = model.clone();
    let result = pool
        .run_until(host.root("alice", move |ctx| {
            async move { recover.recover(&ctx, operation, &request).await }.boxed_local()
        }))
        .unwrap();
    assert!(matches!(result, Recovery::Completed(_)));
    assert_eq!(model.calls().len(), 1);
    assert_eq!(model.remaining(), 0);
    pool.run_until(host.close()).unwrap();
}
