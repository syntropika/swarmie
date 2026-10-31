use std::collections::{BTreeMap, HashMap, HashSet};

use crate::codecs::completion::{CompletionRequest, CompletionResponse, FinishReason, Usage};
use crate::codecs::error::ProviderError;
use crate::codecs::message::citation;
use crate::codecs::message::{
    Api, AssistantContent, CallId, Image, LocalCallId, Opaque, Origin, Reasoning, Text, ToolCall,
    ToolFunction, ToolName,
};
use crate::codecs::streaming::{Item, Part, PartKind, StreamEvent};
use crate::codecs::telemetry::{GenAiOperation, SpanBuilder, SpanCombinator};
use crate::codecs::wire::{
    Assembled, Call, Descriptor, Emit, Fold, Mode, Operation, Out, Reply, Shared, WireCitation,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Completion;

impl Operation for Completion {
    type Request = CompletionRequest;
    type Event = StreamEvent;
    type End = Finish;
    type Response = CompletionResponse;
    type Fold = ReplyFold;
    type Emit = Assembled;

    fn fold(request: &Self::Request, call: &mut Call<'_>) -> Self::Fold {
        let telemetry = call
            .wire
            .telemetry
            .map_or(GenAiOperation::Chat, |telemetry| telemetry(call.mode));
        debug_assert!(telemetry.is_completion());
        // A wire that picks its API per request records the one it used.
        let replay = call
            .wire
            .replay
            .map(|target| target.route(request).unwrap_or(target));
        let model = request
            .model
            .as_deref()
            .or(call.wire.model)
            .or_else(|| replay.map(|target| target.model()))
            .unwrap_or_default();
        let span = SpanBuilder::new(call.wire.name, model, telemetry)
            .streaming(matches!(call.mode, Mode::Streaming))
            .system_instructions(
                request.system_instructions(),
                request.record_telemetry_content,
            )
            .build();
        call.instrument(span.clone());
        let api = replay.map_or_else(
            || Api::from(call.wire.name.to_owned()),
            |target| target.api(),
        );
        let mut origin = Origin::new(api, call.wire.name, model);
        // Only a wire that binds items to the context needs it recorded.
        if replay.is_some_and(|target| target.binds_context(model)) {
            origin.context = replay.map(|target| {
                crate::codecs::completion::history::context_of(request, target, model)
            });
        }
        ReplyFold {
            span,
            facts: call.wire.facts.cloned().unwrap_or_default(),
            wire: replay.is_some_and(|target| target.states_finish_reason()),
            call_id_slot: replay.and_then(|target| target.call_id_slot()),
            catalog_priced: replay.map(|target| target.catalog_priced()),
            blob_slots: replay.map_or(&[], |target| target.blob_slots()),
            accept_unknown_finish: request.accept_unknown_finish_reasons,
            ..ReplyFold::new(origin)
        }
    }

    fn observed_usage(finish: &Finish) -> Option<&crate::codecs::completion::Usage> {
        Some(&finish.usage)
    }

    fn prepare(
        mut request: Self::Request,
        wire: &Descriptor<'_>,
    ) -> Result<Self::Request, ProviderError> {
        let Some(target) = wire.replay else {
            return Err(ProviderError::request(format!(
                "completion wire `{}` names no replay target",
                wire.name
            )));
        };
        // The caller's history is checked as written, so a rejection names
        // its own messages; adapting never empties a message it keeps.
        request.validate_message_content()?;
        // A wire that addresses no model (an interaction read back) leaves
        // it to the reply, which names the model the turn is from.
        request.model = request
            .model
            .take()
            .filter(|model| !model.is_empty())
            .or_else(|| Some(target.model().to_owned()).filter(|model| !model.is_empty()));
        let target = target.route(&request).unwrap_or(target);
        // Every option the routed wire cannot honour is reported here, before
        // any wire encodes, whatever its encoder does.
        crate::codecs::completion::options::check(target, &mut request)?;
        // Message cache points name the history as written; they are moved
        // with it once it is adapted.
        let written = request.cache_points.iter().any(|point| {
            matches!(
                point,
                crate::codecs::completion::CachePoint::Messages(_)
                    | crate::codecs::completion::CachePoint::Within { .. }
            )
        });
        let written = written.then(|| {
            let documents = (!target.takes_documents())
                .then(|| request.normalized_documents())
                .flatten();
            (request.chat_history.clone(), documents)
        });
        let stored = target.continues_stored(&request);
        let model = request.model.as_deref().unwrap_or(target.model());
        // Only a target that binds items to the context reads it.
        let binds = target.binds_context(model) && !target.drops_unbound_items(&request);
        let shape = crate::codecs::completion::history::Request {
            model: request.model.as_deref(),
            stored,
            foreign_reasoning: request.foreign_reasoning,
            // A conversation the provider stores holds its own tools, so a
            // continuation that declares none still calls them.
            tools: stored || target.declares_tools(&request),
            context: binds
                .then(|| crate::codecs::completion::history::context_of(&request, target, model)),
        };
        // The history moves into the adapter. Documents join it first, so
        // the adapter's rules apply to them, unless the encoder sends them
        // itself.
        let mut history = std::mem::take(&mut request.chat_history);
        if !target.takes_documents() {
            if let Some(documents) = request.normalized_documents() {
                crate::codecs::completion::request::insert_after_leading_system(
                    &mut history,
                    documents,
                );
            }
            request.documents.clear();
        }
        // What the model's last request left; another request waits for it.
        let mut kept = wire.prepared.and_then(|prepared| prepared.0.lock().ok());
        let mut fresh = Default::default();
        let memo = kept.as_deref_mut().unwrap_or(&mut fresh);
        let adapted = crate::codecs::completion::history::adapt_for(history, target, &shape, memo);
        drop(kept);
        if let Some((written, documents)) = written {
            request.cache_points = crate::codecs::completion::history::cache_points(
                &request.cache_points,
                &written,
                documents.as_ref(),
                &adapted,
                target,
                &shape,
            );
        }
        request.chat_history = adapted;
        request.validate_message_content()?;
        Ok(request)
    }
}

impl crate::codecs::wire::reply::Closing<Completion> for Assembled {
    fn close(shared: &mut Shared<Completion>) {
        let Shared { fold, items, .. } = shared;
        fold.close_open(items);
    }
}

impl Emit<Completion> for Assembled {}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Finish {
    pub usage: Usage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<FinishReason>,
    pub response_id: Option<String>,
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Text,
    Reasoning { redacted: bool },
    Image(Image),
    Call { id: CallId, name: ToolName },
    Opaque { replay: bool },
}

pub struct ReplyFold {
    span: tracing::Span,
    origin: Origin,
    facts: crate::codecs::catalog::ModelFacts,
    // The writer.
    open: BTreeMap<usize, Draft>,
    ended: HashMap<usize, usize>,
    next_part: u32,
    call_ids: HashSet<CallId>,
    wire: bool,
    call_id_slot: Option<&'static str>,
    catalog_priced: Option<bool>,
    blob_slots: &'static [&'static str],
    accept_unknown_finish: bool,
    first_incomplete: Option<usize>,
    unfinished: Vec<CallId>,
    last_call: Option<usize>,
    run: Option<usize>,
    next_auto: usize,
    by_index: Option<BTreeMap<usize, usize>>,
    lead: Option<usize>,
    // The fold.
    choice: Vec<Option<AssistantContent>>,
    open_text: BTreeMap<usize, AssistantContent>,
}

