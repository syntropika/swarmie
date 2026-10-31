use crate::{AiModel, Configuration, Credentials, ModelProtection};
use futures::FutureExt;
use std::rc::Rc;
use swarmie_providers::{Model, ModelMethod, ModelRequest, OperationId, Progress};
use tembra::{Orb, Service};

pub const SERVICE: &str = "swarmie.models.ai";

/// Installs configured model objects without PluginManager. Each address uses a
/// ModelKey and the consumer's principal. The object's resource owns its client.
pub fn orb(
    configuration: Configuration,
    credentials: Rc<dyn Credentials>,
    protection: ModelProtection,
    metadata: impl Fn(
        &crate::Connection,
        &crate::ModelId,
    ) -> tembra::Result<std::sync::Arc<swarmie_ai::catalog::Snapshot>>
    + 'static,
) -> tembra::Result<Orb> {
    configuration.validate()?;
    let metadata = Rc::new(metadata);
    Ok(Orb::new(SERVICE, 1).service(
        Service::new(SERVICE, move |declaration| {
            let (connection, model) = configuration.select(&declaration.address()?.key)?;
            let credentials = credentials.clone();
            let metadata = metadata.clone();
            let protection = protection.clone();
            let progress = Progress::bind(&declaration)?;
            let model = declaration.resource(
                "model.client",
                1,
                move || {
                    let snapshot = metadata(&connection, &model)?;
                    AiModel::new(connection, model, credentials.clone(), protection)?
                        .with_metadata(snapshot)
                },
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
