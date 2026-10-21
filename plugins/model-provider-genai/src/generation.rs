use crate::{
    Limits,
    backend::{Backend, ClientConnection},
    conversion,
    error::{failure, map},
};
use futures::{
    FutureExt, StreamExt,
    future::{Either, select},
};
use genai::chat::{ChatStreamEvent, RawFrameRef};
use std::{
    cell::{Cell as Flag, RefCell},
    rc::Rc,
    sync::{Arc, Mutex},
    task::{Poll, Waker},
    time::Duration,
};
use swarmie_model_contracts::*;
use tembra::{Cell, Context, Readable, Result, RuntimeError, core};

/// Internal coalesced wakeup; all consumer observations use Tembra State.
#[derive(Default)]
pub(crate) struct Wake {
    pending: Flag<bool>,
    waiter: RefCell<Option<Waker>>,
}
impl Wake {
    pub fn notify(&self) {
        self.pending.set(true);
        if let Some(w) = self.waiter.borrow_mut().take() {
            w.wake();
        }
    }
    pub async fn wait(&self) {
        futures::future::poll_fn(|cx| {
            if self.pending.replace(false) {
                Poll::Ready(())
            } else {
                *self.waiter.borrow_mut() = Some(cx.waker().clone());
                Poll::Pending
            }
        })
        .await
    }
}

pub(crate) struct Generation {
    pub state: Cell<GenerationState>,
    pub request: ModelRequest,
    pub admission: RefCell<Option<Rc<GenerationResources>>>,
    pub limits: Limits,
    pub driver: Rc<dyn core::Driver>,
    pub wake: Wake,
    pub cancellation: core::CancellationToken,
}

pub(crate) struct GenerationResources {
    pub connection: Rc<ClientConnection>,
    pub backend: Rc<Backend>,
}
impl Drop for GenerationResources {
    fn drop(&mut self) {
        self.backend
            .generations
            .set(self.backend.generations.get() - 1);
    }
}

impl Generation {
    pub fn initial(request: &ModelRequest) -> GenerationState {
        GenerationState {
            request_id: request.request_id.clone(),
            phase: GenerationPhase::Prepared,
            acknowledged: 0,
            last_sequence: 0,
            events: vec![],
            response: None,
            error: None,
            physically_settled: false,
        }
    }

    pub async fn work(self: Rc<Self>, scope: core::FacetScope) -> Result<()> {
        // Resources open before State initialization; do not rely on Driver scheduling order.
        self.state.readable().await_hydrated().await?;
        loop {
            let state = self.state.value()?;
            if state.phase != GenerationPhase::Prepared || self.cancellation.is_cancelled() {
                break;
            }
            self.wake.wait().await;
        }
        self.execute(&scope).await
    }

    async fn execute(&self, scope: &core::FacetScope) -> Result<()> {
        let resource = self.admission.borrow().clone().ok_or_else(|| {
            RuntimeError::unavailable("model_admission", "generation Resource is closed")
        })?;
        let timer = self.driver.sleep(Duration::from_millis(
            resource.connection.configuration.timeout_ms,
        ));
        let stopped = self.cancellation.cancelled();
        // Both losing futures are dropped before publishing local physical settlement.
        let result = {
            let inference = self.infer(scope, &resource.connection).boxed_local();
            match select(inference, select(stopped, timer)).await {
                Either::Left((result, _)) => result,
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
            }
        };
        let state = self.state.clone();
        scope
            .root(move |context| {
                async move {
                    state
                        .change(&Context::from_core(context), |state| {
                            state.phase = GenerationPhase::Settled;
                            state.physically_settled = true;
                            match result {
                                Ok(response) => state.response = Some(response),
                                Err(error) => state.error = Some(error),
                            }
                            Ok(())
                        })
                        .map(|_| ())
                }
                .boxed_local()
            })
            .await
    }

    async fn publish(&self, scope: &core::FacetScope, batch: &mut Vec<ModelEvent>) -> Result<()> {
        if batch.is_empty() {
            return Ok(());
        }
        loop {
            let current = self.state.value()?;
            let mut candidate = current.events.clone();
            for (offset, event) in batch.iter().cloned().enumerate() {
                candidate.push(SequencedEvent {
                    sequence: current.last_sequence.saturating_add(offset as u64 + 1),
                    event,
                });
            }
            let candidate_size = conversion::bytes(&candidate);
            if current.events.len() + batch.len() <= self.limits.window_events
                && candidate_size <= self.limits.window_bytes
            {
                break;
            }
            self.wake.wait().await;
        }
        let state = self.state.clone();
        let events = std::mem::take(batch);
        scope
            .root(move |context| {
                async move {
                    state
                        .change(&Context::from_core(context), |state| {
                            for event in events {
                                state.last_sequence =
                                    state.last_sequence.checked_add(1).ok_or_else(|| {
                                        RuntimeError::invalid(
                                            "model_sequence",
                                            "generation sequence exhausted",
                                        )
                                    })?;
                                state.events.push(SequencedEvent {
                                    sequence: state.last_sequence,
                                    event,
                                });
                            }
                            Ok(())
                        })
                        .map(|_| ())
                }
                .boxed_local()
            })
            .await
    }