struct Draft {
    part: Part,
    started: bool,
    item: serde_json::Value,
    body: Body,
    citations: Vec<WireCitation>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Closing {
    Complete,
    Incomplete,
}

enum Body {
    Text(String),
    Image(Image),
    Reasoning {
        text: String,
        redacted: bool,
    },
    Call {
        id: Option<CallId>,
        name: String,
        arguments: Arguments,
    },
    Opaque {
        replay: bool,
    },
}

impl Body {
    fn kind(&self) -> PartKind {
        match self {
            Self::Text(_) => PartKind::Text,
            Self::Reasoning { .. } => PartKind::Reasoning,
            Self::Image(_) => PartKind::Image,
            Self::Call { .. } => PartKind::ToolCall,
            Self::Opaque { .. } => PartKind::Opaque,
        }
    }
}

#[derive(Default)]
struct Arguments {
    text: String,
    streamed: usize,
    overflowed: bool,
    substantive: bool,
    announced: Option<serde_json::Value>,
}

const MAX_TOOL_INPUT_BYTES: usize = 32 * 1024 * 1024;

impl Arguments {
    fn push(&mut self, fragment: &str, name: &str) {
        self.substantive |= !fragment.trim().is_empty();
        // Some OpenAI-compatible gateways send a literal `null` before the
        // real fragments; a non-blank fragment supersedes it.
        if self.text.trim() == "null" && !fragment.trim().is_empty() {
            self.text.clear();
        }
        if self.text.len().saturating_add(fragment.len()) > MAX_TOOL_INPUT_BYTES {
            if !self.overflowed {
                self.overflowed = true;
                tracing::warn!(
                    tool = name,
                    "streamed tool-call input exceeded the accumulation bound; truncating"
                );
            }
        } else {
            self.text.push_str(fragment);
        }
    }

    fn function(&self, name: ToolName) -> ToolFunction {
        if self.text.is_empty()
            && let Some(announced) = &self.announced
        {
            return ToolFunction::new(name, announced.clone());
        }
        let mut function = ToolFunction::parse(name, &self.text);
        if self.overflowed {
            function.invalid_arguments = Some(self.text.clone());
        }
        function
    }

    fn unstreamed(&mut self) -> Option<String> {
        if "null".starts_with(self.text.trim()) {
            return None;
        }
        let fragment = self.text.get(self.streamed..)?;
        if fragment.is_empty() {
            return None;
        }
        let fragment = fragment.to_owned();
        self.streamed = self.text.len();
        Some(fragment)
    }

