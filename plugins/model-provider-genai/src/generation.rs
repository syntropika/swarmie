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
    collections::BTreeMap,
    panic::AssertUnwindSafe,
    rc::Rc,
    sync::{Arc, Mutex},
    task::{Poll, Waker},
    time::Duration,
};
use swarmie_model_contracts::*;
use tembra::{Cell, Context, Readable, Result, RuntimeError, core};

/// Internal coalesced wakeup; incremental observations use Tembra State.
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
    // Bounded terminal authority survives rejected State candidates until Resource disposal.
    pub completion: RefCell<Option<GenerationState>>,
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
        self.settle(scope, result).await
    }

    async fn settle(
        &self,
        scope: &core::FacetScope,
        result: std::result::Result<ModelResponse, ProviderError>,
    ) -> Result<()> {
        let mut terminal = self.state.value()?;
        terminal.phase = GenerationPhase::Settled;
        terminal.physically_settled = true;
        match result {
            Ok(response) => terminal.response = Some(response),
            Err(error) => terminal.error = Some(error),
        }
        *self.completion.borrow_mut() = Some(terminal);
        // Publication repair is bounded and never encloses or reexecutes infer(). An internal
        // projection can reject every terminal candidate while recover() still returns it.
        for attempt in 0..3 {
            let state = self.state.clone();
            let terminal = self.completion.borrow().as_ref().unwrap().clone();
            let publication = AssertUnwindSafe(scope.root(move |context| {
                async move {
                    state
                        .change(&Context::from_core(context), |state| {
                            state.phase = terminal.phase;
                            state.physically_settled = true;
                            state.response = terminal.response;
                            state.error = terminal.error;
                            Ok(())
                        })
                        .map(|_| ())
                }
                .boxed_local()
            }))
            .catch_unwind()
            .await;
            // Native observer panics happen after adoption. Inspect authority before any
            // publication-only retry, avoiding duplicate events or retraction of a result.
            if matches!(publication, Ok(Ok(())))
                || self.state.value().is_ok_and(|s| s.physically_settled)
            {
                return Ok(());
            }
            if attempt < 2 {
                self.driver.sleep(Duration::from_millis(1)).await;
            }
        }
        // Failed observation must not turn a physically finished worker into failed cleanup.
        Ok(())
    }

    fn recovered(&self) -> Result<GenerationState> {
        match self.completion.borrow().as_ref() {
            Some(terminal) => {
                let mut snapshot = self.state.value().unwrap_or_else(|_| terminal.clone());
                snapshot.phase = terminal.phase;
                snapshot.physically_settled = terminal.physically_settled;
                snapshot.response.clone_from(&terminal.response);
                snapshot.error.clone_from(&terminal.error);
                Ok(snapshot)
            }
            None => self.state.value(),
        }
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
        let expected = self
            .state
            .value()?
            .last_sequence
            .checked_add(events.len() as u64)
            .ok_or_else(|| {
                RuntimeError::invalid("model_sequence", "generation sequence exhausted")
            })?;
        let publication = AssertUnwindSafe(scope.root(move |context| {
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
        }))
        .catch_unwind()
        .await;
        match publication {
            Ok(Ok(())) => Ok(()),
            _ if self
                .state
                .value()
                .is_ok_and(|s| s.last_sequence == expected) =>
            {
                Ok(())
            }
            Ok(Err(error)) => Err(error),
            Err(_) => Err(RuntimeError::unavailable(
                "model_publication",
                "generation publication interrupted",
            )),
        }
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
        let capture = Arc::new(Mutex::new(conversion::StreamCapture::new(
            connection.kind,
            self.limits.response_bytes,
        )));
        let captured = capture.clone();
        let limit = self.limits.event_bytes;
        let kind = connection.kind;
        let options = options.with_raw_frame_fn(move |_, frame: RawFrameRef<'_>| {
            let mut guard = observation.lock().unwrap_or_else(|e| e.into_inner());
            guard.inspect(frame, limit);
            if guard.failure.is_some() {
                return;
            }
            if let Some(value) = frame.json() {
                // Anthropic's genai path dispatches by the SSE event name, whereas native
                // content capture uses JSON type. Both must identify the same event.
                if kind == genai::adapter::AdapterKind::Anthropic
                    && frame.event != value.get("type").and_then(|v| v.as_str())
                {
                    guard.failure = Some(ProviderErrorKind::DataLoss);
                    return;
                }
                captured
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .observe(&value);
            }
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
        let mut tool_snapshots: BTreeMap<String, (String, String)> = BTreeMap::new();
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
                        failure(ProviderErrorKind::Publication, Submission::ResponseReceived)
                    })?;
                    batch_bytes = 0;
                    continue;
                }
            };
            capture.lock().unwrap_or_else(|e| e.into_inner()).check()?;
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
                        failure(ProviderErrorKind::Publication, Submission::ResponseReceived)
                    })?;
                    let metadata = guard.lock().unwrap_or_else(|e| e.into_inner());
                    let ordered = capture.lock().unwrap_or_else(|e| e.into_inner()).finish()?;
                    let completion = conversion::Completion {
                        raw_body: None,
                        content: ordered.unwrap_or(end.captured_content.unwrap_or_default()),
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
                    for (id, (name, arguments)) in &tool_snapshots {
                        let coherent = response.message.content.iter().any(|part| match part {
                            ContentPart::ToolCall { call } => {
                                &call.id == id && &call.name == name
                                    // A terminal item can finish an incomplete snapshot, but
                                    // must not contradict complete arguments already emitted.
                                    && serde_json::from_str::<serde_json::Value>(arguments)
                                        .map_or(true, |value| value == call.arguments)
                            }
                            _ => false,
                        });
                        if !coherent {
                            return Err(failure(
                                ProviderErrorKind::DataLoss,
                                Submission::ResponseReceived,
                            ));
                        }
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
            if let ModelEvent::ToolCallSnapshot {
                id,
                name,
                arguments_json,
            } = &delta
            {
                tool_snapshots.insert(id.clone(), (name.clone(), arguments_json.clone()));
            }
            if batch_bytes + size > self.limits.window_bytes / 2
                || batch.len() >= self.limits.batch_events
            {
                self.publish(scope, &mut batch).await.map_err(|_| {
                    failure(ProviderErrorKind::Publication, Submission::ResponseReceived)
                })?;
                batch_bytes = 0;
            }
            batch_bytes += size;
            batch.push(delta);
            if batch.len() >= self.limits.batch_events {
                self.publish(scope, &mut batch).await.map_err(|_| {
                    failure(ProviderErrorKind::Publication, Submission::ResponseReceived)
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
        if self.failure.is_some() {
            return;
        }
        // This is still after genai's framing buffer, but before its adapters can silently
        // skip malformed JSON. A later valid completion must not erase this failure.
        if frame.data.len() > limit {
            self.failure = Some(ProviderErrorKind::PayloadLimit);
            return;
        }
        if frame.data.trim().is_empty() || frame.data.trim() == "[DONE]" {
            return;
        }
        let Some(v) = frame.json().filter(serde_json::Value::is_object) else {
            self.failure = Some(ProviderErrorKind::Parsing);
            return;
        };
        {
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

impl GenerationWorker {
    pub(crate) async fn close(&self) -> Result<()> {
        self.stop.cancel(RuntimeError::aborted(
            "model_closed",
            "generation Resource closed",
        ));
        self.generation.wake.notify();
        let finished = self.finished.borrow_mut().take();
        let result = if let Some(finished) = finished {
            finished
                .await
                .map_err(|_| {
                    RuntimeError::unavailable("model_worker", "generation worker failed to settle")
                })
                .and_then(|result| result)
        } else {
            Ok(())
        };
        // Admission must be released even if the worker reports an earlier State failure.
        self.generation.admission.borrow_mut().take();
        result
    }
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
        let publication = std::panic::catch_unwind(AssertUnwindSafe(|| {
            self.generation.state.change(&context, |state| {
                if state.phase == GenerationPhase::Prepared {
                    state.phase = GenerationPhase::Running;
                }
                Ok(())
            })
        }));
        // Observer panic may happen after adoption, before this method could signal work.
        self.generation.wake.notify();
        match publication {
            Ok(result) => result.map(|_| ()),
            Err(_)
                if self
                    .generation
                    .state
                    .value()
                    .is_ok_and(|s| s.phase != GenerationPhase::Prepared) =>
            {
                Ok(())
            }
            Err(_) => Err(RuntimeError::unavailable(
                "model_publication",
                "generation start publication interrupted",
            )),
        }
    }
    async fn acknowledge(&self, through: u64, context: Context) -> Result<()> {
        let publication = std::panic::catch_unwind(AssertUnwindSafe(|| {
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
            })
        }));
        self.generation.wake.notify();
        match publication {
            Ok(result) => result.map(|_| ()),
            Err(_)
                if self
                    .generation
                    .state
                    .value()
                    .is_ok_and(|s| s.acknowledged >= through && through <= s.last_sequence) =>
            {
                Ok(())
            }
            Err(_) => Err(RuntimeError::unavailable(
                "model_publication",
                "generation acknowledgement publication interrupted",
            )),
        }
    }
    async fn recover(&self, _context: Context) -> Result<GenerationState> {
        self.generation.recovered()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{self as fixture, Reply, Server};
    use tembra::prelude::*;

    struct Credentials;
    impl crate::Credentials for Credentials {
        fn resolve(&self, _: &str) -> Option<String> {
            Some("fixture-secret".into())
        }
    }

    // The rejection is in the generation's own graph. An external projection is a different
    // failure boundary and cannot reject an already committed source mutation in Tembra.
    async fn rejecting_generation(
        host: &Host,
        configuration: crate::Configuration,
        driver: Rc<tembra_native::TokioDriver>,
        reject_terminal: bool,
        max_failures: usize,
        attempts: Rc<Flag<usize>>,
    ) -> GenerationReference {
        host.install(
            NativeFacet::new(
                "fixture.rejected-publication",
                vec![],
                move |scope, setup| {
                    let configuration = configuration.clone();
                    let driver = driver.clone();
                    let attempts = attempts.clone();
                    let scope = scope.clone();
                    let progress_definition = setup.state("progress")?;
                    let terminal_publication_guard_definition =
                        setup.state("terminal-publication-guard")?;
                    setup.serve(
                        GenerationEndpoint::new("fixture.rejected-generation")?,
                        move |input, _, declaration| {
                            let provider = declaration.get(
                                ModelEndpoint::new(crate::PROVIDER_CONTRACT)?,
                                ModelProviderSchemaInput {},
                            )?;
                            let state = declaration.state(
                                progress_definition.value(Generation::initial(&input.request))?,
                            )?;
                            let attempts = attempts.clone();
                            let _gate = declaration.derive(
                                &terminal_publication_guard_definition,
                                (state.readable(),),
                                move |(candidate,)| {
                                    if attempts.get() < max_failures
                                        && ((reject_terminal && candidate.physically_settled)
                                            || (!reject_terminal && !candidate.events.is_empty()))
                                    {
                                        attempts.set(attempts.get() + 1);
                                        return Err(RuntimeError::invalid(
                                            "fixture_publication",
                                            "internal projection refused candidate",
                                        ));
                                    }
                                    Ok(true)
                                },
                            )?;
                            let generation = Rc::new(Generation {
                                state,
                                request: input.request,
                                admission: RefCell::new(None),
                                limits: configuration.limits.clone(),
                                driver: driver.clone(),
                                wake: Wake::default(),
                                cancellation: core::CancellationToken::new(),
                                completion: RefCell::new(None),
                            });
                            let activity = generation.clone();
                            let scope = scope.clone();
                            let configuration = configuration.clone();
                            let worker = declaration.core_mut().resource(
                                "generation-worker",
                                ResourceDeclaration::new(move |_| {
                                    let generation = activity.clone();
                                    let scope = scope.clone();
                                    let configuration = configuration.clone();
                                    async move {
                                        let backend =
                                            Rc::new(Backend::open(&configuration, &Credentials)?);
                                        let connection =
                                            backend.connection(&generation.request.model).unwrap();
                                        backend.generations.set(1);
                                        *generation.admission.borrow_mut() =
                                            Some(Rc::new(GenerationResources {
                                                backend,
                                                connection,
                                            }));
                                        let stop = core::CancellationToken::new();
                                        let stopped = stop.clone();
                                        let activity = generation.clone();
                                        let (sender, receiver) =
                                            futures::channel::oneshot::channel();
                                        generation.driver.spawn(
                                            async move {
                                                let work = activity.work(scope).boxed_local();
                                                let result =
                                                    match select(work, stopped.cancelled()).await {
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
                                .with_close(|worker| {
                                    async move { worker.close().await }.boxed_local()
                                }),
                            )?;
                            Ok(GenerationService {
                                generation,
                                _provider: provider,
                                _worker: worker,
                            })
                        },
                    )
                },
            )
            .unwrap(),
            core::LoadOptions::default(),
        )
        .unwrap()
        .await
        .unwrap();
        host.get(
            GenerationEndpoint::new("fixture.rejected-generation").unwrap(),
            GenerationInput {
                request: fixture::request("rejected-publication"),
            },
        )
        .await
        .unwrap()
    }

    fn reply() -> Reply {
        Reply::Frames(vec![
            (
                0,
                fixture::frame(serde_json::json!({"choices":[{"delta":{"content":"answer"}}]})),
            ),
            (
                0,
                fixture::frame(
                    serde_json::json!({"choices":[{"delta":{},"finish_reason":"stop"}]}),
                ),
            ),
            (0, fixture::done()),
        ])
    }

    #[test]
    fn rejected_incremental_candidate_stops_inference_without_sequence_gaps() {
        let server = Server::new(vec![reply()]);
        fixture::run(|driver| async {
            let configuration = fixture::configuration(&server.url);
            let host = fixture::host(&configuration, driver.clone()).await;
            let attempts = Rc::new(Flag::new(0));
            let generation = rejecting_generation(
                &host,
                configuration,
                driver,
                false,
                usize::MAX,
                attempts.clone(),
            )
            .await;
            generation.start().await.unwrap();
            let terminal = fixture::settled(&generation).await;
            assert_eq!(terminal.error.unwrap().kind, ProviderErrorKind::Publication);
            assert_eq!(terminal.last_sequence, 0);
            assert!(terminal.events.is_empty());
            assert_eq!(attempts.get(), 1);
            assert!(generation.recover().await.unwrap().physically_settled);
            assert_eq!(server.requests.lock().unwrap().len(), 1);
            host.close().await.unwrap();
        });
    }

    #[test]
    fn rejected_terminal_candidate_has_bounded_repair_and_recoverable_result() {
        let server = Server::new(vec![reply()]);
        fixture::run(|driver| async {
            let configuration = fixture::configuration(&server.url);
            let host = fixture::host(&configuration, driver.clone()).await;
            let attempts = Rc::new(Flag::new(0));
            let generation = rejecting_generation(
                &host,
                configuration,
                driver,
                true,
                usize::MAX,
                attempts.clone(),
            )
            .await;
            generation.start().await.unwrap();
            fixture::wait_for(|| attempts.get() == 3).await;
            let published = generation.progress().value().unwrap();
            assert_eq!(published.phase, GenerationPhase::Running);
            assert!(!published.physically_settled);
            let recovered = generation.recover().await.unwrap();
            assert_eq!(recovered.phase, GenerationPhase::Settled);
            assert!(recovered.physically_settled);
            assert_eq!(
                recovered.response.unwrap().message,
                Message::text(Role::Assistant, "answer")
            );
            assert_eq!(recovered.last_sequence, 1);
            generation.acknowledge(1).await.unwrap();
            let recovered = generation.recover().await.unwrap();
            assert!(recovered.events.is_empty());
            assert_eq!(recovered.acknowledged, 1);
            generation.cancel().await.unwrap();
            assert!(generation.recover().await.unwrap().response.is_some());
            assert_eq!(server.requests.lock().unwrap().len(), 1);
            host.close().await.unwrap();
            assert!(generation.recover().await.is_err());
        });
    }
    #[test]
    fn transient_terminal_projection_failure_repairs_only_publication() {
        let server = Server::new(vec![reply()]);
        fixture::run(|driver| async {
            let configuration = fixture::configuration(&server.url);
            let host = fixture::host(&configuration, driver.clone()).await;
            let attempts = Rc::new(Flag::new(0));
            let generation =
                rejecting_generation(&host, configuration, driver, true, 1, attempts.clone()).await;
            generation.start().await.unwrap();
            let terminal = fixture::settled(&generation).await;
            assert_eq!(
                terminal.response.unwrap().message,
                Message::text(Role::Assistant, "answer")
            );
            assert_eq!(attempts.get(), 1);
            assert_eq!(terminal.last_sequence, 1);
            assert_eq!(server.requests.lock().unwrap().len(), 1);
            host.close().await.unwrap();
        });
    }
}
