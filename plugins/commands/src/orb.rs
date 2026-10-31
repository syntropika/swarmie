use crate::{Catalog, Invocation};
use futures::FutureExt;
use serde_json::Value;
use tembra::{Host, Orb, Reference, Service};

pub const SERVICE: &str = "swarmie.commands";

struct Handler(Reference);

impl Drop for Handler {
    fn drop(&mut self) {
        let _ = self.0.release();
    }
}

/// Select visible command metadata under the authenticated object principal.
/// Handlers must authorize their domain operations independently. A command never
/// acquires a host principal, grants consent or appends a model-bound chat message.
pub fn orb(
    host: Host,
    select: impl Fn(&tembra::Declaration) -> tembra::Result<Catalog> + 'static,
) -> Orb {
    let service = Service::new(SERVICE, move |declaration| {
        let catalog = select(&declaration)?;
        let host = host.clone();
        declaration.resource(
            "commands.catalog",
            1,
            move || Ok((host, catalog)),
            |_, _| async { Ok(()) }.boxed_local(),
        )
    })
    .method("list", |env, context, (): ()| async move {
        context.check()?;
        Ok(env.value()?.1.list())
    })
    .method(
        "invoke",
        |env, context, (invocation,): (Invocation,)| async move {
            context.check()?;
            let resource = env.value()?;
            let command = resource.1.get(&invocation.command)?;
            let handler = Handler(resource.0.get(
                &command.target.service,
                command.target.key,
                &context.principal()?,
            )?);
            handler.0.ready().await?;
            context.check()?;
            handler
                .0
                .call::<_, Value>(&context, &command.target.method, (invocation,))
                .await
        },
    );
    Orb::new(SERVICE, 1).service(service)
}
