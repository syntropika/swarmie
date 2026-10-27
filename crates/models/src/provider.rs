use crate::{ModelProgress, ModelRequest, ModelResponse, OperationId, Progress};
use futures::{FutureExt, future::LocalBoxFuture};
use serde::{Deserialize, Serialize};
use tembra::{Context, Declaration, Host, Reference, Resource};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Recovery<T> {
    Completed(T),
    SafeToRetry,
    Unknown,
}

pub trait Model {
    fn complete<'a>(
        &'a self,
        context: &'a Context,
        operation: OperationId,
        request: &'a ModelRequest,
    ) -> LocalBoxFuture<'a, tembra::Result<ModelResponse>>;

    /// Optional previews. Providers without streaming still return a final result.
    /// A preview is never a receipt, checkpoint or executable tool call.
    fn stream<'a>(
        &'a self,
        context: &'a Context,
        operation: OperationId,
        request: &'a ModelRequest,
        progress: &'a Progress,
    ) -> LocalBoxFuture<'a, tembra::Result<ModelResponse>> {
        async move {
            progress.publish(
                context,
                ModelProgress {
                    operation,
                    content: String::new(),
                },
            );
            context.check()?;
            let response = self.complete(context, operation, request).await?;
            progress.publish(
                context,
                ModelProgress {
                    operation,
                    content: response.message.content.clone(),
                },
            );
            Ok(response)
        }
        .boxed_local()
    }

    /// Default refusal prevents an automatic duplicate request after uncertain dispatch.
    fn recover<'a>(
        &'a self,
        _context: &'a Context,
        _operation: OperationId,
        _request: &'a ModelRequest,
    ) -> LocalBoxFuture<'a, tembra::Result<Recovery<ModelResponse>>> {
        async { Ok(Recovery::Unknown) }.boxed_local()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelMethod {
    Complete,
    Stream,
    Recover,
}

impl ModelMethod {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Stream => "stream",
            Self::Recover => "recover",
        }
    }
}

/// An ordinary Tembra reference owned by a managed object resource.
/// No provider registry or second lifecycle is introduced.
pub struct ModelService(Resource<Reference>);

impl ModelService {
    /// Resolves as the declaring object's principal and releases during resource close.
    pub fn bind(
        declaration: &Declaration,
        host: Host,
        service: impl Into<String>,
        key: Vec<String>,
    ) -> tembra::Result<Self> {
        Ok(Self(bind(
            declaration,
            host,
            "agent.model",
            service.into(),
            key,
        )?))
    }
}

impl Model for ModelService {
    fn stream<'a>(
        &'a self,
        context: &'a Context,
        operation: OperationId,
        request: &'a ModelRequest,
        progress: &'a Progress,
    ) -> LocalBoxFuture<'a, tembra::Result<ModelResponse>> {
        async move {
            context.check()?;
            let reference = self.0.value()?;
            reference.ready().await?;
            context.check()?;
            let readable = reference.state::<Option<ModelProgress>>()?;
            let owner = context.clone();
            let progress = progress.clone();
            // State may retain a previous preview. It must not be replayed as
            // progress for a newly dispatched or recovered operation.
            let subscribing = std::cell::Cell::new(true);
            let observation = readable.subscribe(move |value| {
                if !subscribing.replace(false)
                    && let Some(value) =
                        value.as_ref().as_ref().filter(|v| v.operation == operation)
                {
                    progress.publish(&owner, value.clone());
                }
                Ok(())
            })?;
            let result = reference
                .call(
                    context,
                    ModelMethod::Stream.as_str(),
                    (operation, request.clone()),
                )
                .await;
            drop(observation);
            result
        }
        .boxed_local()
    }

    fn complete<'a>(
        &'a self,
        context: &'a Context,
        operation: OperationId,
        request: &'a ModelRequest,
    ) -> LocalBoxFuture<'a, tembra::Result<ModelResponse>> {
        async move {
            self.0
                .value()?
                .call(
                    context,
                    ModelMethod::Complete.as_str(),
                    (operation, request.clone()),
                )
                .await
        }
        .boxed_local()
    }

    fn recover<'a>(
        &'a self,
        context: &'a Context,
        operation: OperationId,
        request: &'a ModelRequest,
    ) -> LocalBoxFuture<'a, tembra::Result<Recovery<ModelResponse>>> {
        async move {
            self.0
                .value()?
                .call(
                    context,
                    ModelMethod::Recover.as_str(),
                    (operation, request.clone()),
                )
                .await
        }
        .boxed_local()
    }
}

fn bind(
    declaration: &Declaration,
    host: Host,
    resource: &str,
    service: String,
    key: Vec<String>,
) -> tembra::Result<Resource<Reference>> {
    let principal = declaration.address()?.principal;
    // JSON tuple encoding distinguishes services, key segments and separators.
    let resource = serde_json::to_string(&(resource, &service, &key)).map_err(crate::invalid)?;
    declaration.resource(
        &resource,
        1,
        move || host.get(&service, key, &principal),
        |reference, _| async move { reference.release() }.boxed_local(),
    )
}
