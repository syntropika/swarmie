use futures::{FutureExt, executor::LocalPool, future::LocalBoxFuture};
use std::{cell::Cell, num::NonZeroU32, rc::Rc};
use swarmie_agent_loop::{
    Agent, AgentContext, AssistantMessage, Message, Model, ModelRequest, ModelResponse,
    OperationId, Phase, Recovery, Request, Tools, Usage,
};
use swarmie_providers::{PreparationPolicy, PreparedDescription, PreparedModelRequest};
use tembra::{Code, Context, Host};

struct Fixture {
    prepared: Cell<u32>,
    executed: Cell<u32>,
    tag: Cell<u8>,
    metadata_prepared_at: Cell<u64>,
    lose_reply: Cell<bool>,
    recovered: Cell<bool>,
    safe_retry: Cell<bool>,
}

fn response() -> ModelResponse {
    ModelResponse {
        protection: swarmie_providers::SubmissionProtection::Clean,
        message: AssistantMessage {
            provider: None,
            content: "confirmed".into(),
            tool_calls: vec![],
        },
        usage: Usage::default(),
    }
}

impl Model for Fixture {
    fn recover_prepared<'a>(
        &'a self,
        _: &'a Context,
        _: &'a PreparedDescription,
    ) -> LocalBoxFuture<'a, tembra::Result<Recovery<ModelResponse>>> {
        async move {
            Ok(if self.recovered.get() {
                Recovery::Completed(response())
            } else if self.safe_retry.get() {
                Recovery::SafeToRetry
            } else {
                Recovery::Unknown
            })
        }
        .boxed_local()
    }

    fn complete<'a>(
        &'a self,
        _: &'a Context,
        _: OperationId,
        _: &'a ModelRequest,
    ) -> LocalBoxFuture<'a, tembra::Result<ModelResponse>> {
        async {
            Err(tembra::Error::new(
                Code::Invalid,
                "fixture.bypass",
                "direct execution bypassed preparation",
            ))
        }
        .boxed_local()
    }

    fn prepare<'a>(
        &'a self,
        context: &'a Context,
        operation: OperationId,
        _: ModelRequest,
        _: Option<swarmie_providers::Progress>,
    ) -> LocalBoxFuture<'a, tembra::Result<PreparedModelRequest<'a>>> {
        async move {
            self.prepared.set(self.prepared.get()+1);
            let description:PreparedDescription=serde_json::from_value(serde_json::json!({
                "operation":operation,"fingerprint":vec![self.tag.get();32],"streaming":false,"output_reserve":20,"output_reservation":{"kind":"sent_cap"},
                "model":{"identity":{"provider":"fixture","adapter":"one","endpoint":"https://provider.example/","deployment":null,"model":"one","revision":null},"generation":"a".repeat(64),
                    "metadata_freshness":{"prepared_at_ms":self.metadata_prepared_at.get().to_string(),"valid_until_ms":null,"stale_limit_evidence":0},"native_output_maximum":{"status":"unknown","reason":"missing"},
                    "context":{"tokens":100,"precision":"reported","claims":[],"bounded_by_context":false},"input":{"tokens":100,"precision":"reported","claims":[],"bounded_by_context":true},"output":{"tokens":20,"precision":"reported","claims":[],"bounded_by_context":false},"native_compaction":{"status":"unknown","reason":"missing"}},
                "measurement":{"input_tokens":10,"final_payload_bytes":40,"precision":"estimated","method":"fixture","components":[{"kind":"conversation","tokens":10,"bytes":40}]}
            })).unwrap();
            PreparedModelRequest::new(description,move || async move {
                context.check()?;
                self.executed.set(self.executed.get()+1);
                if self.lose_reply.get() { Err(tembra::Error::new(Code::Unavailable,"fixture.lost","outcome unknown")) } else { Ok(response()) }
            }.boxed_local())
        }.boxed_local()
    }

    fn recover<'a>(
        &'a self,
        _: &'a Context,
        _: OperationId,
        _: &'a ModelRequest,
    ) -> LocalBoxFuture<'a, tembra::Result<Recovery<ModelResponse>>> {
        async move {
            Ok(if self.recovered.get() {
                Recovery::Completed(response())
            } else if self.safe_retry.get() {
                Recovery::SafeToRetry
            } else {
                Recovery::Unknown
            })
        }
        .boxed_local()
    }
}

struct Policy {
    refuse: bool,
    observations: Cell<u32>,
}

impl PreparationPolicy for Policy {
    fn check(
        &self,
        _: &Context,
        prepared: &PreparedDescription,
        _: Option<&PreparedDescription>,
    ) -> tembra::Result<()> {
        prepared.validate()?;
        self.observations.set(self.observations.get() + 1);
        if self.refuse {
            Err(tembra::Error::new(
                Code::Exhausted,
                "fixture.compaction_required",
                "budget unavailable",
            ))
        } else {
            Ok(())
        }
    }
}

fn fixture() -> Rc<Fixture> {
    Rc::new(Fixture {
        prepared: Cell::new(0),
        executed: Cell::new(0),
        tag: Cell::new(1),
        metadata_prepared_at: Cell::new(0),
        lose_reply: Cell::new(false),
        recovered: Cell::new(false),
        safe_retry: Cell::new(false),
    })
}

