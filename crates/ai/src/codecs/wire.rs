use std::sync::{Arc, Mutex};

use crate::codecs::error::{EncodeError, ProviderError};
use crate::codecs::http_client::MultipartForm;
use crate::codecs::wasm_compat::{WasmCompatSend, WasmCompatSync};

pub use crate::codecs::http_client::framing::{Framing, WireFrame};
pub use crate::codecs::observe::{
    AdapterErrorEnvelope, AdapterEvent, AdapterUsage, AdapterVerdict, ObservationSink,
};

mod citation;
pub mod document;
pub(crate) mod secret;

pub use citation::{SpanUnit, WireCitation, WireSpan};
pub use secret::Secret;

pub struct Encoded {
    pub request: http::Request<Body>,
    pub framing: Framing,
    pub request_id_header: Option<&'static str>,
    pub relaxed_content_type: bool,
    pub route: Option<&'static str>,
    pub project: Option<Projector>,
    pub analysis_only: Option<fn(&WireFrame) -> bool>,
}

pub type Projector = fn(&[u8], &mut ObservationSink<'_>);

impl Encoded {
    pub fn new(request: http::Request<Body>, framing: Framing) -> Self {
        Self {
            request,
            framing,
            request_id_header: None,
            relaxed_content_type: false,
            route: None,
            project: None,
            analysis_only: None,
        }
    }

    pub fn with_request_id_header(mut self, header: Option<&'static str>) -> Self {
        self.request_id_header = header;
        self
    }

    pub fn with_relaxed_content_type(mut self) -> Self {
        self.relaxed_content_type = true;
        self
    }

    pub fn with_route(mut self, route: Option<&'static str>) -> Self {
        self.route = route;
        self
    }

    pub fn with_projection(mut self, project: Projector) -> Self {
        self.project = Some(project);
        self
    }

    pub fn with_analysis_only(mut self, analysis_only: fn(&WireFrame) -> bool) -> Self {
        self.analysis_only = Some(analysis_only);
        self
    }
}

pub enum Body {
    Bytes(Vec<u8>),
    Multipart(MultipartForm),
}

impl Body {
    pub fn empty() -> Self {
        Self::Bytes(Vec::new())
    }
}

impl std::fmt::Debug for Body {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bytes(bytes) => write!(f, "Bytes({} bytes)", bytes.len()),
            Self::Multipart(_) => f.write_str("Multipart"),
        }
    }
}

impl std::fmt::Debug for Encoded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Diagnostics omit query, authority, headers and bodies. A caller's
        // path can still contain sensitive data; it is not scrubbed here.
        f.debug_struct("Encoded")
            .field(
                "request",
                &(self.request.method(), self.request.uri().path()),
            )
            .field("framing", &self.framing)
            .field("request_id_header", &self.request_id_header)
            .field("relaxed_content_type", &self.relaxed_content_type)
            .field("route", &self.route)
            .field("project", &self.project.is_some())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Unary,
    Streaming,
}

pub trait Operation: Sized + 'static {
    type Request: WasmCompatSend + 'static;
    type Event: WasmCompatSend + 'static;
    type End: WasmCompatSend + 'static;
    type Response: WasmCompatSend + 'static;
    type Fold: Fold<Self> + WasmCompatSend + 'static;
    type Emit: Emit<Self>;

    fn fold(request: &Self::Request, call: &mut Call<'_>) -> Self::Fold;

    fn prepare(
        request: Self::Request,
        wire: &Descriptor<'_>,
    ) -> Result<Self::Request, ProviderError> {
        let _ = wire;
        Ok(request)
    }

    fn observed_usage(end: &Self::End) -> Option<&crate::codecs::completion::Usage> {
        let _ = end;
        None
    }
}

#[derive(Clone, Debug, Default)]
pub struct Prepared(pub(crate) Arc<Mutex<crate::codecs::completion::history::Memo>>);

