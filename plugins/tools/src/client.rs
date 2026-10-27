use crate::{
    Catalog, OperationId, Recovery, Selection, ToolCall, ToolDecision, ToolExecution,
    ToolPreparation, ToolProvider, ToolResult, ToolSpec, Tools, invalid, provider,
    routing::Routing,
};
use futures::{FutureExt, future::LocalBoxFuture};
use tembra::{Context, Declaration, Host, Orb, Reference, Resource, Service};

pub const SERVICE: &str = "swarmie.tools";
pub const INSTANCE_KEY: &str = "main";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogMethod {
    List,
    Prepare,
    PrepareRetry,
    Execute,
    Recover,
}

impl CatalogMethod {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Prepare => "prepare",
            Self::PrepareRetry => "prepare_retry",
            Self::Recover => "recover",
            Self::Execute => "execute",
        }
    }
}

/// Exposes metadata and preparation. Actual execution uses Tembra provider calls.
/// Providers must enforce visibility/permission; catalog membership is not authority.
pub fn orb(host: Host, catalog: Catalog) -> Orb {
    orb_with_catalog(host, move |_| Ok(catalog.clone()))
}

/// Select metadata under the service object's principal. Use separate catalogs
/// for private definitions; rejecting acquisition must not reveal their contents.
/// The ordinary `orb` intentionally exposes one shared catalog to its consumers.
pub fn orb_with_catalog(
    host: Host,
    select: impl Fn(&str) -> tembra::Result<Catalog> + 'static,
) -> Orb {
    let service = Service::new(SERVICE, move |declaration| {
        let catalog = select(&declaration.address()?.principal)?;
        let host = host.clone();
        declaration.resource(
            "tools.claims",
            1,
            move || Ok(Routing::new(host, catalog)),
            |routing, _| async move { routing.close() }.boxed_local(),
        )
    })
    .method(
        CatalogMethod::List.as_str(),
        |env, ctx, (): ()| async move {
            ctx.check()?;
            env.value()?.catalog.snapshot()
        },
    )
    .method(
        CatalogMethod::Prepare.as_str(),
        |env, ctx, (spec, call): (ToolSpec, ToolCall)| async move {
            env.value()?.prepare(&ctx, spec, call, None).await
        },
    )
    .method(
        CatalogMethod::PrepareRetry.as_str(),
        |env, ctx, (spec, call, target): (ToolSpec, ToolCall, ToolProvider)| async move {
            env.value()?.prepare(&ctx, spec, call, Some(target)).await
        },
    )
    .method(
        CatalogMethod::Execute.as_str(),
        |env, ctx, (operation, call, target): (OperationId, ToolCall, ToolProvider)| async move {
            env.value()?.execute(&ctx, operation, call, target).await
        },
    )
    .method(
        CatalogMethod::Recover.as_str(),
        |env, ctx, (operation, call, target): (OperationId, ToolCall, ToolProvider)| async move {
            env.value()?
                .ready_reference(&ctx, &target)
                .await?
                .call::<_, Recovery<ToolResult>>(
                    &ctx,
                    crate::ToolMethod::Recover.as_str(),
                    (operation, call),
                )
                .await
        },
    );
    Orb::new(SERVICE, 1).service(service)
}

/// The loop consumes this through Tools. No manager or alternate dispatcher is needed.
pub struct CatalogService {
    reference: Resource<Reference>,
}

impl CatalogService {
    pub fn bind(
        declaration: &Declaration,
        host: Host,
        service: impl Into<String>,
        key: Vec<String>,
    ) -> tembra::Result<Self> {
        Ok(Self {
            reference: provider::bind(declaration, host, &ToolProvider::new(service, key)?)?,
        })
    }

    pub async fn list(&self, context: &Context) -> tembra::Result<Selection> {
        self.reference
            .value()?
            .call(context, CatalogMethod::List.as_str(), ())
            .await
    }
}

impl Tools for CatalogService {
    fn validate<'a>(
        &'a self,
        context: &'a Context,
        spec: &'a ToolSpec,
        call: &'a ToolCall,
    ) -> LocalBoxFuture<'a, tembra::Result<ToolDecision>> {
        async move { Ok(self.prepare(context, spec, call).await?.decision) }.boxed_local()
    }

    fn prepare<'a>(
        &'a self,
        context: &'a Context,
        spec: &'a ToolSpec,
        call: &'a ToolCall,
    ) -> LocalBoxFuture<'a, tembra::Result<ToolPreparation>> {
        async move {
            self.reference
                .value()?
                .call(
                    context,
                    CatalogMethod::Prepare.as_str(),
                    (spec.clone(), call.clone()),
                )
                .await
        }
        .boxed_local()
    }

    fn prepare_retry<'a>(
        &'a self,
        context: &'a Context,
        spec: &'a ToolSpec,
        execution: ToolExecution<'a>,
    ) -> LocalBoxFuture<'a, tembra::Result<ToolPreparation>> {
        async move {
            let provider = execution
                .provider
                .ok_or_else(|| invalid("routed retry has no confirmed provider"))?;
            self.reference
                .value()?
                .call(
                    context,
                    CatalogMethod::PrepareRetry.as_str(),
                    (spec.clone(), execution.call.clone(), provider.clone()),
                )
                .await
        }
        .boxed_local()
    }

    fn execute<'a>(
        &'a self,
        context: &'a Context,
        execution: ToolExecution<'a>,
    ) -> LocalBoxFuture<'a, tembra::Result<ToolResult>> {
        async move {
            let provider = execution
                .provider
                .ok_or_else(|| invalid("routed execution has no confirmed provider"))?;
            self.reference
                .value()?
                .call(
                    context,
                    CatalogMethod::Execute.as_str(),
                    (
                        execution.operation,
                        execution.call.clone(),
                        provider.clone(),
                    ),
                )
                .await
        }
        .boxed_local()
    }

    fn recover<'a>(
        &'a self,
        context: &'a Context,
        execution: ToolExecution<'a>,
    ) -> LocalBoxFuture<'a, tembra::Result<Recovery<ToolResult>>> {
        async move {
            let provider = execution
                .provider
                .ok_or_else(|| invalid("routed recovery has no confirmed provider"))?;
            self.reference
                .value()?
                .call(
                    context,
                    CatalogMethod::Recover.as_str(),
                    (
                        execution.operation,
                        execution.call.clone(),
                        provider.clone(),
                    ),
                )
                .await
        }
        .boxed_local()
    }
}