    fn complete(&self) -> bool {
        !self.overflowed && crate::codecs::json_utils::is_whole_object(&self.text)
    }
}

fn stated(id: &str) -> bool {
    !id.is_empty() && id != "null"
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CallFragment<'a> {
    pub id: Option<&'a str>,
    pub name: Option<&'a str>,
    pub arguments: Option<&'a str>,
}

pub const AUTO_INDEX: usize = 1 << 30;

fn not_open(index: usize) -> ProviderError {
    ProviderError::Response(format!(
        "the reply wrote to item {index}, which is not open"
    ))
}

fn stream_call(items: &mut Items, draft: &mut Draft) {
    let Body::Call {
        name, arguments, ..
    } = &mut draft.body
    else {
        return;
    };
    if !draft.started {
        let Ok(name) = ToolName::new(name.clone()) else {
            return;
        };
        draft.started = true;
        emit(
            items,
            StreamEvent::Start {
                part: draft.part,
                kind: PartKind::ToolCall,
                name: Some(name),
            },
        );
    }
    if let Some(json) = arguments.unstreamed() {
        emit(
            items,
            StreamEvent::Arguments {
                part: draft.part,
                json,
            },
        );
    }
}

impl ReplyFold {
    pub(crate) fn new(origin: Origin) -> Self {
        Self {
            span: tracing::Span::none(),
            origin,
            facts: crate::codecs::catalog::ModelFacts::default(),
            open: BTreeMap::new(),
            ended: HashMap::new(),
            next_part: 0,
            call_ids: HashSet::new(),
            wire: false,
            call_id_slot: None,
            catalog_priced: None,
            blob_slots: &[],
            accept_unknown_finish: false,
            first_incomplete: None,
            unfinished: Vec::new(),
            last_call: None,
            run: None,
            next_auto: AUTO_INDEX,
            by_index: None,
            lead: None,
            choice: Vec::new(),
            open_text: BTreeMap::new(),
        }
    }

    fn pricing(&self, origin: &Origin) -> Option<crate::codecs::catalog::Pricing> {
        let priced = self.catalog_priced.unwrap_or_else(|| {
            crate::codecs::providers::registry::ProviderId::catalog(&origin.provider)
                .is_none_or(|id| id.catalog_priced())
        });
        if !priced {
            return None;
        }
        self.facts
            .for_model(&origin.provider, &origin.model)?
            .pricing
    }

    fn next(&mut self) -> Part {
        let part = Part::new(self.next_part);
        self.next_part += 1;
        part
    }

    fn fresh_index(&mut self) -> usize {
        let index = self.next_auto;
        self.next_auto += 1;
        index
    }

    fn draft(&mut self, index: usize) -> Result<&mut Draft, ProviderError> {
        self.open.get_mut(&index).ok_or_else(|| not_open(index))
    }

    pub(crate) fn open_item(
        &mut self,
        items: &mut Items,
        index: usize,
        block: Block,
        item: serde_json::Value,
    ) -> Result<(), ProviderError> {
        if self.open.contains_key(&index) {
            return Err(ProviderError::Response(format!(
                "the reply opened item {index} twice"
            )));
        }
        let body = match block {
            Block::Text => Body::Text(String::new()),
            Block::Image(image) => Body::Image(image),
            Block::Reasoning { redacted } => Body::Reasoning {
                text: String::new(),
                redacted,
            },
            Block::Call { id, name } => Body::Call {
                id: Some(id),
                name: name.into(),
                arguments: Arguments::default(),
            },
            Block::Opaque { replay } => Body::Opaque { replay },
        };
        self.insert(index, body, item);
        stream_call(items, self.draft(index)?);
        Ok(())
    }

    fn insert(&mut self, index: usize, body: Body, item: serde_json::Value) {
        self.ended.remove(&index);
        let part = self.next();
        if let Some(by_index) = &mut self.by_index {
            by_index.insert(part.index(), index);
        }
        let started = false;
        self.open.insert(
            index,
            Draft {
                part,
                started,
                item,
                body,
                citations: Vec::new(),
            },
        );
    }

    pub(crate) fn push_item(
        &mut self,
        items: &mut Items,
        index: usize,
        fragment: &str,
    ) -> Result<(), ProviderError> {
        if fragment.is_empty() {
            return Ok(());
        }
        let draft = self.draft(index)?;
        let event = match &mut draft.body {
            Body::Text(text) => {
                text.push_str(fragment);
                StreamEvent::Text {
                    part: draft.part,
                    text: fragment.to_owned(),
                }
            }
            Body::Reasoning { text, .. } => {
                text.push_str(fragment);
                StreamEvent::Reasoning {
                    part: draft.part,
                    text: fragment.to_owned(),
                }
            }
            Body::Call {
                name, arguments, ..
            } => {
                arguments.push(fragment, name);
                stream_call(items, draft);
                return Ok(());
            }
            Body::Image(image) => {
                if let crate::codecs::message::DocumentSourceKind::Base64(data) = &mut image.data {
                    data.push_str(fragment);
                    return Ok(());
                }
                return Err(ProviderError::Response(format!(
                    "the reply wrote data to the image item {index}, which holds no base64 data"
                )));
            }
            Body::Opaque { .. } => {
                return Err(ProviderError::Response(format!(
                    "the reply wrote text to the opaque item {index}"
                )));
            }
        };
        if !draft.started {
            draft.started = true;
            emit(
                items,
                StreamEvent::Start {
                    part: draft.part,
                    kind: draft.body.kind(),
                    name: None,
                },
            );
        }
        emit(items, event);
        Ok(())
    }

