//! Context and command views of the same native conversation owner.
use super::{SERVICE, failure, types::Snapshot as ConversationSnapshot};
use futures::{FutureExt, task::LocalSpawnExt};
use swarmie_context::{CompactionRequest, Receipt, Snapshot};
use tembra::{Context, Host, Orb, Reference, Service, State};

struct View {
    reference: Reference,
    observation: tembra::Observation,
}

pub(super) fn commands(host: Host, principal: String) -> Orb {
    swarmie_commands::orb(host, move |declaration| {
        let address = declaration.address()?;
        if address.principal != principal || address.key.len() != 1 {
            return Err(failure("context command access refused"));
        }
        let catalog = swarmie_commands::Catalog::default();
        catalog.register_owned(declaration, swarmie_context::compact_command(address.key)?)?;
        Ok(catalog)
    })
}

fn publish(
    state: &State<Snapshot>,
    context: &Context,
    snapshot: Snapshot,
) -> tembra::Result<Snapshot> {
    snapshot.validate()?;
    if *state.value()? != snapshot {
        state.update(context, |_| Ok(snapshot.clone()))?;
    }
    Ok(snapshot)
}

pub(super) fn orb(host: Host, principal: String) -> Orb {
    let service = Service::new(swarmie_context::SERVICE, move |declaration| {
        let address = declaration.address()?;
        if address.principal != principal || address.key.len() != 1 {
            return Err(failure("context observation access refused"));
        }
        let state = declaration.state(Snapshot::default(), 1, Snapshot::validate)?;
        let host = host.clone();
        let resource = declaration.resource_async(
            "context.conversation",
            1,
            &[],
            move |signal| {
                async move {
                    signal.check()?;
                    let reference = host.get(SERVICE, address.key.clone(), &address.principal)?;
                    reference.ready().await?;
                    let selected = reference
                        .state::<ConversationSnapshot>()?
                        .select(|value| Ok(value.context.clone()));
                    let observe_host = host.clone();
                    let observation = selected.subscribe(move |_| {
                        let host = observe_host.clone();
                        let key = address.key.clone();
                        let principal = address.principal.clone();
                        super::tasks::Executor
                            .spawn_local(async move {
                                let Ok(view) = host.get(swarmie_context::SERVICE, key, &principal)
                                else {
                                    return;
                                };
                                let _ = host
                                    .root(&principal, move |context| {
                                        async move {
                                            view.call::<_, ()>(&context, "synchronize", ()).await
                                        }
                                        .boxed_local()
                                    })
                                    .await;
                            })
                            .map_err(|_| failure("context observation executor is unavailable"))?;
                        Ok(())
                    })?;
                    Ok(View {
                        reference,
                        observation,
                    })
                }
                .boxed_local()
            },
            |view, _| {
                async move {
                    view.observation.detach();
                    view.reference.release()
                }
                .boxed_local()
            },
        )?;
        Ok((resource, state))
    })
    .method("synchronize", |env, context, (): ()| async move {
        let snapshot = env
            .0
            .value()?
            .reference
            .state::<ConversationSnapshot>()?
            .value()?
            .context
            .clone();
        publish(&env.1, &context, snapshot)?;
        Ok(())
    })
    .method("inspect", |env, context, (): ()| async move {
        let snapshot: Snapshot = env
            .0
            .value()?
            .reference
            .call(&context, "context_inspect", ())
            .await?;
        publish(&env.1, &context, snapshot)
    })
    .method(
        "compact",
        |env, context, (request,): (CompactionRequest,)| async move {
            let receipt: Receipt = env
                .0
                .value()?
                .reference
                .call(&context, "context_compact", (request,))
                .await?;
            publish(&env.1, &context, receipt.snapshot.clone())?;
            Ok(receipt)
        },
    )
    .method(
        "stop",
        |env, context, (operation,): (swarmie_providers::OperationId,)| async move {
            let receipt: Receipt = env
                .0
                .value()?
                .reference
                .call(&context, "context_stop", (operation,))
                .await?;
            publish(&env.1, &context, receipt.snapshot.clone())?;
            Ok(receipt)
        },
    )
    .method(
        "command",
        |env, context, (invocation,): (swarmie_commands::Invocation,)| async move {
            if invocation.command.as_str() != swarmie_commands::COMPACT {
                return Err(failure("context command identity is invalid"));
            }
            let args: swarmie_context::CompactArguments =
                serde_json::from_value(invocation.arguments)
                    .map_err(|_| failure("context command arguments are invalid"))?;
            let request = CompactionRequest {
                operation: invocation.operation,
                fence: args.fence,
            };
            let receipt: Receipt = env
                .0
                .value()?
                .reference
                .call(&context, "context_compact", (request,))
                .await?;
            publish(&env.1, &context, receipt.snapshot.clone())?;
            serde_json::to_value(receipt).map_err(|_| failure("context receipt cannot be encoded"))
        },
    );
    Orb::new(swarmie_context::SERVICE, 1).service(service)
}
