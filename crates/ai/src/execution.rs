use crate::{Error, Failure, Prepared, Protocol, WireMode};
use reqwest::header::CONTENT_TYPE;
use serde_json::Value;
use tokio::{
    sync::{oneshot, watch},
    time::timeout,
};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use tracing::instrument::WithSubscriber;

/// The final provider response, including unknown output fields and continuation
/// records. It is protocol data, not a normalized or executable tool invocation.
#[derive(Clone)]
pub struct Outcome {
    protocol: Protocol,
    response: Option<Value>,
    events: Vec<Value>,
    completion: Option<Result<crate::CompletionResponse, Failure>>,
    provider_request_id: Option<String>,
}

impl Outcome {
    pub fn provider_request_id(&self) -> Option<&str> {
        self.provider_request_id.as_deref()
    }

    fn with_request_id(mut self, value: Option<String>) -> Self {
        self.provider_request_id = value;
        self
    }

    /// Typed projection of the confirmed provider document, when prepared from typed input.
    /// Projection failure retains the raw outcome and never grants a retry.
    pub fn completion(&self) -> Option<Result<&crate::CompletionResponse, Failure>> {
        self.completion
            .as_ref()
            .map(|result| result.as_ref().map_err(|error| *error))
    }

    pub fn usage(&self) -> Option<&crate::Usage> {
        self.completion
            .as_ref()?
            .as_ref()
            .ok()
            .map(|response| &response.usage)
    }

    pub fn protocol(&self) -> Protocol {
        self.protocol
    }

    /// Complete provider JSON. Streamed responses rebuild the protocol document
    /// from confirmed events; Responses uses its terminal's complete response.
    /// Original events remain available separately. The caller must validate
    /// tool arguments and authority before execution or history publication.
    pub fn response(&self) -> Option<&Value> {
        self.response.as_ref()
    }

    /// Original JSON events in arrival order, including opaque/unknown fields.
    /// A terminal result is not an executable tool call; the caller interprets it.
    pub fn events(&self) -> &[Value] {
        &self.events
    }

    pub(crate) fn streamed(
        protocol: Protocol,
        response: Option<Value>,
        events: Vec<Value>,
    ) -> Self {
        Self {
            protocol,
            response,
            events,
            completion: None,
            provider_request_id: None,
        }
    }
}

/// Explicit local Stop intent. It carries no remote cancellation or replay proof.
#[derive(Clone, Default)]
pub struct StopHandle(CancellationToken);
impl StopHandle {
    pub fn new() -> Self {
        Self(CancellationToken::new())
    }
    pub fn cancel(&self) {
        self.0.cancel();
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.is_cancelled()
    }
    pub async fn cancelled(&self) {
        self.0.cancelled().await;
    }
}

/// Host-owned terminal result for an independently running request. Keep this
/// handle until settlement; presentation receives only bounded `previews`.
/// Dropping previews never requests Stop. Dropping the owner leaves transport
/// running but loses result delivery. Stop cannot prove remote cancellation.
pub struct Execution {
    result: oneshot::Receiver<Result<Outcome, Error>>,
    previews: watch::Receiver<Option<String>>,
    stop: CancellationToken,
}

impl Execution {
    pub fn previews(&self) -> watch::Receiver<Option<String>> {
        self.previews.clone()
    }

    /// Cancellation handle for a host retaining the terminal future through Stop.
    pub fn stop_handle(&self) -> StopHandle {
        StopHandle(self.stop.clone())
    }

    pub fn stop(&self) {
        self.stop.cancel();
    }

    pub async fn wait(self) -> Result<Outcome, Error> {
        self.result
            .await
            .unwrap_or_else(|_| Err(Error::unknown(Failure::Runtime)))
    }
}

pub(crate) fn dispatch(
    prepared: Prepared,
    runtime: &tokio::runtime::Handle,
    guard: impl FnOnce(&crate::Description) -> Result<(), Error> + Send + 'static,
) -> Result<Execution, Error> {
    let (execution, task) = task(prepared, guard);
    runtime.spawn(task);
    Ok(execution)
}

pub(crate) fn dispatch_local(
    prepared: Prepared,
    executor: &impl futures::task::LocalSpawn,
    guard: impl FnOnce(&crate::Description) -> Result<(), Error> + 'static,
) -> Result<Execution, Error> {
    use futures::task::LocalSpawnExt;
    let (execution, task) = task(prepared, guard);
    executor
        .spawn_local(task)
        .map_err(|_| Error::before(Failure::Runtime))?;
    Ok(execution)
}