impl PartialEq for Prepared {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Free {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assembled {}

pub trait Emit<Op: Operation>: reply::Closing<Op> {}

impl<Op: Operation> reply::Closing<Op> for Free {
    fn close(_shared: &mut Shared<Op>) {}
}

impl<Op: Operation> Emit<Op> for Free {}

pub struct Call<'a> {
    pub wire: &'a Descriptor<'a>,
    pub mode: Mode,
    pub(crate) span: tracing::Span,
}

impl<'a> Call<'a> {
    pub(crate) fn new(wire: &'a Descriptor<'a>, mode: Mode) -> Self {
        Self {
            wire,
            mode,
            span: tracing::Span::none(),
        }
    }

    pub fn instrument(&mut self, span: tracing::Span) {
        self.span = span;
    }
}

pub trait Fold<Op: Operation> {
    fn absorb(&mut self, event: &Op::Event) -> Result<(), ProviderError>;

    fn finish(self, end: Op::End, reply: Reply) -> Result<Op::Response, ProviderError>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub provider: String,
    pub raw: serde_json::Value,
    pub provider_request_id: Option<String>,
}

#[derive(Debug)]
#[must_use]
pub enum Flow {
    More,
    Ended(Ended),
}

#[derive(Debug)]
pub struct Ended(());

pub(crate) use reply::Shared;

pub(crate) mod reply {
    use std::collections::VecDeque;

    use super::{Fold, Operation, ProviderError, Reply};
    use crate::codecs::streaming::Item;

    pub struct Shared<Op: Operation> {
        pub(crate) fold: Op::Fold,
        pub(crate) items: VecDeque<Result<Item<Op::Event>, ProviderError>>,
        pub(crate) end: Option<Op::End>,
        pub(crate) raw: Option<serde_json::Value>,
        pub(crate) response: Option<Op::Response>,
        pub(crate) request_id: Option<String>,
        pub(crate) document: Option<serde_json::Value>,
    }

    impl<Op: Operation> Shared<Op> {
        pub(crate) fn new(fold: Op::Fold) -> Self {
            Self {
                fold,
                items: VecDeque::new(),
                end: None,
                raw: None,
                response: None,
                request_id: None,
                document: None,
            }
        }

        pub(crate) fn take(&mut self) -> Option<Result<Item<Op::Event>, ProviderError>> {
            Some(match self.items.pop_front()? {
                Ok(Item::Event(event)) => self.fold.absorb(&event).map(|()| Item::Event(event)),
                Ok(unknown) => Ok(unknown),
                // An id an upstream constructor already attached wins: it
                // saw the reply.
                Err(error) => Err(error.with_provider_request_id(self.request_id.clone())),
            })
        }

        pub(crate) fn conclude(self, provider: &str) -> Result<Op::Response, ProviderError> {
            if let Some(response) = self.response {
                return Ok(response);
            }
            let reply = reply(provider, self.document.or(self.raw), self.request_id);
            let end = self.end.ok_or(ProviderError::Truncated)?;
            self.fold.finish(end, reply)
        }
    }

    fn reply(provider: &str, raw: Option<serde_json::Value>, request_id: Option<String>) -> Reply {
        Reply {
            provider: provider.to_owned(),
            raw: raw.unwrap_or(serde_json::Value::Null),
            provider_request_id: request_id,
        }
    }

    pub trait Closing<Op: Operation> {
        fn close(shared: &mut Shared<Op>);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capabilities {
    pub completion: crate::codecs::completion::ProviderCapabilities,
    pub max_documents: usize,
    pub ndims: usize,
    pub declared: Option<usize>,
}

impl Capabilities {
    pub const fn completion(completion: crate::codecs::completion::ProviderCapabilities) -> Self {
        Self {
            completion,
            ..Self::embedding(0, 0)
        }
    }

    pub const fn embedding(max_documents: usize, ndims: usize) -> Self {
        Self {
            completion: crate::codecs::completion::ProviderCapabilities::new(),
            max_documents,
            ndims,
            declared: None,
        }
    }

    pub const fn rerank(max_documents: usize) -> Self {
        Self::embedding(max_documents, 0)
    }