    fn distinct_call_id(
        &mut self,
        id: Option<CallId>,
        item: serde_json::Value,
    ) -> (CallId, serde_json::Value) {
        let (id, item) = match id {
            Some(id) if !self.call_ids.contains(&id) => (id, item),
            Some(id) => {
                tracing::warn!(%id, "the provider named two tool calls with one id; renaming the second");
                (CallId::Local(LocalCallId::new()), serde_json::Value::Null)
            }
            None if self.call_id_slot.is_some() => (CallId::Local(LocalCallId::new()), item),
            None => (CallId::Local(LocalCallId::new()), serde_json::Value::Null),
        };
        self.call_ids.insert(id.clone());
        (id, item)
    }

    fn close_item(
        &mut self,
        items: &mut Items,
        index: usize,
        closing: Closing,
    ) -> Result<(), ProviderError> {
        self.close_block(items, index, closing).map(drop)
    }

    fn close_block(
        &mut self,
        items: &mut Items,
        index: usize,
        closing: Closing,
    ) -> Result<Option<CallId>, ProviderError> {
        let mut draft = self.open.remove(&index).ok_or_else(|| not_open(index))?;
        // A call streams what it has not yet: its start, if it names a tool,
        // and the argument text since its last fragment.
        stream_call(items, &mut draft);
        if self.run == Some(index) {
            self.run = None;
        }
        self.ended.insert(index, draft.part.index());
        let Draft {
            part,
            started,
            item,
            body,
            citations,
        } = draft;
        let item = match (closing, &body) {
            (Closing::Complete, _) | (Closing::Incomplete, Body::Opaque { .. }) => item,
            (Closing::Incomplete, _) => {
                self.cut_at(part.index());
                serde_json::Value::Null
            }
        };
        let content = match body {
            Body::Text(text) => {
                if text.is_empty() && item.is_null() {
                    return Ok(None);
                }
                // Resolved at close: a provider may cite a text before its
                // last fragment arrives.
                let mut text = Text::new(text);
                citation::attach(
                    &mut text,
                    Vec::new(),
                    citations,
                    &self.origin.provider,
                    index,
                );
                self.with_item(AssistantContent::Text(text), item)
            }
            Body::Reasoning { text, redacted } => {
                if text.is_empty() && !redacted && item.is_null() {
                    return Ok(None);
                }
                let reasoning = Reasoning {
                    text,
                    redacted,
                    native: None,
                };
                self.with_item(AssistantContent::Reasoning(reasoning), item)
            }
            Body::Image(image) => self.with_item(AssistantContent::Image(image), item),
            Body::Opaque { replay } => AssistantContent::Opaque(Opaque {
                item,
                replay: replay && closing == Closing::Complete,
            }),
            Body::Call {
                id,
                name,
                arguments,
            } => {
                let Ok(name) = ToolName::new(name) else {
                    tracing::warn!(
                        index,
                        "the provider closed a tool call without a name; nothing can answer it"
                    );
                    return Ok(None);
                };
                let function = arguments.function(name);
                let (id, item) = self.distinct_call_id(id, item);
                // A call that streamed no text (none arrived, or only a
                // placeholder `null`) states the arguments it ends with.
                if arguments.streamed == 0 {
                    let json = serde_json::Value::Object(function.arguments.clone()).to_string();
                    emit(items, StreamEvent::Arguments { part, json });
                }
                let call = ToolCall::new(id.clone(), function);
                let content = self.with_item(AssistantContent::ToolCall(call), item);
                emit(
                    items,
                    StreamEvent::End {
                        part,
                        content: Box::new(content),
                    },
                );
                return Ok(Some(id));
            }
        };
        if !started {
            emit(
                items,
                StreamEvent::Start {
                    part,
                    kind: kind_of(&content),
                    name: None,
                },
            );
        }
        emit(
            items,
            StreamEvent::End {
                part,
                content: Box::new(content),
            },
        );
        Ok(None)
    }

    fn with_item(&self, block: AssistantContent, item: serde_json::Value) -> AssistantContent {
        if item.is_null() {
            return block;
        }
        let mut block = block.with_native(item);
        if let Some(Some(native)) = block.native_slot() {
            native.find_blob(self.blob_slots);
        }
        block
    }

