use crate::{Configuration, Credentials, GenaiModel};
use futures::FutureExt;
use std::rc::Rc;
use swarmie_models::{Model, ModelMethod, ModelRequest, OperationId, Progress};
use tembra::{Orb, Service};

pub const SERVICE: &str = "swarmie.models.genai";

/// Installs configured model objects without PluginManager. Each address uses a
/// ModelKey and the consumer's principal. The object's resource owns its client.
pub fn orb(configuration: Configuration, credentials: Rc<dyn Credentials>) -> tembra::Result<Orb> {
    configuration.validate()?;
    Ok(Orb::new(SERVICE, 1).service(
        Service::new(SERVICE, move |declaration| {
            let (connection, model) = configuration.select(&declaration.address()?.key)?;
            let credentials = credentials.clone();
            let progress = Progress::bind(&declaration)?;
            let model = declaration.resource(
                "model.client",
                1,
                move || GenaiModel::new(connection, model, credentials.as_ref()),
                |_, _| async { Ok(()) }.boxed_local(),
            )?;
            Ok((model, progress))
        })
        .method(
            ModelMethod::Complete.as_str(),
            |env, context, (operation, request): (OperationId, ModelRequest)| async move {
                env.0.value()?.complete(&context, operation, &request).await
            },
        )
        .method(
            ModelMethod::Recover.as_str(),
            |env, context, (operation, request): (OperationId, ModelRequest)| async move {
                env.0.value()?.recover(&context, operation, &request).await
            },
        )
        .method(
            ModelMethod::Stream.as_str(),
            |env, context, (operation, request): (OperationId, ModelRequest)| async move {
                env.0
                    .value()?
                    .stream(&context, operation, &request, &env.1)
                    .await
            },
        ),
    ))
}
