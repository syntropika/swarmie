//! Optional adapter. One ordinary task assignment drives the same engine to completion.
use crate::{Agent, Output, Request, State, engine::Confirmation, model::invalid};
use futures::{FutureExt, future::LocalBoxFuture};
use std::{num::NonZeroU32, rc::Rc};
use swarmie_durable_tasks::{Execution, TaskDefinition, TaskMethod};
use tembra::{Context, Declaration, Orb, Service};

pub const SERVICE: &str = "swarmie.agent-loop.durable";

/// Stage caller-owned writes using this supplied Context and await every call.
/// Success only means prepared; the enclosing commit confirms them with progress.
pub type Prepare = Rc<dyn Fn(Context, State) -> LocalBoxFuture<'static, tembra::Result<()>>>;

struct Durable<'a> {
    execution: &'a Execution,
    prepare: Option<Prepare>,
}

impl Confirmation for Durable<'_> {
    fn confirm<'a>(
        &'a self,
        context: &'a Context,
        next: &'a State,
    ) -> LocalBoxFuture<'a, tembra::Result<()>> {
        async move {
            self.execution.check(context)?;
            let checkpoint = serde_json::to_value(next).map_err(invalid)?;
            if let Some(prepare) = self.prepare.clone() {
                let next = next.clone();
                self.execution
                    .commit_with(context, move |writing| {
                        async move {
                            prepare(writing, next).await?;
                            Ok(checkpoint)
                        }
                        .boxed_local()
                    })
                    .await
            } else {
                self.execution.commit(context, checkpoint).await
            }
        }
        .boxed_local()
    }
}

/// Drive an owner-issued execution. Input is a Request; a checkpoint is a State.
/// Uncertain effects remain pending and require provider reconciliation on resume.
pub async fn execute(
    agent: &Agent,
    context: &Context,
    execution: &Execution,
) -> tembra::Result<Output> {
    execute_with(agent, context, execution, None).await
}

/// Adds atomic preparation without coupling the engine to a conversation implementation.
/// The backend must understand every staged command. External effects stay outside commits.
pub async fn execute_with(
    agent: &Agent,
    context: &Context,
    execution: &Execution,
    prepare: Option<Prepare>,
) -> tembra::Result<Output> {
    execution.check(context)?;
    let request = serde_json::from_value::<Request>(execution.input()).map_err(invalid)?;
    let checkpoint = execution.checkpoint();
    let mut state = if let Some(checkpoint) = checkpoint {
        serde_json::from_value::<State>(checkpoint).map_err(invalid)?
    } else {
        State::start(request.clone())?
    };
    if !state.matches_request(&request) {
        return Err(invalid("checkpoint does not belong to this request"));
    }
    state.validate()?;
    agent
        .drive(context, &mut state, &Durable { execution, prepare })
        .await
        .map_err(crate::failure::DriveError::for_task)
}

pub fn definition() -> TaskDefinition {
    TaskDefinition::new(SERVICE, NonZeroU32::new(1).unwrap()).expect("static task identity")
}

/// Optional ready-made task orb. The same Agent can also serve ordinary invocations.
pub fn orb(create: impl Fn(Declaration) -> tembra::Result<Agent> + 'static) -> Orb {
    crate::hooks::Hooks::default()
        .declare(Orb::new(SERVICE, 1))
        .service(
            Service::new(SERVICE, move |declaration| {
                create(declaration.clone())?.bind_hooks(&declaration)
            })
            .method(
                TaskMethod::Execute.as_str(),
                |agent, context, execution: Execution| async move {
                    serde_json::to_value(execute(&agent, &context, &execution).await?)
                        .map(swarmie_durable_tasks::TaskOutcome::Completed)
                        .map_err(invalid)
                },
            ),
        )
}
