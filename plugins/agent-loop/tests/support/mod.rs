mod request;
mod scenario;
pub use request::request;
pub use scenario::Scenario;
use std::rc::Rc;
use swarmie_agent_loop::*;
use tembra::{Host, Orb, Service};

/// Two real Tembra orbs behind the neutral provider interfaces.
pub fn factory(
    host: &Host,
    scenario: Rc<Scenario>,
) -> impl Fn(tembra::Declaration) -> tembra::Result<Agent> + 'static {
    let model = scenario.clone();
    host.install(vec![
        Orb::new("fixture.model", 1).service(
            Service::new("fixture.model", move |_| Ok(model.clone()))
                .method(
                    ModelMethod::Complete.as_str(),
                    |env, ctx, (id, request): (OperationId, ModelRequest)| async move {
                        env.complete(&ctx, id, &request).await
                    },
                )
                .method(
                    ModelMethod::Recover.as_str(),
                    |env, ctx, (id, request): (OperationId, ModelRequest)| async move {
                        Model::recover(env.as_ref().as_ref(), &ctx, id, &request).await
                    },
                ),
        ),
        Orb::new("fixture.tools", 1).service(
            Service::new("fixture.tools", move |_| Ok(scenario.clone()))
                .method(
                    ToolMethod::Validate.as_str(),
                    |env, ctx, (spec, call): (ToolSpec, ToolCall)| async move {
                        env.validate(&ctx, &spec, &call).await
                    },
                )
                .method(
                    ToolMethod::Execute.as_str(),
                    |env, ctx, (id, call): (OperationId, ToolCall)| async move {
                        env.execute(&ctx, ToolExecution::new(id, &call)).await
                    },
                )
                .method(
                    ToolMethod::Recover.as_str(),
                    |env, ctx, (id, call): (OperationId, ToolCall)| async move {
                        Tools::recover(env.as_ref().as_ref(), &ctx, ToolExecution::new(id, &call))
                            .await
                    },
                ),
        ),
    ])
    .unwrap();
    let host = host.clone();
    move |declaration| {
        Ok(Agent::new(
            Rc::new(ModelService::bind(
                &declaration,
                host.clone(),
                "fixture.model",
                vec!["main".into()],
            )?),
            Rc::new(ToolService::bind(
                &declaration,
                host.clone(),
                "fixture.tools",
                vec!["main".into()],
            )?),
        ))
    }
}

pub fn agent(_host: &Host, scenario: Rc<Scenario>) -> Agent {
    Agent::new(scenario.clone(), scenario)
}
