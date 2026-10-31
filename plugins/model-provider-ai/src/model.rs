mod preparation;
mod protection;

use crate::{
    Connection, Credentials, ModelId, conversion,
    error::{Reason, invalid, upstream},
};
use futures::{FutureExt, future::LocalBoxFuture};
pub use protection::{AdmittedAttempt, ModelCredential, ModelDecisions, ModelProtection};
use std::{rc::Rc, sync::Arc};
use swarmie_providers::{Model, ModelRequest, ModelResponse, OperationId, Progress, Recovery};
use tembra::{Code, Context, Error};

pub struct AiModel {
    connection: Connection,
    model: ModelId,
    reasoning: Option<swarmie_ai::codecs::completion::Reasoning>,
    credentials: Rc<dyn Credentials>,
    protection: ModelProtection,
    metadata: Option<Arc<swarmie_ai::catalog::Snapshot>>,
    route: Option<swarmie_ai::Model>,
    stop: Option<swarmie_ai::StopHandle>,
}

pub(super) struct PreparedCall {
    admitted: swarmie_ai::Admitted,
    description: swarmie_ai::Description,
    content: Vec<u8>,
}
impl PreparedCall {
    fn destination(&self) -> &str {
        &self.description.destination
    }
    fn content(&self) -> &[u8] {
        &self.content
    }
}

struct Executor;
impl futures::task::LocalSpawn for Executor {
    fn spawn_local_obj(
        &self,
        future: futures::task::LocalFutureObj<'static, ()>,
    ) -> Result<(), futures::task::SpawnError> {
        tokio::task::spawn_local(future);
        Ok(())
    }
}

impl AiModel {
    /// Bind an explicit operation Stop independently of observer lifetime.
    pub fn with_stop(mut self, stop: swarmie_ai::StopHandle) -> Self {
        self.stop = Some(stop);
        self
    }

