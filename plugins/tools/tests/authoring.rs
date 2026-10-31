use futures::{FutureExt, executor::LocalPool};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    io,
    rc::Rc,
};
use swarmie_tools::*;
use tembra::{Code, Context, Host, Orb, Service};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Args {
    a: u32,
    b: u32,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Label(String);

impl ContextValue for Label {
    const KEY: &'static str = "label";
}

type Receipts = Rc<RefCell<BTreeMap<OperationId, (ToolCall, ToolResult)>>>;

#[derive(Clone, Default)]
struct Add {
    effects: Rc<Cell<usize>>,
    receipts: Receipts,
}

impl Tool for Add {
    const NAME: &'static str = "add";
    type Args = Args;
    type Output = Value;
    type Error = ToolExecutionError;

    fn description(&self) -> String {
        "Add two numbers".into()
    }
    fn parameters(&self) -> Value {
        args_schema::<Args>()
    }

    async fn validate(&self, context: &Context, args: &Args) -> tembra::Result<ToolDecision> {
        if context.principal()? != "alice" {
            return Ok(ToolDecision::Block("not permitted".into()));
        }
        if args.a == 900 {
            context.cancel()?;
        }
        Ok(ToolDecision::Allow)
    }

    async fn call(&self, context: &mut ToolContext, args: Args) -> Result<Value, Self::Error> {
        assert_eq!(context.execution().unwrap().principal().unwrap(), "alice");
        self.effects.set(self.effects.get() + 1);
        context.insert(Label("changed".into()))?;
        context.insert_result(Label("receipt".into()))?;
        match args.a {
            901 => {
                return Err(ToolExecutionError::failed(
                    ToolErrorKind::Other,
                    "known failure",
                ));
            }
            902 => return Err(ToolExecutionError::refused("refused by tool")),
            _ => {}
        }
        let output = json!(args.a + args.b);
        self.receipts.borrow_mut().insert(
            context.operation()?,
            (context.call()?.clone(), ToolResult::Success(output.clone())),
        );
        if args.a == 903 {
            return Err(ToolExecutionError::from_error(io::Error::other(
                "secret upstream token",
            )));
        }
        Ok(output)
    }

    async fn recover(
        &self,
        context: &Context,
        operation: OperationId,
        call: &ToolCall,
    ) -> tembra::Result<Recovery<ToolResult>> {
        if context.principal()? != "alice" {
            return Err(tembra::Error::new(
                Code::Forbidden,
                "test.principal",
                "not permitted",
            ));
        }
        Ok(self
            .receipts
            .borrow()
            .get(&operation)
            .filter(|(saved, _)| saved == call)
            .map(|(_, result)| Recovery::Completed(result.clone()))
            .unwrap_or(Recovery::Unknown))
    }
}

fn call(arguments: Value) -> ToolCall {
    ToolCall {
        id: ToolCallId::new("call-1").unwrap(),
        name: ToolName::new("add").unwrap(),
        arguments,
    }
}

fn run(arguments: Value) -> (tembra::Result<ToolResult>, usize) {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let add = Add::default();
    let effects = add.effects.clone();
    let tool = ToolHandler::new(add).unwrap();
    let result = pool.run_until(host.root("alice", move |ctx| {
        async move {
            tool.execute(
                &ctx,
                ToolExecution::new(OperationId::new(), &call(arguments)),
            )
            .await
        }
        .boxed_local()
    }));
    pool.run_until(host.close()).unwrap();
    (result, effects.get())
}

#[test]
fn typed_arguments_and_schema_are_checked_before_effects() {
    for arguments in [
        json!({}),
        json!({"a":"1","b":2}),
        json!({"a":1,"b":2,"extra":3}),
        json!({"a":1.0,"b":2}),
    ] {
        let (result, effects) = run(arguments);
        assert!(matches!(result.unwrap(), ToolResult::Blocked(_)));
        assert_eq!(effects, 0);
    }
    assert_eq!(
        run(json!({"a":2,"b":3})),
        (Ok(ToolResult::Success(json!(5))), 1)
    );
}

#[test]
fn cancellation_after_validation_cannot_start_an_effect() {
    let (result, effects) = run(json!({"a":900,"b":1}));
    assert_eq!(result.unwrap_err().code, Code::Cancelled);
    assert_eq!(effects, 0);
}

#[test]
fn confirmed_failure_refusal_and_unknown_outcome_stay_distinct() {
    assert_eq!(
        run(json!({"a":901,"b":0})).0.unwrap(),
        ToolResult::Failed("known failure".into())
    );
    assert_eq!(
        run(json!({"a":902,"b":0})).0.unwrap(),
        ToolResult::Blocked("refused by tool".into())
    );
    let (result, effects) = run(json!({"a":903,"b":0}));
    let error = result.unwrap_err();
    assert_eq!(error.code, Code::Unavailable);
    assert!(!format!("{error:?}").contains("secret upstream token"));
    assert_eq!(effects, 1);
}

#[test]
fn catalog_dispatch_and_saved_provider_recovery_use_the_same_author() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let add = Add::default();
    let spec = tool_spec(&add).unwrap();
    let effects = add.effects.clone();
    let provider = ToolProvider::new("fixture.add", vec!["main".into()]).unwrap();
    let catalog = Catalog::default();
    let lease = catalog
        .register(ToolDefinition::new(spec.clone(), provider.clone()).unwrap())
        .unwrap();
    let host_for_client = host.clone();
    host.install(vec![
        tool_orb("fixture.add", move |declaration| {
            if declaration.address()?.principal != "alice" {
                return Err(tembra::Error::new(
                    Code::Forbidden,
                    "test.principal",
                    "not permitted",
                ));
            }
            ToolHandler::new(add.clone())
        }),
        orb(host.clone(), catalog),
        Orb::new("fixture.client", 1).service(
            Service::new("fixture.client", move |d| {
                CatalogService::bind(
                    &d,
                    host_for_client.clone(),
                    SERVICE,
                    vec![INSTANCE_KEY.into()],
                )
            })
            .method("roundtrip", |tools, ctx, spec: ToolSpec| async move {
                let call = call(json!({"a":903,"b":0}));
                let prepared = tools.prepare(&ctx, &spec, &call).await?;
                assert_eq!(prepared.decision, ToolDecision::Allow);
                let execution = ToolExecution {
                    operation: OperationId::new(),
                    call: &call,
                    provider: prepared.provider.as_ref(),
                };
                assert!(tools.execute(&ctx, execution).await.is_err());
                let recovered = tools.recover(&ctx, execution).await?;
                assert_eq!(
                    recovered,
                    Recovery::Completed(ToolResult::Success(json!(903)))
                );
                let mut different = call.clone();
                different.arguments = json!({"a":1,"b":2});
                assert_eq!(
                    tools
                        .recover(
                            &ctx,
                            ToolExecution {
                                call: &different,
                                ..execution
                            }
                        )
                        .await?,
                    Recovery::Unknown
                );
                Ok(())
            }),
        ),
    ])
    .unwrap();
    assert_eq!(
        host.get("fixture.add", vec!["main".into()], "bob")
            .err()
            .unwrap()
            .code,
        Code::Forbidden
    );
    let reference = host
        .get("fixture.client", vec!["main".into()], "alice")
        .unwrap();
    pool.run_until(host.root("alice", move |ctx| {
        async move {
            reference.call::<_, ()>(&ctx, "roundtrip", spec).await?;
            reference.release()
        }
        .boxed_local()
    }))
    .unwrap();
    assert_eq!(effects.get(), 1);
    drop(lease);
    pool.run_until(host.close()).unwrap();
    assert_eq!(host.kernel().diagnostics().resources, 0);
}

