use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use super::{Action, Emitter, Observation, Stage, Subject, Witness};

pub fn scrub_diagnostic(value: &str, secrets: &[String]) -> String {
    scrub::text(value, secrets)
}

pub fn diagnostic_url_secrets(url: &str) -> Vec<String> {
    scrub::url_secrets(url)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterObservation {
    pub operation: String,
    pub attempt: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_attempt: Option<std::num::NonZeroU64>,
    pub event: AdapterEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analysis: Option<AdapterAnalysis>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum AdapterEvent {
    Provider { verdict: AdapterVerdict },
    ErrorEnvelope { error: AdapterErrorEnvelope },
    Usage { usage: AdapterUsage },
    Started { method: String, route: String },
    Response { status: u16 },
    TransportEof { after: usize, partial_bytes: usize },
    Finished { ending: AdapterEnding },
    Corrupt { frame: usize },
    IdentityExhausted,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterVerdict {
    pub finish_reason: Option<String>,
    pub block_reason: Option<String>,
    pub detail: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterAnalysis {
    pub response_id: Option<String>,
    pub headers: Option<std::collections::BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterErrorEnvelope {
    pub code: Option<String>,
    pub status: Option<String>,
    pub message: Option<String>,
}

#[derive(Default)]
pub struct ObservedError {
    pub code: Option<serde_json::Value>,
    pub kind: Option<String>,
    pub message: Option<String>,
}

impl ObservedError {
    pub fn of(doc: &serde_json::Value) -> Option<Self> {
        let (error, nested) = crate::codecs::provider_response::located_error(doc)?;
        let text = |key: &str| error.get(key).and_then(serde_json::Value::as_str);
        let kind = nested.then(|| text("type").or_else(|| text("status")));
        Some(Self {
            code: error.get("code").filter(|code| !code.is_null()).cloned(),
            kind: kind.flatten().map(str::to_owned),
            message: error
                .as_str()
                .or_else(|| text("message"))
                .map(str::to_owned),
        })
    }

    pub fn emit(self, sink: &mut ObservationSink<'_>) {
        let code = self.code.map(|code| match code {
            serde_json::Value::String(code) => sink.scrub(&code),
            serde_json::Value::Number(code) => code.to_string(),
            _ => "[invalid]".to_owned(),
        });
        sink.error(AdapterErrorEnvelope {
            code,
            status: self.kind.map(|value| sink.scrub(&value)),
            message: self.message.map(|value| sink.scrub(&value)),
        });
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdapterUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub tool_input_tokens: Option<u64>,
}

impl From<&crate::codecs::completion::Usage> for AdapterUsage {
    fn from(usage: &crate::codecs::completion::Usage) -> Self {
        // Exhaustive: a new `Usage` counter must be mapped or ignored here.
        let crate::codecs::completion::Usage {
            input_tokens,
            output_tokens,
            total_tokens: _,
            cached_input_tokens,
            cache_creation_input_tokens: _,
            tool_use_prompt_tokens,
            reasoning_tokens,
            cost: _,
        } = *usage;
        Self {
            input_tokens,
            output_tokens,
            total_tokens: None,
            cached_input_tokens,
            reasoning_tokens,
            tool_input_tokens: tool_use_prompt_tokens,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterErrorBoundary {
    Request,
    ProviderResponse,
    Decode,
    Transport,
    #[default]
    Unknown,
}

impl AdapterErrorBoundary {
    pub fn from_http(error: &crate::codecs::http_client::Error) -> Self {
        use crate::codecs::http_client::Error as H;
        match error {
            H::Protocol(_) | H::InvalidHeaderValue(_) | H::NoHeaders | H::Unbuilt(_) => {
                Self::Request
            }
            H::InvalidContentType(_) => Self::Decode,
            H::StreamEnded => Self::Transport,
            H::InvalidStatusCodeWithDetails { .. } => Self::ProviderResponse,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "ending", rename_all = "snake_case")]
pub enum AdapterEnding {
    Decoded,
    Error {
        #[serde(default)]
        boundary: AdapterErrorBoundary,
        kind: String,
        status: Option<u16>,
        retryable: bool,
    },
    Terminal,
    Eof {
        after: usize,
    },
    PartialFrame {
        byte_count: usize,
        after: usize,
    },
    Dropped,
}

#[derive(Clone)]
pub struct AdapterContext {
    inner: Arc<AdapterContextInner>,
}

struct AdapterContextInner {
    sink: Arc<dyn Witness + Send + Sync>,
    subject: Subject,
    operation: String,
    next: Arc<Mutex<Option<u64>>>,
    host_attempt: Option<std::num::NonZeroU64>,
}

impl std::fmt::Debug for AdapterContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Caller-supplied identity can contain sensitive data; do not log it.
        f.debug_struct("AdapterContext").finish_non_exhaustive()
    }
}

impl AdapterContext {
    pub fn operation(&self) -> &str {
        &self.inner.operation
    }

    pub fn new(
        sink: Arc<dyn Witness + Send + Sync>,
        subject: Subject,
        operation: impl Into<String>,
    ) -> Self {
        Self {
            inner: Arc::new(AdapterContextInner {
                sink,
                subject,
                operation: operation.into(),
                next: Arc::new(Mutex::new(Some(1))),
                host_attempt: None,
            }),
        }
    }

    pub fn for_host_attempt(&self, subject: Subject, attempt: std::num::NonZeroU64) -> Self {
        Self {
            inner: Arc::new(AdapterContextInner {
                sink: self.inner.sink.clone(),
                subject,
                operation: self.inner.operation.clone(),
                next: self.inner.next.clone(),
                host_attempt: Some(attempt),
            }),
        }
    }

    pub fn attempt_for<B>(
        &self,
        request: &http::Request<B>,
        route: &str,
        request_id_header: Option<&'static str>,
    ) -> Option<AdapterAttempt> {
        let mut attempt = self.begin(request.method(), route)?;
        attempt.secrets = scrub::request_secrets(request);
        attempt.request_id_header = request_id_header;
        Some(attempt)
    }

    fn emit(&self, attempt: Option<u64>, event: AdapterEvent) {
        self.emit_with_analysis(attempt, event, None);
    }

    fn emit_with_analysis(
        &self,
        attempt: Option<u64>,
        event: AdapterEvent,
        analysis: Option<AdapterAnalysis>,
    ) {
        self.inner.sink.observe(Observation::new(
            self.inner.subject.clone(),
            Stage::Handler,
            Emitter::named("swarmie-ai/adapter"),
            Action::Adapter {
                observation: AdapterObservation {
                    operation: self.inner.operation.clone(),
                    attempt,
                    host_attempt: self.inner.host_attempt,
                    event,
                    analysis,
                },
            },
        ));
    }

    pub fn begin(&self, method: &http::Method, route: &str) -> Option<AdapterAttempt> {
        let number = {
            let mut next = self
                .inner
                .next
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let number = (*next)?;
            *next = number.checked_add(1);
            number
        };
        self.emit(
            Some(number),
            AdapterEvent::Started {
                method: method.to_string(),
                route: route.to_owned(),
            },
        );
        if number == u64::MAX {
            self.emit(None, AdapterEvent::IdentityExhausted);
        }
        Some(AdapterAttempt {
            context: self.clone(),
            number,
            closed: false,
            response_seen: false,
            unframed: 0,
            secrets: Vec::new(),
            request_id_header: None,
            pending_response_id: None,
            error_boundary: None,
        })
    }
}

pub struct AdapterAttempt {
    context: AdapterContext,
    number: u64,
    closed: bool,
    response_seen: bool,
    unframed: usize,
    secrets: Vec<String>,
    request_id_header: Option<&'static str>,
    pending_response_id: Option<String>,
    error_boundary: Option<AdapterErrorBoundary>,
}

impl AdapterAttempt {
    pub fn text(&self, text: &str) -> String {
        scrub::text(text, &self.secrets)
    }

    fn emit_with_analysis(&self, event: AdapterEvent, analysis: AdapterAnalysis) {
        let analysis = (analysis != AdapterAnalysis::default()).then_some(analysis);
        self.context
            .emit_with_analysis(Some(self.number), event, analysis);
    }

    fn emit(&self, event: AdapterEvent) {
        self.context.emit(Some(self.number), event);
    }

    pub fn project(&mut self, project: impl FnOnce(&mut ObservationSink<'_>)) {
        project(&mut ObservationSink { attempt: self });
    }

    pub fn provider(&mut self, verdict: AdapterVerdict, response_id: Option<String>) {
        if response_id.is_some() {
            self.pending_response_id = response_id;
        }
        // An ID-only payload must not manufacture a semantic provider event.
        // Retain at most one ID until the next verdict or the attempt closure.
        if verdict != AdapterVerdict::default() {
            let response_id = self.pending_response_id.take();
            self.emit_with_analysis(
                AdapterEvent::Provider { verdict },
                AdapterAnalysis {
                    response_id,
                    ..AdapterAnalysis::default()
                },
            );
        }
    }

    pub fn response_with_headers(
        &mut self,
        status: http::StatusCode,
        headers: Option<&http::HeaderMap>,
    ) {
        if !self.response_seen {
            self.response_seen = true;
            self.emit_with_analysis(
                AdapterEvent::Response {
                    status: status.as_u16(),
                },
                AdapterAnalysis {
                    headers: headers
                        .map(|h| scrub::headers(h, &self.secrets, self.request_id_header)),
                    ..AdapterAnalysis::default()
                },
            );
        }
    }

    pub fn finish(&mut self, ending: AdapterEnding) {
        if !self.closed {
            self.closed = true;
            let response_id = self.pending_response_id.take();
            self.emit_with_analysis(
                AdapterEvent::Finished { ending },
                AdapterAnalysis {
                    response_id,
                    ..AdapterAnalysis::default()
                },
            );
        }
    }
}

impl Drop for AdapterAttempt {
    fn drop(&mut self) {
        self.finish(AdapterEnding::Dropped);
    }
}

pub struct ObservationSink<'a> {
    attempt: &'a mut AdapterAttempt,
}

impl ObservationSink<'_> {
    pub fn error(&mut self, error: AdapterErrorEnvelope) {
        self.attempt.emit(AdapterEvent::ErrorEnvelope { error });
    }

    pub fn provider(&mut self, verdict: AdapterVerdict, response_id: Option<String>) {
        self.attempt.provider(verdict, response_id);
    }

    pub fn scrub(&self, value: &str) -> String {
        self.attempt.text(value)
    }
}

#[derive(Clone, Default)]
pub struct AdapterSlot(Arc<Mutex<Option<AdapterAttempt>>>);

impl AdapterSlot {
    pub fn transport_eof(&self, after: usize) {
        if let Some(attempt) = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            attempt.emit(AdapterEvent::TransportEof {
                after,
                partial_bytes: attempt.unframed,
            });
        }
    }

    pub fn project(&self, project: impl FnOnce(&mut ObservationSink<'_>)) {
        if let Some(attempt) = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()
        {
            attempt.project(project);
        }
    }

    pub fn terminal(&self, usage: Option<&crate::codecs::completion::Usage>) {
        let mut attempt = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(attempt) = attempt.as_mut().filter(|attempt| !attempt.closed) {
            let usage = usage.map(AdapterUsage::from);
            if let Some(usage) = usage.filter(|usage| *usage != AdapterUsage::default()) {
                attempt.emit(AdapterEvent::Usage { usage });
            }
            attempt.finish(AdapterEnding::Terminal);
        }
    }

    pub fn install(&self, attempt: Option<AdapterAttempt>) {
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = attempt;
    }

    pub fn response(&self, status: http::StatusCode) {
        self.response_with_headers(status, None);
    }

    pub fn response_with_headers(
        &self,
        status: http::StatusCode,
        headers: Option<&http::HeaderMap>,
    ) {
        if let Some(attempt) = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()
        {
            attempt.response_with_headers(status, headers);
        }
    }

    fn finish(&self, ending: AdapterEnding) {
        if let Some(attempt) = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()
        {
            attempt.finish(ending);
        }
    }

    pub fn error_boundary(&self, boundary: AdapterErrorBoundary) {
        if let Some(attempt) = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()
        {
            attempt.error_boundary = Some(boundary);
        }
    }

    pub fn fail(&self, error: &crate::codecs::error::ProviderError) {
        if let Some(status) = error.provider_response_status() {
            self.response(status);
        }
        let report = error.report();
        let boundary = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .and_then(|attempt| attempt.error_boundary)
            .unwrap_or_else(|| error.boundary());
        self.finish(AdapterEnding::Error {
            boundary,
            kind: report.kind.code().to_owned(),
            status: report.http_status,
            retryable: report.is_retryable(),
        });
    }

    pub fn unframed(&self, bytes: usize) {
        if let Some(attempt) = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()
        {
            attempt.unframed = bytes;
        }
    }

    pub fn eof(&self, after: usize) {
        if let Some(attempt) = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()
        {
            let byte_count = attempt.unframed;
            let ending = if byte_count == 0 {
                AdapterEnding::Eof { after }
            } else {
                AdapterEnding::PartialFrame { byte_count, after }
            };
            attempt.finish(ending);
        }
    }

    pub fn corrupt(&self, frame: usize) {
        if let Some(attempt) = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
        {
            attempt
                .context
                .emit(Some(attempt.number), AdapterEvent::Corrupt { frame });
        }
    }
}

mod scrub;