    fn edit_ended(
        &mut self,
        items: &mut Items,
        index: usize,
        edit: impl FnOnce(&mut serde_json::Value),
    ) -> Result<(), ProviderError> {
        let position = *self.ended.get(&index).ok_or_else(|| not_open(index))?;
        let slots = self.blob_slots;
        let queued = items.iter_mut().find_map(|item| match item {
            Ok(Item::Event(StreamEvent::End { part, content })) if part.index() == position => {
                Some(content.as_mut())
            }
            _ => None,
        });
        let content = match queued {
            Some(content) => Some(content),
            None => self.choice.get_mut(position).and_then(Option::as_mut),
        };
        if let Some(content) = content {
            if let Some(item) = item_of(content) {
                edit(item);
            }
            if let Some(Some(native)) = content.native_slot() {
                native.find_blob(slots);
            }
        }
        Ok(())
    }

    fn cite_item(
        &mut self,
        items: &mut Items,
        index: usize,
        citations: Vec<WireCitation>,
        replace: bool,
    ) {
        let provider = self.origin.provider.as_str();
        if let Some(draft) = self.open.get_mut(&index) {
            if !matches!(draft.body, Body::Text(_)) {
                tracing::warn!(
                    provider,
                    index,
                    "dropped citations of an item that is not text"
                );
            } else if replace {
                draft.citations = citations;
            } else {
                draft.citations.extend(citations);
            }
            return;
        }
        let Some(position) = self.ended.get(&index).copied() else {
            tracing::warn!(
                provider,
                index,
                "dropped citations of an item the reply never opened"
            );
            return;
        };
        let queued = items.iter_mut().find_map(|item| match item {
            Ok(Item::Event(StreamEvent::End { part, content })) if part.index() == position => {
                Some(content.as_mut())
            }
            _ => None,
        });
        let content = match queued {
            Some(content) => Some(content),
            None => self.choice.get_mut(position).and_then(Option::as_mut),
        };
        match content {
            Some(AssistantContent::Text(text)) => {
                let kept = if replace {
                    Vec::new()
                } else {
                    text.citations().to_vec()
                };
                citation::attach(text, kept, citations, provider, index);
            }
            _ => tracing::warn!(
                provider,
                index,
                "dropped citations of an item that is not text"
            ),
        }
    }

    pub(crate) fn run_item(
        &mut self,
        items: &mut Items,
        block: Block,
        fragment: &str,
    ) -> Result<usize, ProviderError> {
        let current = self.run.filter(|index| {
            self.open.get(index).is_some_and(|draft| {
                matches!(
                    (&draft.body, &block),
                    (Body::Text(_), Block::Text)
                        | (
                            Body::Reasoning {
                                redacted: false,
                                ..
                            },
                            Block::Reasoning { redacted: false }
                        )
                )
            })
        });
        let index = match current {
            Some(index) => index,
            None => {
                self.end_run(items)?;
                let index = self.fresh_index();
                self.open_item(items, index, block, serde_json::Value::Null)?;
                self.run = Some(index);
                index
            }
        };
        self.push_item(items, index, fragment)?;
        Ok(index)
    }

    pub(crate) fn end_run(&mut self, items: &mut Items) -> Result<(), ProviderError> {
        match self.run.take() {
            Some(index) => self.close_item(items, index, Closing::Complete),
            None => Ok(()),
        }
    }

    pub(crate) fn write_content(
        &mut self,
        items: &mut Items,
        content: AssistantContent,
    ) -> Result<(), ProviderError> {
        let content = match content {
            AssistantContent::ToolCall(mut call) => {
                let (id, _) = self.distinct_call_id(Some(call.id.clone()), serde_json::Value::Null);
                if id != call.id {
                    call.native = None;
                }
                call.id = id;
                AssistantContent::ToolCall(call)
            }
            content => content,
        };
        let part = self.next();
        let name = match &content {
            AssistantContent::ToolCall(call) => Some(call.function.name.clone()),
            _ => None,
        };
        emit(
            items,
            StreamEvent::Start {
                part,
                kind: kind_of(&content),
                name,
            },
        );
        match &content {
            AssistantContent::Text(text) if !text.text.is_empty() => emit(
                items,
                StreamEvent::Text {
                    part,
                    text: text.text.clone(),
                },
            ),
            AssistantContent::Reasoning(reasoning) if !reasoning.text.is_empty() => emit(
                items,
                StreamEvent::Reasoning {
                    part,
                    text: reasoning.text.clone(),
                },
            ),
            AssistantContent::ToolCall(call) => emit(
                items,
                StreamEvent::Arguments {
                    part,
                    json: call.function.arguments_json(),
                },
            ),
            _ => {}
        }
        emit(
            items,
            StreamEvent::End {
                part,
                content: Box::new(content),
            },
        );
        Ok(())
    }

    pub(crate) fn close_open(&mut self, items: &mut Items) {
        let mut open: Vec<(Part, usize)> = self
            .open
            .iter()
            .map(|(index, draft)| (draft.part, *index))
            .collect();
        open.sort();
        for (_, index) in open {
            let closing = if self.run == Some(index) {
                Closing::Complete
            } else {
                Closing::Incomplete
            };
            match self.close_block(items, index, closing) {
                Ok(Some(call)) if closing == Closing::Incomplete => self.unfinished.push(call),
                Ok(_) => {}
                Err(error) => items.push_back(Err(error)),
            }
        }
    }

