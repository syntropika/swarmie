use futures::{FutureExt, executor::LocalPool};
use serde_json::json;
use std::rc::Rc;
use swarmie_agent_loop::{hooks::*, *};
use tembra::{Host, Orb, Reference, core::Requirement};

pub fn contributor(target: &str) -> Orb {
    Orb::new("fixture.hook-contributor", 1).requires(Requirement {
        orb: target.into(),
        api_version: 1,
        methods: Default::default(),
        hooks: [
            HookName::BeforeTurnAgent,
            HookName::BeforeTurn,
            HookName::PrepareContext,
            HookName::BeforeToolCall,
            HookName::AfterToolCall,
            HookName::AfterTurn,
            HookName::AfterTurnAgent,
        ]
        .map(|name| name.as_str().into())
        .into(),
    })
}

pub fn response(tools: bool) -> ModelResponse {
    ModelResponse {
        message: AssistantMessage {
            content: if tools { "" } else { "Done" }.into(),
            tool_calls: if tools {
                vec![ToolCall {
                    id: ToolCallId::new("call-1").unwrap(),
                    name: ToolName::new("counter.add").unwrap(),
                    arguments: json!({"amount": 99}),
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

pub fn invoke<A: 'static, R: 'static>(
    pool: &mut LocalPool,
    host: &Host,
    reference: &Rc<Reference>,
    method: &str,
    args: A,
) -> R {
    let reference = reference.clone();
    let method = method.to_owned();
    pool.run_until(host.root("alice", move |ctx| {
        async move { reference.call(&ctx, &method, args).await }.boxed_local()
    }))
    .unwrap()
}

pub fn close(pool: &mut LocalPool, host: &Host) {
    pool.run_until(host.close()).unwrap();
    let d = host.kernel().diagnostics();
    assert_eq!((d.roots, d.calls, d.resources), (0, 0, 0));
}
