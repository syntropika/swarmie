use futures::{FutureExt, executor::LocalPool};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use swarmie_tools::{
    hooks::{FAILURE_METHOD, Failure, Finished, Hooks, Outcome, Phase},
    *,
};
use tembra::{Code, Host, Orb, Reference, core::Requirement};

const SERVICE_NAME: &str = "fixture.echo";

#[derive(Clone, Default)]
struct Echo(Rc<Cell<usize>>);

impl Tool for Echo {
    const NAME: &'static str = "echo";
    type Args = Value;
    type Output = Value;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Echo arguments".into()
    }
    fn parameters(&self) -> Value {
        json!({"type":"object"})
    }
    async fn call(&self, ctx: &mut ToolContext, args: Value) -> Result<Value, Self::Error> {
        self.0.set(self.0.get() + 1);
        if args["cancel"] == true {
            ctx.execution()?
                .cancel()
                .map_err(ToolExecutionError::from_error)?;
        }
        ctx.insert_result(FilesChanged(vec![FileChange::modified("/work/a", 1, 0)]))?;
        Ok(args)
    }
}

fn call(arguments: Value) -> ToolCall {
    ToolCall {
        id: ToolCallId::new("echo-call").unwrap(),
        name: ToolName::new("echo").unwrap(),
        arguments,
    }
}

fn extension(name: &str) -> Orb {
    Orb::new(name, 1).requires(Requirement {
        orb: SERVICE_NAME.into(),
        api_version: 1,
        methods: Default::default(),
        hooks: ["before_call".into(), "after_call".into()].into(),
    })
}

struct Fixture {
    pool: LocalPool,
    host: Host,
    reference: Option<Rc<Reference>>,
    effects: Rc<Cell<usize>>,
}

impl Fixture {
    fn new(extensions: Vec<Orb>) -> Self {
        let pool = LocalPool::new();
        let host = Host::new(pool.spawner());
        let echo = Echo::default();
        let effects = echo.0.clone();
        let mut orbs = vec![tool_orb(SERVICE_NAME, move |_| {
            ToolHandler::new(echo.clone())
        })];
        orbs.extend(extensions);
        host.install(orbs).unwrap();
        let reference = Some(Rc::new(
            host.get(SERVICE_NAME, vec!["main".into()], "alice")
                .unwrap(),
        ));
        Self {
            pool,
            host,
            reference,
            effects,
        }
    }

    fn invoke<A: 'static, R: 'static>(&mut self, method: &str, args: A) -> tembra::Result<R> {
        let reference = self.reference.as_ref().unwrap().clone();
        let method = method.to_owned();
        self.pool.run_until(self.host.root("alice", move |ctx| {
            async move { reference.call(&ctx, &method, args).await }.boxed_local()
        }))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.reference.take().unwrap().release().unwrap();
        self.pool.run_until(self.host.close()).unwrap();
        assert_eq!(self.host.kernel().diagnostics().resources, 0);
        assert_eq!(self.host.kernel().diagnostics().calls, 0);
        assert_eq!(self.host.kernel().diagnostics().roots, 0);
    }
}

#[test]
fn a_block_stops_later_contributions_and_cannot_start_the_tool() {
    let hooks = Hooks::default();
    let mut fixture = Fixture::new(vec![
        extension("guard")
            .contribute(
                SERVICE_NAME,
                &hooks.before_call,
                0,
                |_, mut input| async move {
                    input.decision = ToolDecision::Block("declined".into());
                    Ok(input)
                },
            )
            .contribute(SERVICE_NAME, &hooks.before_call, 1, |_, _| async {
                panic!("block must stop composition")
            }),
    ]);
    let result: ToolResult = fixture
        .invoke(
            ToolMethod::Execute.as_str(),
            (OperationId::new(), call(json!({}))),
        )
        .unwrap();
    assert_eq!(result, ToolResult::Blocked("declined".into()));
    assert_eq!(fixture.effects.get(), 0);
}

#[test]
fn hooks_cannot_rewrite_the_confirmed_arguments_or_operation() {
    for change_operation in [false, true] {
        let hooks = Hooks::default();
        let mut fixture = Fixture::new(vec![extension("invalid").contribute(
            SERVICE_NAME,
            &hooks.before_call,
            0,
            move |_, mut input| async move {
                if change_operation {
                    input.operation = Some(OperationId::new());
                } else {
                    input.call.arguments = json!({"rewritten":true});
                }
                Ok(input)
            },
        )]);
        let result: tembra::Result<ToolResult> = fixture.invoke(
            ToolMethod::Execute.as_str(),
            (OperationId::new(), call(json!({}))),
        );
        assert!(result.is_err());
        assert_eq!(fixture.effects.get(), 0);
    }
}