    async fn infer(
        &self,
        scope: &core::FacetScope,
        connection: &Rc<ClientConnection>,
    ) -> std::result::Result<ModelResponse, ProviderError> {
        if self.cancellation.is_cancelled() {
            return Err(failure(ProviderErrorKind::Cancelled, Submission::NotSent));
        }
        let _permit = connection.permit()?;
        let (request, options) = conversion::request(&self.request, connection, &self.limits)?;
        // genai can discard parallel tool calls or simultaneous content in OpenAI frames.
        // Inspect without retaining raw frames; fail before publishing any lossy event.
        let guard = Arc::new(Mutex::new(FrameGuard::default()));
        let observation = guard.clone();
        let limit = self.limits.event_bytes;
        let options = options.with_raw_frame_fn(move |_, frame: RawFrameRef<'_>| {
            let mut guard = observation.lock().unwrap_or_else(|e| e.into_inner());
            guard.inspect(frame, limit);
        });
        let mut stream = connection
            .client
            .exec_chat_stream(&self.request.model.model, request, Some(&options))
            .await
            .map_err(map)?
            .stream;
        let mut batch = Vec::new();
        let mut batch_bytes = 0usize;
        let mut observed = 0usize;
        let mut text = String::new();
        let mut reasoning = String::new();
        loop {
            // A full published window stops polling upstream until acknowledged.
            let current = self
                .state
                .value()
                .map_err(|_| failure(ProviderErrorKind::Cancelled, Submission::MayHaveBeenSent))?;
            if current.events.len() >= self.limits.window_events
                || conversion::bytes(&current.events) + self.limits.event_bytes
                    > self.limits.window_bytes
            {
                self.wake.wait().await;
                continue;
            }
            let tick = self
                .driver
                .sleep(Duration::from_millis(self.limits.batch_interval_ms));
            let event = match select(stream.next().boxed_local(), tick).await {
                Either::Left((event, _)) => event,
                Either::Right(_) => {
                    self.publish(scope, &mut batch).await.map_err(|_| {
                        failure(ProviderErrorKind::Cancelled, Submission::MayHaveBeenSent)
                    })?;
                    batch_bytes = 0;
                    continue;
                }
            };
            if let Some(kind) = guard.lock().unwrap_or_else(|e| e.into_inner()).failure {
                return Err(failure(kind, Submission::ResponseReceived));
            }
            let event = event
                .ok_or_else(|| failure(ProviderErrorKind::Parsing, Submission::MayHaveBeenSent))?
                .map_err(map)?;
            let delta = match event {
                ChatStreamEvent::Start | ChatStreamEvent::Heartbeat => continue,
                ChatStreamEvent::Chunk(chunk) => {
                    text.push_str(&chunk.content);
                    ModelEvent::TextDelta {
                        text: chunk.content,
                    }
                }
                ChatStreamEvent::ReasoningChunk(chunk) => {
                    reasoning.push_str(&chunk.content);
                    ModelEvent::ReasoningDelta {
                        text: chunk.content,
                    }
                }
                ChatStreamEvent::ThoughtSignatureChunk(chunk) => ModelEvent::Extension {
                    extension: Extension {
                        namespace: format!(
                            "swarmie.continuation.{}",
                            connection.configuration.provider
                        ),
                        version: 1,
                        value: serde_json::json!({"kind":"thought_signature","value":chunk.content}),
                    },
                },
                ChatStreamEvent::ToolCallChunk(chunk) => ModelEvent::ToolCallSnapshot {
                    id: chunk.tool_call.call_id,
                    name: chunk.tool_call.fn_name,
                    arguments_json: match chunk.tool_call.fn_arguments {
                        serde_json::Value::String(s) => s,
                        v => v.to_string(),
                    },
                },
                ChatStreamEvent::End(end) => {
                    self.publish(scope, &mut batch).await.map_err(|_| {
                        failure(ProviderErrorKind::Cancelled, Submission::MayHaveBeenSent)
                    })?;
                    let metadata = guard.lock().unwrap_or_else(|e| e.into_inner());
                    let completion = conversion::Completion {
                        content: end.captured_content.unwrap_or_default(),
                        reasoning: end.captured_reasoning_content,
                        stop: end.captured_stop_reason,
                        usage: end.captured_usage.unwrap_or_default(),
                        response_id: end
                            .captured_response_id
                            .or_else(|| metadata.response_id.clone()),
                        raw_usage: metadata.raw_usage.clone(),
                        reported_model: metadata.reported_model.clone(),
                    };
                    drop(metadata);
                    let response =
                        conversion::response(&self.request, connection, completion, &self.limits)?;
                    let final_text = response
                        .message
                        .content
                        .iter()
                        .filter_map(|p| match p {
                            ContentPart::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<String>();
                    let final_reasoning = response
                        .message
                        .content
                        .iter()
                        .filter_map(|p| match p {
                            ContentPart::Reasoning { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<String>();
                    if final_text != text || (!reasoning.is_empty() && final_reasoning != reasoning)
                    {
                        return Err(failure(
                            ProviderErrorKind::DataLoss,
                            Submission::ResponseReceived,
                        ));
                    }
                    return Ok(response);
                }
            };
            let size = conversion::bytes(&delta);
            observed = observed.saturating_add(size);
            if size > self.limits.event_bytes || observed > self.limits.response_bytes {
                return Err(failure(
                    ProviderErrorKind::PayloadLimit,
                    Submission::ResponseReceived,
                ));
            }
            if batch_bytes + size > self.limits.window_bytes / 2
                || batch.len() >= self.limits.batch_events
            {
                self.publish(scope, &mut batch).await.map_err(|_| {
                    failure(ProviderErrorKind::Cancelled, Submission::MayHaveBeenSent)
                })?;
                batch_bytes = 0;
            }
            batch_bytes += size;
            batch.push(delta);
            if batch.len() >= self.limits.batch_events {
                self.publish(scope, &mut batch).await.map_err(|_| {
                    failure(ProviderErrorKind::Cancelled, Submission::MayHaveBeenSent)
                })?;
                batch_bytes = 0;
            }
        }
    }
}

#[derive(Default)]
struct FrameGuard {
    failure: Option<ProviderErrorKind>,
    response_id: Option<String>,
    reported_model: Option<String>,
    raw_usage: Option<serde_json::Value>,
}
impl FrameGuard {
    fn inspect(&mut self, frame: RawFrameRef<'_>, limit: usize) {
        if let Some(v) = frame.json() {
            if conversion::bytes(&v) > limit {
                self.failure = Some(ProviderErrorKind::PayloadLimit);
                return;
            }
            if let Some(id) = v.get("id").and_then(|v| v.as_str()) {
                self.response_id = Some(id.into());
            }
            if let Some(model) = v.get("model").and_then(|v| v.as_str()) {
                self.reported_model = Some(model.into());
            }
            if let Some(usage) = v.get("usage").or_else(|| v.get("usageMetadata")) {
                self.raw_usage = Some(usage.clone());
            }
            if let Some(delta) = v.pointer("/choices/0/delta") {
                let count = delta
                    .get("tool_calls")
                    .and_then(|v| v.as_array())
                    .map_or(0, |v| v.len());
                if count > 1
                    || (count > 0
                        && delta
                            .get("content")
                            .and_then(|v| v.as_str())
                            .is_some_and(|s| !s.is_empty()))
                    || delta.get("reasoning_details").is_some()
                {
                    self.failure = Some(ProviderErrorKind::DataLoss);
                }
            }
        }
    }
}

/// The Resource owns an existing-Driver worker and joins it before releasing clients.
/// It creates no long-lived invocation pin on the referenced generation object.
pub(crate) struct GenerationWorker {
    pub generation: Rc<Generation>,
    pub stop: core::CancellationToken,
    pub finished: RefCell<Option<futures::channel::oneshot::Receiver<Result<()>>>>,
}

pub(crate) struct GenerationService {
    pub generation: Rc<Generation>,
    pub _provider: ModelReference,
    pub _worker: tembra::Resource<GenerationWorker>,
}
impl ModelGenerationSchemaProvider for GenerationService {
    fn progress(&self) -> Readable<GenerationState> {
        self.generation.state.readable()
    }
    async fn start(&self, context: Context) -> Result<()> {
        self.generation.state.change(&context, |state| {
            if state.phase == GenerationPhase::Prepared {
                state.phase = GenerationPhase::Running;
            }
            Ok(())
        })?;
        self.generation.wake.notify();
        Ok(())
    }
    async fn acknowledge(&self, through: u64, context: Context) -> Result<()> {
        self.generation.state.change(&context, |state| {
            if through > state.last_sequence {
                return Err(RuntimeError::invalid(
                    "model_acknowledgement",
                    "acknowledgement exceeds published sequence",
                ));
            }
            if through > state.acknowledged {
                state.acknowledged = through;
                state.events.retain(|e| e.sequence > through);
            }
            Ok(())
        })?;
        self.generation.wake.notify();
        Ok(())
    }
    async fn cancel(&self, _context: Context) -> Result<()> {
        self.generation.cancellation.cancel(RuntimeError::aborted(
            "model_cancelled",
            "generation cancellation requested",
        ));
        self.generation.wake.notify();
        Ok(())
    }
}
