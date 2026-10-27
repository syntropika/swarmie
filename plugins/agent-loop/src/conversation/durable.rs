//! Atomic history and checkpoint confirmation through the existing task authority.
use super::{Checkpoint, History, Request, writer::Writer};
use crate::{Agent, Output, model::invalid};
use std::{cell::RefCell, num::NonZeroU32};
use swarmie_durable_tasks::{Execution, TaskDefinition, TaskMethod};
use tembra::{Context, Declaration, Orb, Service};

pub const SERVICE: &str = "swarmie.agent-loop.conversation.durable";

pub async fn execute(
    history: &History,
    context: &Context,
    agent: &Agent,
    execution: &Execution,
) -> tembra::Result<Output> {
    execution.check(context)?;
    let input: Request = serde_json::from_value(execution.input()).map_err(invalid)?;
    let checkpoint = if let Some(value) = execution.checkpoint() {
        let checkpoint: Checkpoint = serde_json::from_value(value).map_err(invalid)?;
        if checkpoint.input != input {
            return Err(invalid(
                "checkpoint does not belong to this history request",
            ));
        }
        checkpoint
    } else {
        Checkpoint::start(input)
    };
    let (mut state, progress) = history.restore(context, checkpoint).await?;
    let writer = Writer {
        history,
        progress: RefCell::new(progress),
        execution: Some(execution),
    };
    agent
        .drive(context, &mut state, &writer)
        .await
        .map_err(crate::failure::DriveError::for_task)
}

pub fn definition() -> TaskDefinition {
    TaskDefinition::new(SERVICE, NonZeroU32::new(1).unwrap()).expect("static task identity")
}

pub fn orb(create: impl Fn(Declaration) -> tembra::Result<(Agent, History)> + 'static) -> Orb {
    crate::hooks::Hooks::default()
        .declare(Orb::new(SERVICE, 1))
        .service(
            Service::new(SERVICE, move |declaration| {
                let (agent, history) = create(declaration.clone())?;
                Ok((agent.bind_hooks(&declaration)?, history))
            })
            .method(
                TaskMethod::Execute.as_str(),
                |env, context, execution: Execution| async move {
                    serde_json::to_value(execute(&env.1, &context, &env.0, &execution).await?)
                        .map(swarmie_durable_tasks::TaskOutcome::Completed)
                        .map_err(invalid)
                },
            ),
        )
}