fn task(
    prepared: Prepared,
    guard: impl FnOnce(&crate::Description) -> Result<(), Error> + 'static,
) -> (Execution, impl std::future::Future<Output = ()>) {
    let (sender, result) = oneshot::channel();
    let (preview_sender, previews) = watch::channel(None);
    let stop = CancellationToken::new();
    let task_stop = stop.clone();
    let parent = tracing::Span::current();
    let task = async move {
        let result = if let Err(error) = prepared.check_dispatch() {
            Err(error)
        } else if task_stop.is_cancelled() {
            Err(Error::before(Failure::Stopped))
        } else if let Err(error) = guard(&prepared.description) {
            Err(Error::before(error.failure))
        } else if let Err(error) = prepared.check_dispatch() {
            Err(error)
        } else {
            tokio::select! {
                biased;
                _ = task_stop.cancelled() => Err(Error::unknown(Failure::Stopped)),
                result = execute(prepared, preview_sender) => result,
            }
        };
        let _ = sender.send(result);
    }
    .instrument(parent)
    .with_current_subscriber();
    (
        Execution {
            result,
            previews,
            stop,
        },
        task,
    )
}

async fn execute(
    prepared: Prepared,
    previews: watch::Sender<Option<String>>,
) -> Result<Outcome, Error> {
    let span = prepared
        .start_span
        .as_ref()
        .map(|start| start())
        .unwrap_or_else(tracing::Span::none);
    let config = &prepared.configuration;
    let request = config
        .client
        .post(prepared.endpoint.clone())
        .headers(prepared.headers)
        .body(prepared.body);
    let streaming = prepared.description.mode == WireMode::Stream;
    let run = async {
        let sent = if streaming {
            timeout(config.timeouts.headers, request.send())
                .await
                .map_err(|_| Error::unknown(Failure::Timeout))?
        } else {
            request.send().await
        };
        let mut response = sent.map_err(|_| Error::unknown(Failure::Transport))?;
        if !response.status().is_success() {
            return Err(Error::unknown(Failure::Http(response.status().as_u16())));
        }
        let provider_request_id = prepared
            .request_id_header
            .and_then(|header| response.headers().get(header))
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        if let Some(id) = provider_request_id.as_deref() {
            span.record(crate::codecs::telemetry::PROVIDER_REQUEST_ID_FIELD, id);
        }
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim();
        if (streaming
            && if config.protocol == Protocol::Ollama {
                !matches!(content_type, "application/x-ndjson" | "application/json")
            } else {
                content_type != "text/event-stream"
            })
            || (!streaming && content_type != "application/json")
        {
            return Err(Error::unknown(Failure::Protocol));
        }
        let mut buffer = Vec::new();
        let mut received = 0_usize;
        let mut decoder =
            crate::stream::Decoder::new(config.protocol, config.snapshot.identity().clone());
        loop {
            let chunk = if streaming {
                timeout(config.timeouts.idle, response.chunk())
                    .await
                    .map_err(|_| Error::unknown(Failure::Timeout))?
            } else {
                response.chunk().await
            };
            let chunk = chunk.map_err(|_| Error::unknown(Failure::Transport))?;
            let Some(chunk) = chunk else { break };
            received = received
                .checked_add(chunk.len())
                .filter(|size| *size <= config.timeouts.max_response_bytes)
                .ok_or_else(|| Error::unknown(Failure::Limit))?;
            if streaming {
                if let Some(value) = decoder.push(&chunk, &previews)? {
                    return Ok(value.with_request_id(provider_request_id));
                }
            } else {
                buffer.extend_from_slice(&chunk);
            }
        }
        if streaming {
            return decoder
                .finish(&previews)
                .map(|outcome| outcome.with_request_id(provider_request_id));
        }
        let response: Value =
            serde_json::from_slice(&buffer).map_err(|_| Error::unknown(Failure::Protocol))?;
        config
            .protocol
            .validate_response(&response, config.snapshot.identity())?;
        Ok(Outcome {
            protocol: config.protocol,
            response: Some(response),
            events: vec![],
            completion: None,
            provider_request_id,
        })
    };
    let folded = async {
        let mut outcome = run.await?;
        if let Some(decode) = prepared.decode {
            outcome.completion = Some(outcome.response.as_ref().ok_or(Failure::Protocol).and_then(
                |raw| {
                    let mut completion = decode(raw)?;
                    // The confirmed transport document is authoritative even if a
                    // typed reassembler does not model every extension field.
                    completion.raw = raw.clone();
                    if outcome.provider_request_id.is_some() {
                        completion.provider_request_id = outcome.provider_request_id.clone();
                    }
                    Ok(completion)
                },
            ));
        }
        Ok(outcome)
    };
    let folded = folded.instrument(span.clone());
    if prepared.description.mode == WireMode::Complete {
        timeout(config.timeouts.completion, folded)
            .await
            .map_err(|_| Error::unknown(Failure::Timeout))?
    } else {
        folded.await
    }
}