    pub const fn declaring(mut self, declared: Option<usize>) -> Self {
        self.declared = declared;
        self
    }

    pub(crate) fn honour_declaration(
        &self,
        provider: &str,
        widths: impl IntoIterator<Item = usize>,
    ) -> Result<(), ProviderError> {
        // Zero is the sentinel for an unknown width, never a claim about
        // one: a model absent from every table this build knows resolves to
        // it, and treating that as a declaration would fail every reply.
        let Some(requested) = self.declared.filter(|declared| *declared > 0) else {
            return Ok(());
        };
        let Some(returned) = widths.into_iter().find(|width| *width != requested) else {
            return Ok(());
        };
        Err(ProviderError::MismatchedDimensions {
            provider: provider.to_owned(),
            requested,
            returned,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Descriptor<'a> {
    pub replay: Option<&'a dyn crate::codecs::completion::ReplayTarget>,
    pub name: &'a str,
    pub model: Option<&'a str>,
    pub capabilities: Capabilities,
    pub telemetry: Option<fn(Mode) -> crate::codecs::telemetry::GenAiOperation>,
    pub facts: Option<&'a crate::codecs::catalog::ModelFacts>,
    pub prepared: Option<&'a Prepared>,
}

impl<'a> Descriptor<'a> {
    pub fn new(name: &'a str) -> Self {
        Self {
            replay: None,
            name,
            model: None,
            capabilities: Capabilities::default(),
            telemetry: None,
            facts: None,
            prepared: None,
        }
    }

    pub fn spec(&self) -> Option<&'a crate::codecs::catalog::ModelSpec> {
        let model = self.model?;
        match self.facts {
            Some(facts) => facts.for_model(self.name, model),
            None => None,
        }
    }

    pub fn replay(mut self, target: &'a dyn crate::codecs::completion::ReplayTarget) -> Self {
        self.replay = Some(target);
        self
    }

    pub fn model(mut self, model: impl Into<Option<&'a str>>) -> Self {
        self.model = model.into();
        self
    }

    pub fn capabilities(mut self, capabilities: Capabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    pub fn facts(mut self, facts: &'a crate::codecs::catalog::ModelFacts) -> Self {
        self.facts = Some(facts);
        self
    }

    pub fn telemetry(
        mut self,
        telemetry: fn(Mode) -> crate::codecs::telemetry::GenAiOperation,
    ) -> Self {
        self.telemetry = Some(telemetry);
        self
    }
}

#[derive(Debug)]
pub enum WireEvent<T> {
    Known(T),
    Unknown {
        event_type: String,
        value: crate::codecs::streaming::UnknownPayload,
    },
    Corrupt(serde_json::Error),
}

impl<T> WireEvent<T> {
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> WireEvent<U> {
        match self {
            Self::Known(event) => WireEvent::Known(f(event)),
            Self::Unknown { event_type, value } => WireEvent::Unknown { event_type, value },
            Self::Corrupt(error) => WireEvent::Corrupt(error),
        }
    }
}

pub struct Out<'id, Op: Operation> {
    shared: &'id std::sync::Mutex<Shared<Op>>,
    brand: std::marker::PhantomData<fn(&'id ()) -> &'id ()>,
}

impl<'id, Op: Operation> Out<'id, Op> {
    pub(crate) fn new(shared: &'id std::sync::Mutex<Shared<Op>>) -> Self {
        Self {
            shared,
            brand: std::marker::PhantomData,
        }
    }

    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'id, Shared<Op>> {
        self.shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn end(self, end: Op::End) -> Flow {
        let mut shared = self.lock();
        <Op::Emit as reply::Closing<Op>>::close(&mut shared);
        shared.end = Some(end);
        Flow::Ended(Ended(()))
    }

    pub fn unknown(&mut self, payload: crate::codecs::streaming::UnknownPayload) {
        self.lock()
            .items
            .push_back(Ok(crate::codecs::streaming::Item::Unknown(payload)));
    }
}

impl<Op: Operation<Emit = Free>> Out<'_, Op> {
    pub fn raw(&mut self, raw: serde_json::Value) {
        self.lock().raw = Some(raw);
    }

    pub fn event(&mut self, event: Op::Event) {
        self.lock()
            .items
            .push_back(Ok(crate::codecs::streaming::Item::Event(event)));
    }
}

pub trait Decoder<'id, Op: Operation, Frame = WireFrame> {
    type Event;

    fn classify(&self, frame: Frame) -> WireEvent<Self::Event>;

    fn decode(&mut self, event: Self::Event, out: Out<'id, Op>) -> Result<Flow, ProviderError>;

    fn eof(&mut self, out: Out<'id, Op>) -> Result<Flow, ProviderError> {
        let _ = out;
        Err(ProviderError::Truncated)
    }
}

pub trait Wire: Clone + WasmCompatSend + WasmCompatSync + 'static {
    type Op: Operation;
    type Payload: WasmCompatSend + 'static;
    type Frame: WasmCompatSend + 'static;
    type Event: WasmCompatSend + 'static;
    type Decoder<'id>: Decoder<'id, Self::Op, Self::Frame, Event = Self::Event> + WasmCompatSend;
    type Reassembler: document::Reassemble<Self::Event> + document::Serves<Self::Op>;

    fn describe(&self) -> Descriptor<'_>;

    fn encode(&self, request: Request<Self>, mode: Mode) -> Result<Self::Payload, EncodeError>;

    fn decoder<'id>(&self) -> Self::Decoder<'id>;

    fn reassembler(&self) -> Self::Reassembler {
        Self::Reassembler::default()
    }
}

pub fn decode_reply<W: Wire>(
    wire: &W,
    request: &Request<W>,
    mode: Mode,
    frames: impl IntoIterator<Item = W::Frame>,
) -> Result<Response<W>, ProviderError> {
    crate::codecs::driver::decode_reply(wire, request, mode, frames, serde_json::Value::Null)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Json;

impl<'id, Op> Decoder<'id, Op, WireFrame> for Json
where
    Op: Operation<Emit = Free>,
    Op::End: serde::de::DeserializeOwned,
{
    type Event = (Op::End, serde_json::Value);

    fn classify(&self, frame: WireFrame) -> WireEvent<Self::Event> {
        let document = match serde_json::from_str::<serde_json::Value>(&frame.as_str()) {
            Ok(document) => document,
            Err(error) => return WireEvent::Corrupt(error),
        };
        match <Op::End as serde::Deserialize>::deserialize(&document) {
            Ok(end) => WireEvent::Known((end, document)),
            Err(error) => WireEvent::Corrupt(error),
        }
    }

    fn decode(
        &mut self,
        (end, document): Self::Event,
        mut out: Out<'id, Op>,
    ) -> Result<Flow, ProviderError> {
        out.raw(document);
        Ok(out.end(end))
    }
}

pub struct DocumentDecoder<Op: Operation, Event> {
    classify: fn(&str) -> WireEvent<Event>,
    end: fn(Event) -> Result<Op::End, ProviderError>,
}

impl<Op: Operation, Event> DocumentDecoder<Op, Event> {
    pub fn new(
        classify: fn(&str) -> WireEvent<Event>,
        end: fn(Event) -> Result<Op::End, ProviderError>,
    ) -> Self {
        Self { classify, end }
    }
}

impl<'id, Op: Operation, Event> Decoder<'id, Op> for DocumentDecoder<Op, Event> {
    type Event = Event;

    fn classify(&self, frame: WireFrame) -> WireEvent<Event> {
        (self.classify)(&frame.as_str())
    }

    fn decode(&mut self, event: Event, out: Out<'id, Op>) -> Result<Flow, ProviderError> {
        Ok(out.end((self.end)(event)?))
    }
}

pub type Request<W> = <<W as Wire>::Op as Operation>::Request;
pub type Response<W> = <<W as Wire>::Op as Operation>::Response;
pub type Event<W> = <<W as Wire>::Op as Operation>::Event;