#[test]
fn physical_observers_receive_facts_and_failures_do_not_replace_success() {
    let hooks = Hooks::default();
    let observed = Rc::new(RefCell::new(Vec::<Finished>::new()));
    let output = observed.clone();
    let mut fixture = Fixture::new(vec![extension("observer").contribute(
        SERVICE_NAME,
        &hooks.after_call,
        0,
        move |ctx, event| {
            let output = output.clone();
            async move {
                assert_eq!(ctx.principal()?, "alice");
                assert_ne!(ctx.root, event.source.root);
                assert_eq!(ctx.frame(), 0);
                output.borrow_mut().push(event);
                Err(tembra::Error::new(
                    Code::Unavailable,
                    "fixture.observer",
                    "observer failed",
                ))
            }
        },
    )]);
    let operation = OperationId::new();
    let call = call(json!({"value":42}));
    let result: ToolResult = fixture
        .invoke(ToolMethod::Execute.as_str(), (operation, call.clone()))
        .unwrap();
    assert_eq!(result, ToolResult::Success(json!({"value":42})));
    let events = observed.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].operation, operation);
    assert_eq!(events[0].call, call);
    assert_eq!(events[0].outcome, Outcome::Executed(result));
    assert!(events[0].metadata.get::<FilesChanged>().unwrap().is_some());
    assert_eq!(fixture.effects.get(), 1);
    let failure: Option<Failure> = fixture.invoke(FAILURE_METHOD, ()).unwrap();
    assert_eq!(failure.unwrap().reason, "fixture.observer");
}

#[test]
fn preparation_execution_and_recovery_have_distinct_frozen_intents() {
    let hooks = Hooks::default();
    let phases = Rc::new(RefCell::new(Vec::new()));
    let capture = phases.clone();
    let mut fixture = Fixture::new(vec![extension("phases").contribute(
        SERVICE_NAME,
        &hooks.before_call,
        0,
        move |_, input| {
            let capture = capture.clone();
            async move {
                capture.borrow_mut().push((input.phase, input.operation));
                Ok(input)
            }
        },
    )]);
    let call = call(json!({}));
    let operation = OperationId::new();
    let _: ToolDecision = fixture
        .invoke(
            ToolMethod::Validate.as_str(),
            (tool_spec(&Echo::default()).unwrap(), call.clone()),
        )
        .unwrap();
    let _: ToolResult = fixture
        .invoke(ToolMethod::Execute.as_str(), (operation, call.clone()))
        .unwrap();
    let recovery: Recovery<ToolResult> = fixture
        .invoke(ToolMethod::Recover.as_str(), (operation, call))
        .unwrap();
    assert_eq!(recovery, Recovery::Unknown);
    assert_eq!(
        *phases.borrow(),
        vec![
            (Phase::Preparation, None),
            (Phase::Execution, Some(operation)),
            (Phase::Recovery, Some(operation))
        ]
    );
    assert_eq!(fixture.effects.get(), 1);
}

#[test]
fn a_cancelled_source_keeps_its_confirmed_outcome_and_original_identity() {
    let hooks = Hooks::default();
    let events = Rc::new(RefCell::new(Vec::new()));
    let record = events.clone();
    let mut fixture = Fixture::new(vec![extension("cancelled-observer").contribute(
        SERVICE_NAME,
        &hooks.after_call,
        0,
        move |ctx, event| {
            let record = record.clone();
            async move {
                assert_eq!(ctx.principal()?, "alice");
                record.borrow_mut().push(event);
                Ok(())
            }
        },
    )]);
    let call = call(json!({"cancel":true}));
    let operation = OperationId::new();
    let result: tembra::Result<ToolResult> =
        fixture.invoke(ToolMethod::Execute.as_str(), (operation, call.clone()));
    match result {
        Ok(result) => assert_eq!(result, ToolResult::Success(json!({"cancel":true}))),
        Err(error) => assert_eq!(error.code, Code::Cancelled),
    }
    fixture.pool.run_until_stalled();
    let events = events.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].operation, operation);
    assert_eq!(events[0].call, call);
    assert!(events[0].source.cancelled);
    assert_eq!(
        events[0].outcome,
        Outcome::Executed(ToolResult::Success(json!({"cancel":true})))
    );
    assert_eq!(fixture.effects.get(), 1);
}