    fn cut_at(&mut self, position: usize) {
        self.first_incomplete = Some(
            self.first_incomplete
                .map_or(position, |first| first.min(position)),
        );
    }

    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    fn ordered(
        &self,
        parts: Vec<(usize, Option<AssistantContent>)>,
        canonical_from: Option<usize>,
    ) -> Vec<AssistantContent> {
        let mut parts: Vec<(usize, AssistantContent)> = parts
            .into_iter()
            .filter_map(|(position, part)| {
                let part = part?;
                Some(match canonical_from {
                    Some(first) if position >= first => (position, canonical(part)),
                    _ => (position, part),
                })
            })
            .collect();
        if let Some(by_index) = &self.by_index {
            parts.sort_by_key(|(position, _)| {
                (
                    by_index.get(position).copied().unwrap_or(usize::MAX),
                    *position,
                )
            });
        }
        if let Some(lead) = self.lead {
            parts.sort_by_key(|(position, _)| *position != lead);
        }
        parts.into_iter().map(|(_, part)| part).collect()
    }

    fn response(
        &self,
        end: Finish,
        reply: Reply,
        choice: Vec<AssistantContent>,
    ) -> CompletionResponse {
        let Finish {
            usage,
            reason,
            response_id,
            model,
            error,
        } = end;
        use crate::codecs::provider_response::reported;
        let mut origin = self.origin.clone();
        let response_model = reported(model);
        named_by_reply(&mut origin, response_model.as_deref());
        origin.response_model = response_model;
        origin.response_id = reported(response_id);
        let usage = match usage.cost {
            Some(_) => usage,
            None => usage.cost(
                self.pricing(&origin)
                    .and_then(|pricing| pricing.cost(&usage)),
            ),
        };
        let error = error.or_else(|| {
            if !self.wire {
                return None;
            }
            match &reason {
                None => Some("the provider ended the reply without a finish reason".to_owned()),
                Some(FinishReason::Length) => None,
                Some(_) if !self.unfinished.is_empty() => Some(
                    "the provider ended the reply with a tool call it never finished".to_owned(),
                ),
                Some(_) => None,
            }
        });
        let mut response = CompletionResponse::new(choice, usage, origin, reply.raw)
            .with_optional_finish_reason(reason)
            .accept_unknown_finish_reasons(self.accept_unknown_finish)
            .with_unfinished_calls(self.unfinished.clone());
        response.error = error;
        response.provider_request_id = reported(reply.provider_request_id);
        response
    }
}

fn named_by_reply(origin: &mut Origin, reported: Option<&str>) {
    if origin.model.is_empty()
        && let Some(model) = reported
    {
        model.clone_into(&mut origin.model);
    }
}

pub(crate) fn canonical(block: AssistantContent) -> AssistantContent {
    match block {
        AssistantContent::Opaque(opaque) => AssistantContent::Opaque(Opaque {
            replay: false,
            ..opaque
        }),
        block => block.into_canonical(),
    }
}

fn item_of(content: &mut AssistantContent) -> Option<&mut serde_json::Value> {
    let native = match content {
        AssistantContent::Text(text) => text.native.as_mut(),
        AssistantContent::ToolCall(call) => call.native.as_mut(),
        AssistantContent::Reasoning(reasoning) => reasoning.native.as_mut(),
        AssistantContent::Image(image) => image.native.as_mut(),
        AssistantContent::Opaque(opaque) => return Some(&mut opaque.item),
    };
    native.map(|native| &mut native.item)
}

fn kind_of(content: &AssistantContent) -> PartKind {
    match content {
        AssistantContent::Text(_) => PartKind::Text,
        AssistantContent::Reasoning(_) => PartKind::Reasoning,
        AssistantContent::ToolCall(_) => PartKind::ToolCall,
        AssistantContent::Image(_) => PartKind::Image,
        AssistantContent::Opaque(_) => PartKind::Opaque,
    }
}

pub(crate) type Items = std::collections::VecDeque<Result<Item<StreamEvent>, ProviderError>>;

fn emit(items: &mut Items, event: StreamEvent) {
    items.push_back(Ok(Item::Event(event)));
}

pub fn merge(item: &mut serde_json::Value, delta: &serde_json::Map<String, serde_json::Value>) {
    use serde_json::Value;
    if !item.is_object() {
        *item = Value::Object(serde_json::Map::new());
    }
    let Value::Object(item) = item else {
        return;
    };
    for (key, value) in delta {
        if key == "type" {
            continue;
        }
        match (item.get_mut(key), value) {
            (Some(Value::String(existing)), Value::String(fragment)) => {
                existing.push_str(fragment);
            }
            (Some(Value::Array(existing)), Value::Array(more)) => {
                existing.extend(more.iter().cloned());
            }
            _ => {
                item.insert(key.clone(), value.clone());
            }
        }
    }
}

impl Fold<Completion> for ReplyFold {
    fn absorb(&mut self, event: &StreamEvent) -> Result<(), ProviderError> {
        match event {
            StreamEvent::Start { part, .. } => {
                if self.choice.len() <= part.index() {
                    self.choice.resize(part.index() + 1, None);
                }
            }
            StreamEvent::End { part, content } => {
                if self.choice.len() <= part.index() {
                    self.choice.resize(part.index() + 1, None);
                }
                if let Some(slot) = self.choice.get_mut(part.index()) {
                    *slot = Some(content.as_ref().clone());
                }
                self.open_text.remove(&part.index());
            }
            StreamEvent::Text { part, text } => {
                if let AssistantContent::Text(open) = self
                    .open_text
                    .entry(part.index())
                    .or_insert_with(|| AssistantContent::text(""))
                {
                    open.text.push_str(text);
                }
            }
            StreamEvent::Reasoning { part, text } => {
                if let AssistantContent::Reasoning(open) = self
                    .open_text
                    .entry(part.index())
                    .or_insert_with(|| AssistantContent::reasoning(""))
                {
                    open.text.push_str(text);
                }
            }
            StreamEvent::Arguments { .. } => {}
        }
        Ok(())
    }

