use crate::{
    Configuration, Credentials, EnvironmentCredentials, FACET_ID, GENERATION_CONTRACT, PLUGIN_ID,
    PROVIDER_CONTRACT,
    backend::Backend,
    conversion,
    error::{failure, map},
    generation::{Generation, GenerationResources, GenerationService, GenerationWorker, Wake},
};
use futures::{
    FutureExt,
    future::{Either, select},
};
use std::{cell::RefCell, rc::Rc, time::Duration};
use swarmie_model_contracts::*;
use tembra::{core, prelude::*};
use tracing::instrument::WithSubscriber;

/// Prepares a compiled provider with private credential resolution. Installation opens one
/// managed Resource of connection clients. Retirement closes referenced activities before
/// dropping clients. Native configuration changes require a fresh Host.
///
/// # Errors
/// Refuses invalid configuration synchronously. Credential/client initialization errors arrive
/// through installation readiness, with static diagnostics that contain no provider payloads.
pub fn facet(configuration: &Configuration, driver: Rc<dyn core::Driver>) -> Result<NativeFacet> {
    facet_with_credentials(configuration, driver, Rc::new(EnvironmentCredentials))
}

pub fn facet_with_credentials(
    configuration: &Configuration,
    driver: Rc<dyn core::Driver>,
    credentials: Rc<dyn Credentials>,
) -> Result<NativeFacet> {
    configuration.validate()?;
    let configuration = configuration.clone();
    NativeFacet::new(FACET_ID, vec![], move |scope, setup| {
        let resource: Rc<RefCell<Option<Resource<Backend>>>> = Rc::new(RefCell::new(None));
        let writing = resource.clone();
        let config = configuration.clone();
        let secrets = credentials.clone();
        let provider_driver = driver.clone();
        let provider_configuration = configuration.clone();
        setup.serve_with_options(
            ModelEndpoint::new(PROVIDER_CONTRACT)?,
            move |_, _, declaration| {
                let config = config.clone();
                let secrets = secrets.clone();
                let clients = declaration.core_mut().resource(
                    "connections",
                    ResourceDeclaration::new(move |_| {
                        let config = config.clone();
                        let secrets = secrets.clone();
                        async move { Backend::open(&config, secrets.as_ref()) }.boxed_local()
                    }),
                )?;
                *writing.borrow_mut() = Some(clients.clone());
                declaration.core_mut().register_method(
                    &model_providers()?,
                    PLUGIN_ID,
                    serde_json::to_value(ProviderRegistration {
                        provider: PLUGIN_ID.into(),
                        version: 1,
                        provider_contract: PROVIDER_CONTRACT.into(),
                        generation_contract: GENERATION_CONTRACT.into(),
                        connections: configuration_ids(&provider_configuration),
                    })
                    .map_err(|_| crate::configuration::invalid())?,
                    "generate",
                )?;
                Ok(ProviderService {
                    resource: clients,
                    limits: provider_configuration.limits.clone(),
                    driver: provider_driver.clone(),
                })
            },
            core::ServeOptions {
                lifetime: core::Lifetime::Host,
                ..Default::default()
            },
        )?;
        let limits = configuration.limits.clone();
        let generation_scope = scope.clone();
        let driver = driver.clone();
        let progress_definition = setup.state("progress")?;
        setup.serve(
            GenerationEndpoint::new(GENERATION_CONTRACT)?,
            move |input, _, declaration| {
                let provider = declaration.get(
                    ModelEndpoint::new(PROVIDER_CONTRACT)?,
                    ModelProviderSchemaInput {},
                )?;
                let backend_resource = resource.borrow().as_ref().cloned().ok_or_else(|| {
                    RuntimeError::unavailable(
                        "model_not_ready",
                        "provider Resource is not declared",
                    )
                })?;
                let state = declaration
                    .state(progress_definition.value(Generation::initial(&input.request))?)?;
                let generation = Rc::new(Generation {
                    state,
                    request: input.request,
                    admission: RefCell::new(None),
                    limits: limits.clone(),
                    driver: driver.clone(),
                    wake: Wake::default(),
                    cancellation: core::CancellationToken::new(),
                    completion: RefCell::new(None),
                });
                let activity = generation.clone();
                let worker_scope = generation_scope.clone();
                let mut opening = ResourceDeclaration::new(move |_| {
                    let backend_resource = backend_resource.clone();
                    let generation = activity.clone();
                    let worker_scope = worker_scope.clone();
                    async move {
                        let backend = backend_resource.value()?;
                        let connection =
                            backend.connection(&generation.request.model).map_err(|_| {
                                RuntimeError::invalid("model_request", "unknown configured model")
                            })?;
                        conversion::request(&generation.request, &connection, &generation.limits)
                            .map_err(|_| {
                            RuntimeError::invalid(
                                "model_request",
                                "generation request refused before submission",
                            )
                        })?;
                        if backend.generations.get() >= generation.limits.max_generations {
                            return Err(RuntimeError::unavailable(
                                "model_capacity",
                                "generation object limit reached",
                            ));
                        }
                        backend.generations.set(backend.generations.get() + 1);
                        *generation.admission.borrow_mut() = Some(Rc::new(GenerationResources {
                            backend,
                            connection,
                        }));
                        let stop = core::CancellationToken::new();
                        let stopped = stop.clone();
                        let activity = generation.clone();
                        let (sender, receiver) = futures::channel::oneshot::channel();
                        generation.driver.spawn(
                            async move {
                                let work = activity
                                    .work(worker_scope)
                                    .with_subscriber(tracing::subscriber::NoSubscriber::default())
                                    .boxed_local();
                                let result = match select(work, stopped.cancelled()).await {
                                    Either::Left((result, _)) => result,
                                    Either::Right((_, remaining)) => {
                                        drop(remaining);
                                        Ok(())
                                    }
                                };
                                let _ = sender.send(result);
                            }
                            .boxed_local(),
                        );
                        Ok(GenerationWorker {
                            generation,
                            stop,
                            finished: RefCell::new(Some(receiver)),
                        })
                    }
                    .boxed_local()
                })
                .with_close(|worker| async move { worker.close().await }.boxed_local());
                opening
                    .deps
                    .push(core::ResourceDependency::Reference(provider.core().clone()));
                let worker = declaration
                    .core_mut()
                    .resource("generation-worker", opening)?;
                Ok(GenerationService {
                    generation,
                    _provider: provider,
                    _worker: worker,
                })
            },
        )
    })
}