    pub fn new(
        connection: Connection,
        model: ModelId,
        credentials: Rc<dyn Credentials>,
        protection: ModelProtection,
    ) -> tembra::Result<Self> {
        connection.validate()?;
        if !connection.models.contains(&model) {
            return Err(invalid(
                Reason::Configuration,
                "model is not admitted by this connection",
            ));
        }
        tokio::runtime::Handle::try_current().map_err(|_| {
            Error::new(
                Code::Unavailable,
                Reason::Runtime.as_str(),
                "model HTTP requires the host Tokio runtime",
            )
        })?;
        Ok(Self {
            connection,
            model,
            reasoning: None,
            credentials,
            protection,
            metadata: None,
            route: None,
            stop: None,
        })
    }
    pub fn with_reasoning(mut self, effort: &str) -> tembra::Result<Self> {
        if self.metadata.is_some() {
            return Err(invalid(
                Reason::Configuration,
                "attach metadata after selecting reasoning",
            ));
        }
        self.reasoning = Some(crate::configuration::reasoning(effort)?);
        Ok(self)
    }
    pub fn with_metadata(
        mut self,
        metadata: Arc<swarmie_ai::catalog::Snapshot>,
    ) -> tembra::Result<Self> {
        let effort = self.reasoning.map(|r| match r {
            swarmie_ai::codecs::completion::Reasoning::Effort(e) => e.as_str(),
            _ => "off",
        });
        if metadata.identity() != &self.connection.model_identity(&self.model)?
            || metadata.preparation().configuration_revision
                != self
                    .connection
                    .configuration_revision(&self.model, effort)?
        {
            return Err(invalid(
                Reason::Configuration,
                "metadata does not match operational model configuration",
            ));
        }
        if metadata.context().known().is_none()
            || metadata.input().known().is_none()
            || metadata.output().known().is_none()
        {
            return Err(Error::new(
                Code::Unavailable,
                "swarmie.models.metadata",
                "required model limits are unavailable",
            ));
        }
        if matches!(
            metadata.native_compaction(),
            swarmie_ai::catalog::Capability::Supported { .. }
        ) {
            return Err(invalid(
                Reason::Configuration,
                "native compaction is not qualified for this adapter",
            ));
        }
        let timeout = std::time::Duration::from_millis(self.connection.timeout_ms.get());
        let timeouts = swarmie_ai::Timeouts {
            completion: timeout,
            headers: timeout,
            idle: timeout,
            max_response_bytes: self.connection.response_bytes.get(),
        };
        self.route = Some(
            if self.connection.credential_key.is_some() {
                swarmie_ai::Model::deferred(
                    metadata.clone(),
                    self.connection.provider.protocol(),
                    swarmie_ai::AuthProfile::ApiKey,
                    timeouts,
                )
            } else {
                swarmie_ai::Model::new(
                    metadata.clone(),
                    self.connection.provider.protocol(),
                    swarmie_ai::AuthProfile::Anonymous,
                    None,
                    None,
                    timeouts,
                )
            }
            .map_err(upstream)?,
        );
        self.metadata = Some(metadata);
        Ok(self)
    }
    pub fn metadata(&self) -> Option<&Arc<swarmie_ai::catalog::Snapshot>> {
        self.metadata.as_ref()
    }
    fn budget(&self, bytes: usize, request: bool) -> tembra::Result<()> {
        let limit = if request {
            self.connection.request_bytes.get()
        } else {
            self.connection.response_bytes.get()
        };
        if bytes > limit {
            return Err(Error::new(
                Code::Exhausted,
                if request {
                    Reason::Request.as_str()
                } else {
                    Reason::Response.as_str()
                },
                "model payload exceeds its configured decoded limit",
            ));
        }
        Ok(())
    }
    async fn execute_prepared(
        &self,
        context: &Context,
        operation: OperationId,
        request: &ModelRequest,
        progress: Option<&Progress>,
        prepared: PreparedCall,
    ) -> tembra::Result<ModelResponse> {
        context.check().map_err(crate::error::before_dispatch)?;
        let stopped_error = || {
            Error::new(
                Code::Cancelled,
                "swarmie.models.ai.not_dispatched",
                "the operation was stopped before dispatch",
            )
        };
        let check_stop = || {
            if self.stop.as_ref().is_some_and(|stop| stop.is_cancelled()) {
                Err(stopped_error())
            } else {
                Ok(())
            }
        };
        check_stop()?;
        if let Some(progress) = progress {
            progress.publish(
                context,
                swarmie_providers::ModelProgress {
                    operation,
                    protection: swarmie_providers::SubmissionProtection::Pending,
                    content: String::new(),
                },
            );
        }
        context.check().map_err(crate::error::before_dispatch)?;
        check_stop()?;
        let stopped = async {
            if let Some(stop) = &self.stop {
                stop.cancelled().await;
            } else {
                futures::future::pending::<()>().await;
            }
        };
        futures::pin_mut!(stopped);
        let authenticate = async {
            let admitted = self
                .protection
                .admit(
                    context,
                    operation,
                    prepared,
                    &format!("{}:{}", self.connection.id.as_str(), self.model.as_str()),
                )
                .await?;
            check_stop()?;
            self.authentication(context, admitted).await
        };
        futures::pin_mut!(authenticate);
        // Ending observation retires unanswered decisions and the scoped binding.
        // Credential I/O retains its physical owner independently of this wait.
        let mut authentication = tokio::select! {
            biased;
            _ = &mut stopped => return Err(stopped_error()),
            result = &mut authenticate => result.map_err(crate::error::before_dispatch)?,
        };
        if let Some(progress) = progress {
            progress.publish(
                context,
                swarmie_providers::ModelProgress {
                    operation,
                    protection: authentication.protection(),
                    content: String::new(),
                },
            );
        }
        context.check().map_err(crate::error::before_dispatch)?;
        check_stop()?;
        let execution = authentication
            .dispatch(context, self.stop.clone())
            .map_err(crate::error::before_dispatch)?;
        let mut previews = execution.previews();
        let signal = context.signal();
        let cancelled = signal.cancelled();
        futures::pin_mut!(cancelled);
        let stop = execution.stop_handle();
        let terminal = execution.wait();
        futures::pin_mut!(terminal);
        let interval = std::time::Duration::from_millis(self.connection.progress_interval_ms.get());
        let mut last: Option<tokio::time::Instant> = None;
        let mut pending: Option<String> = None;
        let deadline = tokio::time::sleep(interval);
        futures::pin_mut!(deadline);
        // The host retains the terminal receiver through explicit Stop. Only the
        // latest preview waits for its display interval; previews grant no receipt.
        let result = loop {
            tokio::select! {
                biased;
                result=&mut terminal=>break result,
                _=&mut stopped=>{stop.cancel();break terminal.await;}
                _=&mut cancelled=>{stop.cancel();break terminal.await;}
                _=&mut deadline, if pending.is_some()=>{
                    if let (Some(progress),Some(content))=(progress,pending.take()) {progress.publish(context,swarmie_providers::ModelProgress {operation,protection:authentication.protection(),content:authentication.preview(&content)});last=Some(tokio::time::Instant::now());}
                }
                changed=previews.changed(), if progress.is_some()=>{
                    if changed.is_err() {break terminal.await;}
                    let content=previews.borrow_and_update().clone();
                    if let (Some(progress),Some(content))=(progress,content) {
                        if content.is_empty() { continue; }
                        let now=tokio::time::Instant::now();
                        if last.is_none_or(|previous|now.duration_since(previous)>=interval) {
                            progress.publish(context,swarmie_providers::ModelProgress {operation,protection:authentication.protection(),content:authentication.preview(&content)});
                            last=Some(now);pending=None;
                        } else {pending=Some(content);if let Some(last)=last {deadline.as_mut().reset(last+interval);}}
                    }
                }
            }
        };
        let outcome = result.map_err(upstream)?;
        let completion = outcome
            .completion()
            .ok_or_else(|| invalid(Reason::Response, "typed provider result is unavailable"))?
            .map_err(|_| invalid(Reason::Response, "provider response cannot be projected"))?;
        let mut output = conversion::response(completion, request)?;
        output.protection = authentication.protection();
        authentication.mask_response(&mut output);
        // Native continuation is separately bounded by the physical response
        // limit. Do not charge duplicate retained protocol data against the
        // existing visible decoded-output budget.
        let native = output.message.provider.take();
        let visible = serde_json::to_vec(&output);
        output.message.provider = native;
        self.budget(
            visible
                .map_err(|_| invalid(Reason::Response, "response cannot be encoded"))?
                .len(),
            false,
        )?;
        if let Some(progress) = progress {
            progress.publish(
                context,
                swarmie_providers::ModelProgress {
                    operation,
                    protection: output.protection,
                    content: output.message.content.clone(),
                },
            );
        }
        Ok(output)
    }
    async fn stream_prepared(
        &self,
        context: &Context,
        operation: OperationId,
        request: &ModelRequest,
        progress: &Progress,
        prepared: PreparedCall,
    ) -> tembra::Result<ModelResponse> {
        self.execute_prepared(context, operation, request, Some(progress), prepared)
            .await
    }
    async fn complete_prepared(
        &self,
        context: &Context,
        operation: OperationId,
        request: &ModelRequest,
        prepared: PreparedCall,
    ) -> tembra::Result<ModelResponse> {
        self.execute_prepared(context, operation, request, None, prepared)
            .await
    }
}
impl Model for AiModel {
    fn prepare<'a>(
        &'a self,
        context: &'a Context,
        operation: OperationId,
        request: ModelRequest,
        progress: Option<Progress>,
    ) -> LocalBoxFuture<'a, tembra::Result<swarmie_providers::PreparedModelRequest<'a>>> {
        self.prepare_native(context, operation, request, progress)
            .boxed_local()
    }

    fn stream<'a>(
        &'a self,
        context: &'a Context,
        operation: OperationId,
        request: &'a ModelRequest,
        progress: &'a Progress,
    ) -> LocalBoxFuture<'a, tembra::Result<ModelResponse>> {
        async move {
            context.check()?;
            let prepared = self.final_request(request, true, operation).await?;
            self.stream_prepared(context, operation, request, progress, prepared)
                .await
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
            context.check()?;
            let prepared = self.final_request(request, false, operation).await?;
            self.complete_prepared(context, operation, request, prepared)
                .await
        }
        .boxed_local()
    }

    fn recover<'a>(
        &'a self,
        context: &'a Context,
        _operation: OperationId,
        _request: &'a ModelRequest,
    ) -> LocalBoxFuture<'a, tembra::Result<Recovery<ModelResponse>>> {
        // A local operation ID does not establish remote provider idempotency.
        // No profile exposes portable operation lookup; never infer safe replay.
        async move {
            context.check()?;
            Ok(Recovery::Unknown)
        }
        .boxed_local()
    }
}
