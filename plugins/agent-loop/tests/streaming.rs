#[path = "support/request.rs"]
mod input;
#[path = "support/scenario.rs"]
mod scenario;

use futures::{FutureExt, executor::LocalPool};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};
use swarmie_agent_loop::*;
use swarmie_models::{
    ModelProgress, Progress,
    mock::{self, MockModel, Step},
};
use tembra::{Host, Orb, Service};

#[test]
fn concurrent_native_generations_filter_progress_by_operation() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let gates = Rc::new(RefCell::new(BTreeMap::new()));
    let entered = gates.clone();
    host.install(vec![Orb::new("fixture.stream", 1).service(
        Service::new("fixture.stream", |d| Progress::bind(&d)).method(
            ModelMethod::Stream.as_str(),
            move |progress, ctx, (operation, request): (OperationId, ModelRequest)| {
                let gates = entered.clone();
                async move {
                    let Message::User { content } = &request.context.messages[0] else {
                        panic!("fixture needs a user message")
                    };
                    let content = content.clone();
                    let (send, receive) = futures::channel::oneshot::channel();
                    gates.borrow_mut().insert(content.clone(), send);
                    progress.publish(
                        &ctx,
                        ModelProgress {
                            operation,
                            content: content.clone(),
                        },
                    );
                    receive.await.unwrap();
                    let content = format!("{content}!");
                    progress.publish(
                        &ctx,
                        ModelProgress {
                            operation,
                            content: content.clone(),
                        },
                    );
                    Ok(ModelResponse {
                        message: AssistantMessage {
                            content,
                            tool_calls: vec![],
                        },
                        usage: Usage::default(),
                    })
                }
            },
        ),
    )])
    .unwrap();
    let binding = host.clone();
    host.install(vec![orb(move |d| {
        Ok(Agent::new(
            Rc::new(ModelService::bind(
                &d,
                binding.clone(),
                "fixture.stream",
                vec!["shared".into()],
            )?),
            Rc::new(scenario::Scenario::default()),
        )
        .with_progress(Progress::bind(&d)?))
    })])
    .unwrap();
    let a = Rc::new(host.get(SERVICE, vec!["a".into()], "alice").unwrap());
    let b = Rc::new(host.get(SERVICE, vec!["b".into()], "alice").unwrap());
    pool.run_until(a.ready()).unwrap();
    pool.run_until(b.ready()).unwrap();
    let events_a = Rc::new(RefCell::new(vec![]));
    let events_b = Rc::new(RefCell::new(vec![]));
    let collect_a = events_a.clone();
    let collect_b = events_b.clone();
    let observe_a = a
        .state::<Option<ModelProgress>>()
        .unwrap()
        .subscribe(move |v| {
            if let Some(v) = v.as_ref() {
                collect_a.borrow_mut().push(v.clone());
            }
            Ok(())
        })
        .unwrap();
    let observe_b = b
        .state::<Option<ModelProgress>>()
        .unwrap()
        .subscribe(move |v| {
            if let Some(v) = v.as_ref() {
                collect_b.borrow_mut().push(v.clone());
            }
            Ok(())
        })
        .unwrap();
    let mut running = vec![];
    for (label, reference) in [("A", a), ("B", b)] {
        let mut request = input::request();
        request.context.messages = vec![Message::User {
            content: label.into(),
        }];
        request.tools.clear();
        let caller = host.clone();
        let future = async move {
            caller
                .root("alice", move |ctx| {
                    async move {
                        reference
                            .call::<_, ExecutionResult>(&ctx, Method::Execute.as_str(), request)
                            .await
                    }
                    .boxed_local()
                })
                .await
        };
        let (future, result) = future.remote_handle();
        use futures::task::LocalSpawnExt;
        pool.spawner().spawn_local(future).unwrap();
        running.push(result);
    }
    pool.run_until_stalled();
    assert_eq!(gates.borrow().len(), 2);
    assert_eq!(events_a.borrow()[0].content, "A");
    assert_eq!(events_b.borrow()[0].content, "B");
    assert_ne!(
        events_a.borrow()[0].operation,
        events_b.borrow()[0].operation
    );
    gates.borrow_mut().remove("A").unwrap().send(()).unwrap();
    pool.run_until_stalled();
    assert_eq!(events_a.borrow().last().unwrap().content, "A!");
    assert_eq!(events_b.borrow().last().unwrap().content, "B");
    gates.borrow_mut().remove("B").unwrap().send(()).unwrap();
    for result in running {
        assert_eq!(
            pool.run_until(result).unwrap().unwrap().reason,
            FinishReason::Complete
        );
    }
    assert_eq!(events_b.borrow().last().unwrap().content, "B!");
    assert_eq!(events_a.borrow().len(), 2);
    assert_eq!(events_b.borrow().len(), 2);
    drop((observe_a, observe_b));
    pool.run_until(host.close()).unwrap();
    assert_eq!(host.kernel().diagnostics().resources, 0);
}

#[test]
fn scripted_streaming_uses_the_native_model_contract_without_tasks_or_history() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let model = Rc::new(MockModel::new([Step::Stream {
        chunks: vec!["hel".into(), "lo".into()],
        response: ModelResponse {
            message: AssistantMessage {
                content: "hello".into(),
                tool_calls: vec![],
            },
            usage: Usage::default(),
        },
    }]));
    host.install(vec![mock::orb("fixture.mock", model.clone())])
        .unwrap();
    let binding = host.clone();
    host.install(vec![orb(move |d| {
        Ok(Agent::new(
            Rc::new(ModelService::bind(
                &d,
                binding.clone(),
                "fixture.mock",
                vec!["main".into()],
            )?),
            Rc::new(scenario::Scenario::default()),
        )
        .with_progress(Progress::bind(&d)?))
    })])
    .unwrap();
    let reference = Rc::new(host.get(SERVICE, vec!["main".into()], "alice").unwrap());
    pool.run_until(reference.ready()).unwrap();
    let events = Rc::new(RefCell::new(vec![]));
    let collect = events.clone();
    let observation = reference
        .state::<Option<ModelProgress>>()
        .unwrap()
        .subscribe(move |value| {
            if let Some(value) = value.as_ref() {
                collect.borrow_mut().push(value.content.clone());
            }
            Ok(())
        })
        .unwrap();
    let mut request = input::request();
    request.tools.clear();
    let output = pool
        .run_until(host.root("alice", move |ctx| {
            async move {
                reference
                    .call::<_, ExecutionResult>(&ctx, Method::Execute.as_str(), request)
                    .await
            }
            .boxed_local()
        }))
        .unwrap()
        .unwrap();
    assert_eq!(events.borrow().as_slice(), ["", "hel", "hello"]);
    assert!(matches!(&output.messages[0], Message::Assistant(m) if m.content == "hello"));
    assert_eq!(model.calls().len(), 1);
    assert_eq!(model.remaining(), 0);
    drop(observation);
    pool.run_until(host.close()).unwrap();
    assert_eq!(host.kernel().diagnostics().resources, 0);
}