    fn finish(mut self, end: Finish, reply: Reply) -> Result<CompletionResponse, ProviderError> {
        if let Some(FinishReason::Other(_)) = &end.reason {
            tracing::warn!(
                provider = %self.origin.provider,
                accepted = self.accept_unknown_finish,
                "the provider ended the reply with an unknown finish reason"
            );
        }
        // The parts taken, in their position (in wire-index order on a wire
        // that asked for it, after the lead block); parts from the first
        // incomplete one on keep no provider item.
        let parts = std::mem::take(&mut self.choice).into_iter().enumerate();
        let choice = self.ordered(parts.collect(), self.first_incomplete);
        let response = self.response(end, reply, choice);
        self.span
            .record_response(response.response_id(), response.model(), &response.usage);
        Ok(response)
    }
}

impl<'id> Out<'id, Completion> {
    pub(crate) fn catalog_pricing(
        &self,
        reported: Option<&str>,
    ) -> Option<crate::codecs::catalog::Pricing> {
        let shared = self.lock();
        let mut origin = shared.fold.origin.clone();
        named_by_reply(&mut origin, reported);
        shared.fold.pricing(&origin)
    }

    pub fn open(
        &mut self,
        index: usize,
        block: Block,
        item: serde_json::Value,
    ) -> Result<(), ProviderError> {
        let mut shared = self.lock();
        let Shared { fold, items, .. } = &mut *shared;
        fold.open_item(items, index, block, item)
    }

    pub fn push(&mut self, index: usize, fragment: &str) -> Result<(), ProviderError> {
        let mut shared = self.lock();
        let Shared { fold, items, .. } = &mut *shared;
        fold.push_item(items, index, fragment)
    }

    pub fn edit(
        &mut self,
        index: usize,
        edit: impl FnOnce(&mut serde_json::Value),
    ) -> Result<(), ProviderError> {
        let mut shared = self.lock();
        let Shared { fold, items, .. } = &mut *shared;
        match fold.open.get_mut(&index) {
            Some(draft) => {
                edit(&mut draft.item);
                Ok(())
            }
            None => fold.edit_ended(items, index, edit),
        }
    }

    pub fn close(&mut self, index: usize) -> Result<(), ProviderError> {
        let mut shared = self.lock();
        let Shared { fold, items, .. } = &mut *shared;
        fold.close_item(items, index, Closing::Incomplete)
    }

    pub fn finish(&mut self, index: usize) -> Result<(), ProviderError> {
        let mut shared = self.lock();
        let Shared { fold, items, .. } = &mut *shared;
        fold.close_item(items, index, Closing::Complete)
    }

    pub fn finish_open(&mut self) -> Result<(), ProviderError> {
        let mut shared = self.lock();
        let Shared { fold, items, .. } = &mut *shared;
        let mut open: Vec<(Part, usize)> = fold
            .open
            .iter()
            .map(|(index, draft)| (draft.part, *index))
            .collect();
        open.sort();
        for (_, index) in open {
            fold.close_item(items, index, Closing::Complete)?;
        }
        Ok(())
    }

    pub fn whole(
        &mut self,
        index: usize,
        block: Block,
        item: serde_json::Value,
        text: &str,
    ) -> Result<(), ProviderError> {
        self.open(index, block, item)?;
        self.push(index, text)?;
        self.finish(index)
    }

    pub fn order_by_index(&mut self) {
        let mut shared = self.lock();
        if shared.fold.by_index.is_none() {
            shared.fold.by_index = Some(BTreeMap::new());
        }
    }

    pub fn lead(&mut self, index: usize) -> Result<(), ProviderError> {
        let mut shared = self.lock();
        let position = shared.fold.draft(index)?.part.index();
        shared.fold.lead.get_or_insert(position);
        Ok(())
    }