fn configuration_ids(configuration: &Configuration) -> Vec<String> {
    configuration
        .connections
        .iter()
        .map(|c| c.id.clone())
        .collect()
}

struct ProviderService {
    resource: Resource<Backend>,
    limits: crate::Limits,
    driver: Rc<dyn core::Driver>,
}
impl ModelProviderSchemaProvider for ProviderService {
    async fn capabilities(&self, model: ModelIdentity, _context: Context) -> Result<Capabilities> {
        self.resource
            .value()?
            .connection(&model)
            .map(|c| c.capabilities())
            .map_err(|_| RuntimeError::invalid("model_identity", "unknown configured model"))
    }
    async fn models(&self, connection: String, context: Context) -> Result<ModelList> {
        let backend = self.resource.value()?;
        let Some(c) = backend.connections.get(&connection) else {
            return Ok(ModelList::Error {
                error: failure(ProviderErrorKind::InvalidRequest, Submission::NotSent),
            });
        };
        let _permit = match c.permit() {
            Ok(p) => p,
            Err(error) => return Ok(ModelList::Error { error }),
        };
        let task = c
            .client
            .all_model_names(c.kind, c.discovery.clone())
            .with_subscriber(tracing::subscriber::NoSubscriber::default())
            .boxed_local();
        let timer = self
            .driver
            .sleep(Duration::from_millis(c.configuration.timeout_ms));
        let cancelled = context.cancellation().cancelled();
        let result = match select(task, select(cancelled, timer)).await {
            Either::Left((result, _)) => result.map_err(map).and_then(|models| {
                if models.len() > 1024 {
                    return Err(failure(
                        ProviderErrorKind::PayloadLimit,
                        Submission::ResponseReceived,
                    ));
                }
                let models = models
                    .into_iter()
                    .map(|model| ModelIdentity {
                        connection: connection.clone(),
                        model: model.to_string(),
                    })
                    .collect::<Vec<_>>();
                if conversion::bytes(&models) > self.limits.response_bytes {
                    return Err(failure(
                        ProviderErrorKind::PayloadLimit,
                        Submission::ResponseReceived,
                    ));
                }
                Ok(models)
            }),
            Either::Right((Either::Left(_), remaining)) => {
                drop(remaining);
                Err(failure(
                    ProviderErrorKind::Cancelled,
                    Submission::MayHaveBeenSent,
                ))
            }
            Either::Right((Either::Right(_), remaining)) => {
                drop(remaining);
                Err(failure(
                    ProviderErrorKind::Timeout,
                    Submission::MayHaveBeenSent,
                ))
            }
        };
        Ok(match result {
            Ok(models) => ModelList::Models { models },
            Err(error) => ModelList::Error { error },
        })
    }
    async fn generate(&self, request: ModelRequest, context: Context) -> Result<GenerateResult> {
        let backend = self.resource.value()?;
        let c = match backend.connection(&request.model) {
            Ok(c) => c,
            Err(error) => return Ok(GenerateResult::Error { error }),
        };
        let (chat, options) = match conversion::request(&request, &c, &self.limits) {
            Ok(v) => v,
            Err(error) => return Ok(GenerateResult::Error { error }),
        };
        let _permit = match c.permit() {
            Ok(p) => p,
            Err(error) => return Ok(GenerateResult::Error { error }),
        };
        let inference = c
            .client
            .exec_chat(&request.model.model, chat, Some(&options))
            .with_subscriber(tracing::subscriber::NoSubscriber::default())
            .boxed_local();
        let timer = self
            .driver
            .sleep(Duration::from_millis(c.configuration.timeout_ms));
        let cancelled = context.cancellation().cancelled();
        let result = match select(inference, select(cancelled, timer)).await {
            Either::Left((result, _)) => result
                .map_err(map)
                .and_then(|r| conversion::response(&request, &c, r.into(), &self.limits)),
            Either::Right((Either::Left(_), remaining)) => {
                drop(remaining);
                Err(failure(
                    ProviderErrorKind::Cancelled,
                    Submission::MayHaveBeenSent,
                ))
            }
            Either::Right((Either::Right(_), remaining)) => {
                drop(remaining);
                Err(failure(
                    ProviderErrorKind::Timeout,
                    Submission::MayHaveBeenSent,
                ))
            }
        };
        Ok(match result {
            Ok(response) => GenerateResult::Response {
                response: Box::new(response),
            },
            Err(error) => GenerateResult::Error { error },
        })
    }
}