fn request() -> Request {
    Request {
        context: AgentContext {
            sources: None,
            instructions: "current".into(),
            messages: vec![Message::User {
                content: "preserve pending input".into(),
            }],
        },
        tools: vec![],
        max_turns: NonZeroU32::MIN,
    }
}

struct NoTools;

impl Tools for NoTools {
    fn validate<'a>(
        &'a self,
        _: &'a Context,
        _: &'a swarmie_agent_loop::ToolSpec,
        _: &'a swarmie_agent_loop::ToolCall,
    ) -> LocalBoxFuture<'a, tembra::Result<swarmie_agent_loop::ToolDecision>> {
        async {
            Err(tembra::Error::new(
                Code::Invalid,
                "fixture.tools",
                "unexpected tool validation",
            ))
        }
        .boxed_local()
    }

    fn execute<'a>(
        &'a self,
        _: &'a Context,
        _: swarmie_agent_loop::ToolExecution<'a>,
    ) -> LocalBoxFuture<'a, tembra::Result<swarmie_agent_loop::ToolResult>> {
        async {
            Err(tembra::Error::new(
                Code::Invalid,
                "fixture.tools",
                "unexpected tool execution",
            ))
        }
        .boxed_local()
    }
}

fn tools() -> Rc<dyn Tools> {
    Rc::new(NoTools)
}

#[test]
fn refusal_stays_before_model_intent_and_allowed_work_executes_one_preparation() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let model = fixture();
    let observed = model.clone();
    let policy = Rc::new(Policy {
        refuse: true,
        observations: Cell::new(0),
    });
    let counts = policy.clone();
    let agent = Agent::new(model, tools()).with_preparation(policy);
    let failure = pool
        .run_until(host.root("alice", move |context| {
            async move { Ok(agent.execute(&context, request()).await.unwrap_err()) }.boxed_local()
        }))
        .unwrap();
    assert_eq!(failure.error.code, Code::Exhausted);
    assert_eq!(failure.error.reason, "fixture.compaction_required");
    let state = failure.state.unwrap();
    assert!(matches!(state.phase(), Phase::PrepareTurn));
    assert_eq!(
        (
            observed.prepared.get(),
            observed.executed.get(),
            counts.observations.get()
        ),
        (1, 0, 1)
    );
    let model = fixture();
    let observed = model.clone();
    let policy = Rc::new(Policy {
        refuse: false,
        observations: Cell::new(0),
    });
    let counts = policy.clone();
    let agent = Agent::new(model, tools()).with_preparation(policy);
    pool.run_until(host.root("alice", move |context| {
        async move {
            agent.execute(&context, request()).await.unwrap();
            Ok(())
        }
        .boxed_local()
    }))
    .unwrap();
    assert_eq!(
        (
            observed.prepared.get(),
            observed.executed.get(),
            counts.observations.get()
        ),
        (1, 1, 1)
    );
    pool.run_until(host.close()).unwrap();
}

#[test]
fn restored_unknown_never_reprepares_and_safe_retry_rejects_changed_description() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let model = fixture();
    model.lose_reply.set(true);
    let observed = model.clone();
    let agent = Agent::new(model, tools()).with_preparation(Rc::new(Policy {
        refuse: false,
        observations: Cell::new(0),
    }));
    let state = pool
        .run_until(host.root("alice", {
            let agent = agent.clone();
            move |context| {
                async move {
                    Ok(*agent
                        .execute(&context, request())
                        .await
                        .unwrap_err()
                        .state
                        .unwrap())
                }
                .boxed_local()
            }
        }))
        .unwrap();
    let state: swarmie_agent_loop::State =
        serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap();
    assert!(matches!(
        state.phase(),
        Phase::ModelPending {
            preparation: Some(_),
            ..
        }
    ));
    let failure = pool
        .run_until(host.root("alice", {
            let agent = agent.clone();
            move |context| {
                async move { Ok(agent.resume(&context, state).await.unwrap_err()) }.boxed_local()
            }
        }))
        .unwrap();
    assert_eq!((observed.prepared.get(), observed.executed.get()), (1, 1));
    observed.safe_retry.set(true);
    observed.metadata_prepared_at.set(1);
    let state = *failure.state.unwrap();
    let failure = pool
        .run_until(host.root("alice", {
            let agent = agent.clone();
            move |context| {
                async move { Ok(agent.resume(&context, state).await.unwrap_err()) }.boxed_local()
            }
        }))
        .unwrap();
    assert_eq!((observed.prepared.get(), observed.executed.get()), (2, 1));
    assert_eq!(failure.error.reason, "swarmie.agent_loop.recovery_required");
    observed.metadata_prepared_at.set(0);
    observed.tag.set(2);
    let state = *failure.state.unwrap();
    let failure = pool
        .run_until(host.root("alice", {
            let agent = agent.clone();
            move |context| {
                async move { Ok(agent.resume(&context, state).await.unwrap_err()) }.boxed_local()
            }
        }))
        .unwrap();
    assert_eq!((observed.prepared.get(), observed.executed.get()), (3, 1));
    observed.recovered.set(true);
    let state = *failure.state.unwrap();
    pool.run_until(host.root("alice", move |context| {
        async move {
            agent.resume(&context, state).await.unwrap();
            Ok(())
        }
        .boxed_local()
    }))
    .unwrap();
    assert_eq!((observed.prepared.get(), observed.executed.get()), (3, 1));
    pool.run_until(host.close()).unwrap();
}