    pub fn restate(&mut self, index: usize, text: &str) -> Result<(), ProviderError> {
        let mut shared = self.lock();
        let draft = shared.fold.draft(index)?;
        match &mut draft.body {
            Body::Text(body) | Body::Reasoning { text: body, .. } => text.clone_into(body),
            Body::Call { arguments, .. } => {
                text.clone_into(&mut arguments.text);
                // A call not yet started streams the restated text whole.
                arguments.streamed = if draft.started { text.len() } else { 0 };
            }
            Body::Image(_) | Body::Opaque { .. } => {}
        }
        Ok(())
    }

    pub fn fragment(
        &mut self,
        index: Option<usize>,
        fragment: CallFragment<'_>,
    ) -> Result<(), ProviderError> {
        let mut shared = self.lock();
        let Shared {
            fold: turn, items, ..
        } = &mut *shared;
        let new_id = fragment.id.filter(|id| stated(id));
        let index = match index {
            Some(index) => {
                let names = fragment.name.is_some_and(|name| !name.is_empty());
                let held = turn.open.get(&index).and_then(|draft| match &draft.body {
                    Body::Call {
                        id: Some(id),
                        arguments,
                        ..
                    } if new_id.is_some_and(|new| id.wire() != new) => Some(arguments.complete()),
                    _ => None,
                });
                if let Some(complete) = held.filter(|complete| *complete || names) {
                    // Another call took over the index: the one it held ends.
                    let moved = turn.fresh_index();
                    if let Some(draft) = turn.open.remove(&index) {
                        turn.open.insert(moved, draft);
                    }
                    let closing = if complete {
                        Closing::Complete
                    } else {
                        Closing::Incomplete
                    };
                    turn.close_item(items, moved, closing)?;
                }
                index
            }
            None => {
                let owner = new_id.and_then(|new| {
                    turn.open
                        .iter()
                        .find_map(|(index, draft)| match &draft.body {
                            Body::Call { id: Some(id), .. } if id.wire() == new => Some(*index),
                            _ => None,
                        })
                });
                let continues = |last: &usize| {
                    turn.open.get(last).is_some_and(|draft| {
                        matches!(&draft.body, Body::Call { arguments, .. } if !arguments.complete())
                    })
                };
                match (owner, new_id, turn.last_call) {
                    (Some(index), _, _) => index,
                    (None, None, Some(last)) if continues(&last) => last,
                    _ => turn.fresh_index(),
                }
            }
        };
        turn.last_call = Some(index);
        if !turn.open.contains_key(&index) {
            let body = Body::Call {
                id: None,
                name: String::new(),
                arguments: Arguments::default(),
            };
            turn.insert(index, body, serde_json::Value::Null);
        }
        let Body::Call {
            id,
            name,
            arguments,
        } = &mut turn.draft(index)?.body
        else {
            return Err(ProviderError::Response(format!(
                "the reply sent a call fragment for item {index}, which is not a call"
            )));
        };
        if let Some(call_id) = fragment.id.filter(|id| stated(id)) {
            *id = Some(CallId::from_wire(call_id));
        }
        // A call keeps the first name it is given, so the tool its start
        // names is the tool its end names.
        if name.is_empty()
            && let Some(fragment) = fragment.name.filter(|name| !name.is_empty())
        {
            fragment.clone_into(name);
        }
        if let Some(fragment) = fragment.arguments {
            arguments.push(fragment, name);
        }
        stream_call(items, turn.draft(index)?);
        Ok(())
    }

    pub fn announce(
        &mut self,
        index: usize,
        announced: serde_json::Value,
    ) -> Result<(), ProviderError> {
        if let Body::Call { arguments, .. } = &mut self.lock().fold.draft(index)?.body {
            arguments.announced = Some(announced);
        }
        Ok(())
    }

    pub fn fresh_index(&mut self) -> usize {
        self.lock().fold.fresh_index()
    }

    pub fn run(&mut self, block: Block, fragment: &str) -> Result<usize, ProviderError> {
        let mut shared = self.lock();
        let Shared { fold, items, .. } = &mut *shared;
        fold.run_item(items, block, fragment)
    }

    pub fn end_run(&mut self) -> Result<(), ProviderError> {
        let mut shared = self.lock();
        let Shared { fold, items, .. } = &mut *shared;
        fold.end_run(items)
    }

    pub fn cite(&mut self, index: usize, citation: WireCitation) {
        let mut shared = self.lock();
        let Shared { fold, items, .. } = &mut *shared;
        fold.cite_item(items, index, vec![citation], false);
    }

    pub fn set_citations(&mut self, index: usize, citations: Vec<WireCitation>) {
        let mut shared = self.lock();
        let Shared { fold, items, .. } = &mut *shared;
        fold.cite_item(items, index, citations, true);
    }

    pub fn content(&mut self, content: AssistantContent) -> Result<(), ProviderError> {
        let mut shared = self.lock();
        let Shared { fold, items, .. } = &mut *shared;
        fold.write_content(items, content)
    }
}