#[test]
fn typed_context_is_isolated_and_serialization_carries_no_authority() {
    let mut pool = LocalPool::new();
    let host = Host::new(pool.spawner());
    let tool = ToolHandler::new(Add::default()).unwrap();
    pool.run_until(host.root("alice", move |ctx| {
        async move {
            let mut data = ToolContext::new();
            data.insert(Label("original".into())).unwrap();
            tool.execute_with(
                &ctx,
                ToolExecution::new(OperationId::new(), &call(json!({"a":1,"b":2}))),
                &mut data,
            )
            .await?;
            assert_eq!(data.require::<Label>().unwrap(), Label("original".into()));
            assert_eq!(
                data.result::<Label>().unwrap(),
                Some(Label("receipt".into()))
            );
            let serialized = serde_json::to_value(&data).unwrap();
            assert!(serialized.get("invocation").is_none());
            assert!(
                serde_json::from_value::<ToolContext>(serialized)
                    .unwrap()
                    .execution()
                    .is_err()
            );
            assert_eq!(
                serde_json::to_value(data.result_context()).unwrap(),
                json!({"label":"receipt"})
            );
            Ok(())
        }
        .boxed_local()
    }))
    .unwrap();
    pool.run_until(host.close()).unwrap();
}

#[test]
fn explicit_outputs_and_safe_diagnostics_keep_their_meaning() {
    let text = "{\"secret\":1}".to_string().into_tool_output().unwrap();
    assert_eq!(text.as_text(), Some("{\"secret\":1}"));
    let explicit = json!("text").into_tool_output().unwrap();
    assert_eq!(explicit.as_json(), Some(&json!("text")));
    assert_eq!(
        ToolOutput::json(json!({"a":1}))
            .into_tool_output()
            .unwrap()
            .as_json(),
        Some(&json!({"a":1}))
    );
    let error = ToolExecutionError::from_error(io::Error::other("private credential"));
    assert!(error.downcast_ref::<io::Error>().is_some());
    assert!(error.message().contains("private credential"));
    assert!(!error.model_feedback().contains("private credential"));
    assert!(!format!("{error:?} {text:?}").contains("secret"));
    assert!(!error.is_confirmed());
}
