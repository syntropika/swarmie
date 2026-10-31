use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use base64::Engine as _;
use base64::prelude::{BASE64_STANDARD, BASE64_STANDARD_NO_PAD};

use crate::codecs::conversation::pairing::is_result;
use crate::codecs::message::{
    Api, AssistantContent, AssistantMessage, CallId, DocumentData, DocumentMediaType,
    DocumentSourceKind, ImageMediaType, Message, Origin, StopReason, Text, ToolResult,
    ToolResultContent, UserContent,
};
use crate::codecs::wasm_compat::WasmCompatSync;

pub const IMAGE_OMITTED: &str = "(image omitted: model does not support images)";

pub const TOOL_IMAGE_OMITTED: &str = "(tool image omitted: model does not support images)";

pub const IMAGE_UNSENDABLE: &str = "(image omitted: the provider cannot receive it in this form)";

pub const AUDIO_UNSENDABLE: &str = "(audio omitted: the provider cannot receive it in this form)";

pub const VIDEO_UNSENDABLE: &str = "(video omitted: the provider cannot receive it in this form)";

pub const DOCUMENT_UNSENDABLE: &str =
    "(document omitted: the provider cannot receive it in this form)";

pub const TOOL_IMAGE_ATTACHED: &str = "(see attached image)";

pub const TOOL_IMAGES_HEADING: &str = "Attached image(s) from tool result:";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Accepts {
    pub user_images: bool,
    pub assistant_images: bool,
    pub tool_result_images: bool,
    pub tools: bool,
    pub foreign_reasoning: bool,
}

impl Accepts {
    pub const ALL: Self = Self {
        user_images: true,
        assistant_images: true,
        tool_result_images: true,
        tools: true,
        foreign_reasoning: false,
    };

    pub const TEXT: Self = Self {
        user_images: false,
        assistant_images: false,
        tool_result_images: false,
        tools: true,
        foreign_reasoning: false,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    User,
    ToolResult,
    Assistant,
}

#[derive(Debug, Clone, Copy)]
pub enum Media<'a> {
    Image(&'a crate::codecs::message::Image, Place),
    Audio(&'a crate::codecs::message::Audio),
    Video(&'a crate::codecs::message::Video),
    Document(&'a crate::codecs::message::Document),
}

pub trait ReplayTarget: std::fmt::Debug + WasmCompatSync {
    fn api(&self) -> Api;

    fn provider(&self) -> &str;

    fn model(&self) -> &str;

    fn accepts(&self, model: &str) -> Accepts;

    fn map_options(
        &self,
        request: &crate::codecs::completion::CompletionRequest,
        fields: crate::codecs::completion::options::OptionFields<'_>,
    ) -> crate::codecs::completion::options::OptionMap;

    fn facts(&self) -> Option<&crate::codecs::catalog::ModelFacts> {
        None
    }

    fn encodes(&self, model: &str, media: Media<'_>) -> bool {
        let _ = (model, media);
        true
    }

    fn normalize_tool_call_id(&self, id: &str, model: &str, source: Option<&Origin>) -> String {
        let _ = (model, source);
        id.to_owned()
    }

    fn continues_stored(&self, request: &crate::codecs::completion::CompletionRequest) -> bool {
        let _ = request;
        false
    }

    fn declares_tools(&self, request: &crate::codecs::completion::CompletionRequest) -> bool {
        declares_tools(request)
    }

    fn identity(&self, item: &serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        let _ = item;
        serde_json::Map::new()
    }

    fn call_id_slot(&self) -> Option<&'static str> {
        None
    }

    fn blob_slots(&self) -> &'static [&'static str] {
        &[]
    }

    fn states_finish_reason(&self) -> bool {
        true
    }

    fn catalog_priced(&self) -> bool {
        true
    }

    fn binds_context(&self, model: &str) -> bool {
        let _ = model;
        false
    }

    fn drops_unbound_items(&self, request: &crate::codecs::completion::CompletionRequest) -> bool {
        let _ = request;
        false
    }

    fn route(
        &self,
        request: &crate::codecs::completion::CompletionRequest,
    ) -> Option<&dyn ReplayTarget> {
        let _ = request;
        None
    }

    fn takes_documents(&self) -> bool {
        false
    }

    fn needs_next(&self, item: &serde_json::Value) -> bool {
        let _ = item;
        false
    }

    fn hosted_pair(&self, item: &serde_json::Value) -> Option<(Pairing, String)> {
        let _ = item;
        None
    }

    fn starts_with_user(&self) -> bool {
        false
    }

    fn later_system(&self, model: &str) -> LaterSystem {
        let _ = model;
        LaterSystem::InPlace
    }

    fn alternates_roles(&self) -> bool {
        false
    }

    fn hosted_needs_tools(&self) -> bool {
        false
    }

    fn result_parts(&self, model: &str) -> bool {
        let _ = model;
        false
    }

    fn sends_alone(&self, block: &AssistantContent) -> bool {
        let _ = block;
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaterSystem {
    InPlace,
    Leading,
    UserText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pairing {
    Use,
    Result,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Replay<'a> {
    Item(std::borrow::Cow<'a, serde_json::Value>),
    Identity(serde_json::Map<String, serde_json::Value>),
    Rebuild,
}

impl AssistantContent {
    pub fn replay(
        &self,
        target: &dyn ReplayTarget,
        ids: &crate::codecs::providers::internal::wire_ids::WireIds,
    ) -> Replay<'_> {
        if let Some(item) = self.native_item() {
            return match (self, target.call_id_slot()) {
                (AssistantContent::ToolCall(call), Some(slot)) => {
                    let mut item = item.clone();
                    if let Some(id) = ids.of(&call.id) {
                        set_pointer(&mut item, slot, serde_json::Value::String(id.to_owned()));
                    }
                    Replay::Item(std::borrow::Cow::Owned(item))
                }
                _ => Replay::Item(std::borrow::Cow::Borrowed(item)),
            };
        }
        let identity = self
            .stale_item()
            .map(|item| target.identity(item))
            .unwrap_or_default();
        if identity.is_empty() {
            Replay::Rebuild
        } else {
            Replay::Identity(identity)
        }
    }
}

fn set_pointer(item: &mut serde_json::Value, pointer: &str, value: serde_json::Value) {
    let mut at = item;
    let mut keys = pointer.split('/').skip(1).peekable();
    while let Some(key) = keys.next() {
        if !at.is_object() {
            *at = serde_json::Value::Object(serde_json::Map::new());
        }
        let serde_json::Value::Object(fields) = at else {
            return;
        };
        if keys.peek().is_none() {
            fields.insert(key.to_owned(), value);
            return;
        }
        at = fields
            .entry(key.to_owned())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    }
}

pub fn adapt(history: &[Message], target: &dyn ReplayTarget) -> Vec<Message> {
    let history = history.iter().cloned().map(Arc::new).collect();
    let adapted = adapt_for(history, target, &Request::default(), &mut Memo::default());
    adapted.into_iter().map(Arc::unwrap_or_clone).collect()
}

#[derive(Debug, Default)]
pub(crate) struct Memo {
    shape: Option<Shape>,
    given: Vec<Arc<Message>>,
    shaped: Vec<Option<Arc<Message>>>,
    taken: HashSet<String>,
}

#[derive(Debug, PartialEq)]
struct Shape {
    api: Api,
    provider: String,
    model: String,
    context: Option<crate::codecs::message::Fingerprint>,
    accepts: Accepts,
    from_user: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Request<'a> {
    pub(crate) model: Option<&'a str>,
    pub(crate) stored: bool,
    pub(crate) tools: bool,
    pub(crate) context: Option<crate::codecs::message::Fingerprint>,
    pub(crate) foreign_reasoning: bool,
}

impl Default for Request<'_> {
    fn default() -> Self {
        Self {
            model: None,
            stored: false,
            tools: true,
            context: None,
            foreign_reasoning: false,
        }
    }
}

pub(crate) fn context_of(
    request: &crate::codecs::completion::CompletionRequest,
    target: &dyn ReplayTarget,
    model: &str,
) -> crate::codecs::message::Fingerprint {
    let mut tools: Vec<_> = request.tools.iter().collect();
    tools.sort_by(|left, right| left.name.cmp(&right.name));
    let raw = raw_tools(request);
    let folds = target.later_system(model) == LaterSystem::Leading;
    let mut system: Vec<&str> = request
        .chat_history
        .iter()
        .map_while(|message| match &**message {
            Message::System { content } => Some(Some(content.as_str())),
            Message::User { .. } | Message::Assistant(_) => folds.then_some(None),
        })
        .flatten()
        .filter(|content| !content.trim().is_empty())
        .collect();
    let joined;
    if folds && system.len() > 1 {
        joined = system.join("\n\n");
        system = vec![joined.as_str()];
    }
    let mut fields = vec![serde_json::json!("context"), serde_json::json!(tools)];
    fields.push(serde_json::json!(system));
    if !raw.is_empty() {
        fields.push(serde_json::json!(raw));
    }
    // Hashed as built: `Fingerprint::of` would copy it first.
    let fields = crate::codecs::message::Canonical(&serde_json::Value::Array(fields));
    crate::codecs::message::Fingerprint::of_canonical(&fields)
}

pub(crate) fn declares_tools(request: &crate::codecs::completion::CompletionRequest) -> bool {
    !request.tools.is_empty() || !raw_tools(request).is_empty()
}

pub(crate) fn raw_tools(
    request: &crate::codecs::completion::CompletionRequest,
) -> &[serde_json::Value] {
    use crate::codecs::json_utils::Lenient;
    request
        .additional_params
        .as_ref()
        .map_or(&[][..], |params| params.arr("tools"))
}

pub(crate) fn adapt_for(
    history: Vec<Arc<Message>>,
    target: &dyn ReplayTarget,
    request: &Request<'_>,
    memo: &mut Memo,
) -> Vec<Arc<Message>> {
    let model = request.model.unwrap_or(target.model());
    let stored = request.stored;
    let same = Same {
        api: target.api(),
        provider: target.provider(),
        model,
        context: request.context.filter(|_| target.binds_context(model)),
    };
    let mut accepts = target.accepts(model);
    accepts.tools &= request.tools;
    accepts.foreign_reasoning &= request.foreign_reasoning;
    let starts_with_user = target.starts_with_user() && !stored;
    let shape = Shape {
        api: same.api.clone(),
        provider: same.provider.to_owned(),
        model: model.to_owned(),
        context: same.context,
        accepts,
        from_user: starts_with_user,
    };
    let resumed = memo.shape.as_ref() == Some(&shape)
        && history.get(..memo.given.len()).is_some_and(|given| {
            given
                .iter()
                .zip(&memo.given)
                .all(|(message, was)| Arc::ptr_eq(message, was) || message == was)
        });
    if !resumed {
        *memo = Memo::default();
    }
    // The last message kept was the last turn, so it is adapted again.
    let from = memo.given.len().saturating_sub(1);
    let last_turn = history
        .iter()
        .rposition(|message| matches!(&**message, Message::Assistant(_)));
    let (hosted, open) = hosted_pairs(&history, from, last_turn, target, &same);
    let mut ids = Renamed {
        to: Vec::new(),
        before: &memo.taken,
        taken: HashSet::new(),
    };
    let mut claimed = Vec::new();
    let mut kept = 0;
    let form = Form {
        target,
        model,
        accepts,
    };
    // A message adapting leaves as it is goes on as the same `Arc`.
    let mut shaped: Vec<Option<Arc<Message>>> = Vec::with_capacity(history.len());
    shaped.extend(memo.shaped.iter().cloned());
    for (at, message) in history.iter().enumerate().skip(from) {
        if Some(at) == last_turn {
            claimed = ids.taken.iter().map(|id| id.to_string()).collect();
            kept = shaped.len();
        }
        match &**message {
            Message::System { content } => {
                if !content.trim().is_empty() {
                    shaped.push(Some(message.clone()));
                }
            }
            Message::User { content } => {
                shaped.extend(
                    user(message, content, &mut ids, &form)
                        .into_iter()
                        .map(Some),
                );
            }
            Message::Assistant(turn) if matches!(turn.stop, Some(StopReason::Aborted(_))) => {
                shaped.push(stopped(turn));
            }
            // A failed turn is skipped, with the results answering it.
            Message::Assistant(turn)
                if turn.stop.as_ref().is_some_and(|stop| stop.is_failure()) =>
            {
                shaped.push(None);
            }
            Message::Assistant(turn) => {
                // A use can still be running only in the last turn, and only
                // while nothing new follows it or the turn awaits a client call.
                let last = Some(at) == last_turn
                    && (turn
                        .content
                        .iter()
                        .any(|block| matches!(block, AssistantContent::ToolCall(_)))
                        || history
                            .get(at + 1..)
                            .into_iter()
                            .flatten()
                            .all(|message| matches!(&**message, Message::System { .. })));
                let here: HashSet<usize> = hosted
                    .iter()
                    .filter(|(message, _)| *message == at)
                    .map(|(_, block)| *block)
                    .collect();
                shaped.push(assistant(
                    message, turn, &form, &same, &mut ids, &here, last,
                ));
            }
        }
    }
    let (before, tail) = shaped.split_at(memo.shaped.len().min(shaped.len()));
    let last = before.last().and_then(Option::as_deref);
    if settled(tail, last, starts_with_user, accepts.tools) {
        drop(ids);
        // The next history adapts from the last turn on unless a hosted use
        // before it may yet pair.
        match last_turn.filter(|_| !open) {
            Some(upto) => {
                let given = history.get(memo.given.len()..=upto).unwrap_or_default();
                memo.given.extend(given.iter().cloned());
                let shaped = shaped.get(memo.shaped.len()..kept).unwrap_or_default();
                memo.shaped.extend(shaped.iter().cloned());
                memo.taken.extend(claimed);
                memo.shape = Some(shape);
            }
            None => *memo = Memo::default(),
        }
        let settled: Vec<Arc<Message>> = shaped.into_iter().flatten().collect();
        #[cfg(test)]
        {
            let shaped = settled.iter().map(|message| Some(Message::clone(message)));
            let rebuilt = reshaped(shaped.collect(), target, model, stored, accepts.tools);
            let settled: Vec<&Message> = settled.iter().map(Arc::as_ref).collect();
            assert_eq!(
                settled,
                rebuilt.iter().collect::<Vec<_>>(),
                "a settled history changed"
            );
        }
        return settled;
    }
    drop(ids);
    *memo = Memo::default();
    let shaped = shaped
        .into_iter()
        .map(|message| message.map(Arc::unwrap_or_clone));
    let shaped = reshaped(shaped.collect(), target, model, stored, accepts.tools);
    shaped.into_iter().map(Arc::new).collect()
}

fn settled<'a>(
    history: &'a [Option<Arc<Message>>],
    mut last: Option<&'a Message>,
    starts_with_user: bool,
    tools: bool,
) -> bool {
    let mut waiting: Vec<&CallId> = Vec::new();
    for message in history {
        let Some(message) = message.as_deref() else {
            return false;
        };
        match (message, last) {
            (Message::System { .. }, None) => continue,
            (Message::System { .. }, Some(_))
            | (Message::User { .. }, Some(Message::User { .. }))
            | (Message::Assistant(_), Some(Message::Assistant(_))) => return false,
            (Message::Assistant(_), None) if starts_with_user => return false,
            // A turn follows nothing or a user message, which answered
            // every call before it, so nothing waits here.
            (Message::Assistant(turn), _) => {
                waiting.extend(turn.tool_calls().map(|call| &call.id));
                if !tools && !waiting.is_empty() {
                    return false;
                }
            }
            (Message::User { content }, _) => {
                let results = content.iter().take_while(|part| is_result(part)).count();
                let answered = content.iter().take(results).all(|part| match part {
                    UserContent::ToolResult(result) => {
                        let at = waiting.iter().position(|id| **id == result.call);
                        at.map(|at| waiting.remove(at)).is_some()
                    }
                    _ => false,
                });
                if content.is_empty()
                    || (!tools && results > 0)
                    || !answered
                    || !waiting.is_empty()
                    || content.iter().skip(results).any(is_result)
                {
                    return false;
                }
            }
        }
        last = Some(message);
    }
    waiting.is_empty()
}

fn reshaped(
    shaped: Vec<Option<Message>>,
    target: &dyn ReplayTarget,
    model: &str,
    stored: bool,
    tools: bool,
) -> Vec<Message> {
    // Calls and results pair first, so an orphan result goes whether or not
    // the request declares tools; only then does a request without tools
    // get them as text, with no result made up for an unanswered call.
    let pairing = crate::codecs::conversation::pairing::Pairing {
        stored,
        answers: tools,
    };
    let shaped = merge_users(crate::codecs::conversation::pairing::pair(shaped, pairing).messages);
    let shaped = if tools { shaped } else { tools_as_text(shaped) };
    let shaped = match target.later_system(model) {
        LaterSystem::InPlace => shaped,
        LaterSystem::Leading => leading_system(shaped),
        LaterSystem::UserText => system_as_user_text(shaped),
    };
    let shaped = if target.starts_with_user() && !stored {
        from_first_user(shaped)
    } else {
        shaped
    };
    if target.alternates_roles() {
        alternated(shaped)
    } else {
        shaped
    }
}

pub(crate) fn cache_points(
    points: &[crate::codecs::completion::CachePoint],
    written: &[Arc<Message>],
    documents: Option<&Message>,
    adapted: &[Arc<Message>],
    target: &dyn ReplayTarget,
    request: &Request<'_>,
) -> Vec<crate::codecs::completion::CachePoint> {
    use crate::codecs::completion::CachePoint;
    let prefix_of = |turns: usize, items: Option<usize>| -> Option<Vec<Arc<Message>>> {
        let mut prefix = Vec::new();
        let mut counted = 0;
        for message in written {
            if counted == turns {
                break;
            }
            let mut message = Arc::clone(message);
            if !matches!(*message, Message::System { .. }) {
                counted += 1;
            }
            // Only the message the point cuts inside is copied.
            if let Some(items) = items.filter(|_| counted == turns) {
                match Arc::make_mut(&mut message) {
                    Message::User { content } => content.truncate(items),
                    Message::Assistant(turn) => turn.content.truncate(items),
                    Message::System { .. } => {}
                }
            }
            prefix.push(message);
        }
        (counted == turns).then_some(prefix)
    };
    points
        .iter()
        .filter_map(|point| {
            let prefix = match *point {
                CachePoint::Tools | CachePoint::System | CachePoint::Messages(0) => {
                    return Some(*point);
                }
                CachePoint::Messages(turns) => prefix_of(turns, None),
                CachePoint::Within { message, items } => message
                    .checked_add(1)
                    .and_then(|turns| prefix_of(turns, Some(items))),
            };
            let mut prefix = prefix?;
            if let Some(documents) = documents {
                crate::codecs::completion::request::insert_after_leading_system(
                    &mut prefix,
                    documents.clone(),
                );
            }
            let prefix = adapt_for(prefix, target, request, &mut Memo::default());
            Some(shared_prefix(adapted, &prefix))
        })
        .collect()
}

fn shared_prefix(
    history: &[Arc<Message>],
    prefix: &[Arc<Message>],
) -> crate::codecs::completion::CachePoint {
    use crate::codecs::completion::CachePoint;
    fn turns(history: &[Arc<Message>]) -> impl Iterator<Item = &Message> {
        history
            .iter()
            .map(Arc::as_ref)
            .filter(|message| !matches!(message, Message::System { .. }))
    }
    fn lead<T: PartialEq>(whole: &[T], part: &[T]) -> (usize, usize, usize) {
        let shared = whole
            .iter()
            .zip(part)
            .take_while(|(whole, part)| whole == part)
            .count();
        (shared, whole.len(), part.len())
    }
    let mut message = 0;
    for (whole, part) in turns(history).zip(turns(prefix)) {
        let (shared, whole, part) = match (whole, part) {
            (Message::User { content: whole }, Message::User { content: part }) => {
                lead(whole, part)
            }
            (Message::Assistant(whole), Message::Assistant(part)) => {
                lead(&whole.content, &part.content)
            }
            _ => (0, 1, 1),
        };
        if shared == whole && shared == part {
            message += 1;
            continue;
        }
        return if shared == 0 {
            CachePoint::Messages(message)
        } else {
            CachePoint::Within {
                message,
                items: shared,
            }
        };
    }
    CachePoint::Messages(message)
}

fn tools_as_text(history: Vec<Message>) -> Vec<Message> {
    let history = history
        .into_iter()
        .map(|message| match message {
            Message::Assistant(mut turn) => {
                for block in &mut turn.content {
                    if let AssistantContent::ToolCall(call) = block {
                        *block = AssistantContent::Text(Text::new(format!(
                            "[called tool {} with {}]",
                            call.function.name,
                            crate::codecs::json_utils::to_canonical_string(
                                &call.function.arguments_value()
                            )
                        )));
                    }
                }
                Message::Assistant(turn)
            }
            Message::User { content } => Message::User {
                content: content
                    .into_iter()
                    .map(|part| match part {
                        UserContent::ToolResult(result) => UserContent::text(result_text(&result)),
                        part => part,
                    })
                    .collect(),
            },
            message => message,
        })
        .collect();
    merge_users(history)
}

fn from_first_user(mut history: Vec<Message>) -> Vec<Message> {
    let lead = history
        .iter()
        .take_while(|message| matches!(message, Message::System { .. }))
        .count();
    while let Some(Message::Assistant(turn)) = history.get(lead) {
        let calls: HashSet<CallId> = turn.tool_calls().map(|call| call.id.clone()).collect();
        history.remove(lead);
        if let Some(Message::User { content }) = history.get_mut(lead) {
            content.retain(|part| {
                !matches!(part, UserContent::ToolResult(result) if calls.contains(&result.call))
            });
            if content.is_empty() {
                history.remove(lead);
            }
        }
    }
    history
}

fn has_later_system(history: &[Message]) -> bool {
    history
        .iter()
        .skip_while(|message| matches!(message, Message::System { .. }))
        .any(|message| matches!(message, Message::System { .. }))
}

fn system_as_user_text(history: Vec<Message>) -> Vec<Message> {
    let mut leading = true;
    let history = history
        .into_iter()
        .map(|message| match message {
            Message::System { content } if !leading => Message::User {
                content: vec![UserContent::text(content)],
            },
            message => {
                leading &= matches!(message, Message::System { .. });
                message
            }
        })
        .collect();
    merge_users(history)
}

fn alternated(history: Vec<Message>) -> Vec<Message> {
    let mut alternated: Vec<Message> = Vec::with_capacity(history.len());
    let mut held: Vec<Message> = Vec::new();
    for message in history {
        let started = alternated
            .iter()
            .any(|message| !matches!(message, Message::System { .. }));
        match (message, alternated.last_mut()) {
            (message @ Message::System { .. }, _) if started => held.push(message),
            (Message::User { content }, Some(Message::User { content: previous })) => {
                previous.extend(content);
            }
            (Message::Assistant(turn), Some(Message::Assistant(previous))) => {
                if previous.origin != turn.origin {
                    previous.origin = None;
                }
                previous.stop = turn.stop;
                previous.content.extend(turn.content);
            }
            (message, _) => {
                alternated.append(&mut held);
                alternated.push(message);
            }
        }
    }
    alternated.append(&mut held);
    alternated
}

fn leading_system(history: Vec<Message>) -> Vec<Message> {
    if !has_later_system(&history) {
        return history;
    }
    let (system, rest): (Vec<Message>, Vec<Message>) = history
        .into_iter()
        .partition(|message| matches!(message, Message::System { .. }));
    let prompt: Vec<String> = system
        .into_iter()
        .filter_map(|message| match message {
            Message::System { content } => Some(content),
            Message::User { .. } | Message::Assistant(_) => None,
        })
        .collect();
    let mut history = Vec::with_capacity(rest.len() + 1);
    if !prompt.is_empty() {
        history.push(Message::system(prompt.join("\n\n")));
    }
    // Moving a system message out may leave two user messages adjacent.
    history.extend(merge_users(rest));
    history
}

struct Renamed<'a> {
    to: Vec<(Cow<'a, CallId>, Cow<'a, CallId>)>,
    before: &'a HashSet<String>,
    taken: HashSet<Cow<'a, str>>,
}

impl<'a> Renamed<'a> {
    fn turn(&mut self) {
        self.to.clear();
    }

    fn claim(&mut self, source: Cow<'a, CallId>, wanted: Cow<'a, str>) -> Cow<'a, CallId> {
        let id = match &*source {
            CallId::Local(_) => source.clone(),
            CallId::Provider(_) => {
                let mut id = wanted.clone();
                let mut attempt: u64 = 1;
                while self.before.contains(id.as_ref()) || self.taken.contains(&id) {
                    id = Cow::Owned(with_counter(&wanted, attempt));
                    attempt += 1;
                }
                self.taken.insert(id.clone());
                if id == source.wire() {
                    source.clone()
                } else {
                    Cow::Owned(CallId::from_wire(id))
                }
            }
        };
        self.to.push((source, id.clone()));
        id
    }

    fn answer(&mut self, source: &CallId) -> Option<Cow<'a, CallId>> {
        let mut calls = (self.to.iter().enumerate())
            .filter(|(_, (from, _))| from.as_ref() == source)
            .map(|(at, _)| at);
        let first = calls.next()?;
        if calls.next().is_some() {
            Some(self.to.remove(first).1)
        } else {
            self.to.get(first).map(|(_, id)| id.clone())
        }
    }
}

fn with_counter(id: &str, attempt: u64) -> String {
    let mut digits = Vec::new();
    let mut value = attempt;
    while value > 0 {
        digits.push(char::from_digit((value % 36) as u32, 36).unwrap_or('0'));
        value /= 36;
    }
    digits.reverse();
    let keep = id.chars().count().saturating_sub(digits.len());
    id.chars().take(keep).chain(digits).collect()
}

struct Same<'a> {
    api: Api,
    provider: &'a str,
    model: &'a str,
    context: Option<crate::codecs::message::Fingerprint>,
}

impl Same<'_> {
    fn is(&self, origin: &Origin) -> bool {
        origin.same_model(&self.api, self.provider, self.model)
            && self
                .context
                .is_none_or(|context| origin.context == Some(context))
    }
}

fn assistant<'a>(
    message: &Arc<Message>,
    turn: &'a AssistantMessage,
    form: &Form<'_>,
    same_model: &Same<'_>,
    ids: &mut Renamed<'a>,
    hosted: &HashSet<usize>,
    last: bool,
) -> Option<Arc<Message>> {
    let Form {
        target,
        model,
        accepts,
    } = *form;
    let same = turn
        .origin
        .as_ref()
        .is_some_and(|origin| same_model.is(origin));
    ids.turn();
    let content: Vec<Option<Cow<'a, AssistantContent>>> = turn
        .content
        .iter()
        .map(|block| {
            let block = if same {
                match block {
                    AssistantContent::ToolCall(call) => {
                        let id = ids.claim(Cow::Borrowed(&call.id), call.id.wire());
                        if *id == call.id {
                            Cow::Borrowed(block)
                        } else {
                            // A route rename is not an edit: replay spells
                            // the new id into the item's slot, so the item
                            // stays.
                            let keep =
                                block.native_item().is_some() && target.call_id_slot().is_some();
                            let id = id.into_owned();
                            let renamed = crate::codecs::message::ToolCall { id, ..call.clone() };
                            let mut renamed = AssistantContent::ToolCall(renamed);
                            if keep
                                && let Some(native) = renamed.native_slot().and_then(Option::take)
                            {
                                let blob_slot = native.blob_slot;
                                renamed = renamed.with_native(native.item);
                                if let Some(Some(native)) = renamed.native_slot() {
                                    native.blob_slot = blob_slot;
                                }
                            } else if let Some(Some(native)) = renamed.native_slot() {
                                native.edited = true;
                            }
                            Cow::Owned(renamed)
                        }
                    }
                    // The capability holds for the model's own turns too: an
                    // image it made but does not read back is left out.
                    AssistantContent::Image(_) if !accepts.assistant_images => return None,
                    block => Cow::Borrowed(block),
                }
            } else {
                Cow::Owned(match block.canonical() {
                    AssistantContent::Reasoning(reasoning) => {
                        if reasoning.redacted || reasoning.text.trim().is_empty() {
                            return None;
                        }
                        if accepts.foreign_reasoning {
                            AssistantContent::Reasoning(reasoning)
                        } else {
                            AssistantContent::Text(Text::new(reasoning.text))
                        }
                    }
                    AssistantContent::Opaque(_) => return None,
                    AssistantContent::Image(_) if !accepts.assistant_images => {
                        AssistantContent::Text(Text::new(IMAGE_OMITTED))
                    }
                    AssistantContent::ToolCall(mut call) => {
                        let normalized = target.normalize_tool_call_id(
                            &call.id.wire(),
                            model,
                            turn.origin.as_ref(),
                        );
                        let source = Cow::Owned(call.id);
                        call.id = ids.claim(source, Cow::Owned(normalized)).into_owned();
                        AssistantContent::ToolCall(call)
                    }
                    block => block,
                })
            };
            // Keys sorted, so the text is the same however the provider
            // ordered the arguments.
            let block = match &*block {
                AssistantContent::Opaque(opaque)
                    if !accepts.tools
                        && target.hosted_needs_tools()
                        && target.hosted_pair(&opaque.item).is_some() =>
                {
                    return None;
                }
                _ => block,
            };
            (!block.is_blank()).then_some(block)
        })
        .collect();
    let content: Vec<Cow<'a, AssistantContent>> = if same {
        paired(content, target, accepts.tools, hosted, last)
    } else {
        content.into_iter().flatten().collect()
    };
    let content: Vec<Cow<'a, AssistantContent>> = content
        .into_iter()
        .map(|block| match &*block {
            AssistantContent::Image(image) if image.native.is_none() => {
                if sendable_as_is(image) && form.sends(image, Place::Assistant) {
                    return block;
                }
                let image = sendable_image(image.clone());
                Cow::Owned(
                    if matches!(image.data, DocumentSourceKind::Unknown)
                        || !target.encodes(model, Media::Image(&image, Place::Assistant))
                    {
                        AssistantContent::Text(Text::new(IMAGE_UNSENDABLE))
                    } else {
                        AssistantContent::Image(image)
                    },
                )
            }
            _ => block,
        })
        .collect();
    if !turn.content.is_empty() && !content.iter().any(|block| target.sends_alone(block)) {
        return None;
    }
    if content.len() == turn.content.len()
        && content
            .iter()
            .all(|block| matches!(block, Cow::Borrowed(_)))
    {
        return Some(message.clone());
    }
    Some(Arc::new(Message::Assistant(AssistantMessage {
        content: content.into_iter().map(Cow::into_owned).collect(),
        origin: turn.origin.clone(),
        stop: turn.stop.clone(),
    })))
}

fn stopped(turn: &AssistantMessage) -> Option<Arc<Message>> {
    let content: Vec<AssistantContent> = turn
        .content
        .iter()
        .filter(|block| matches!(block, AssistantContent::Text(_)))
        .map(AssistantContent::canonical)
        .filter(|block| !block.is_blank())
        .collect();
    (!content.is_empty()).then(|| Arc::new(Message::Assistant(AssistantMessage::new(content))))
}

fn hosted_pairs(
    history: &[Arc<Message>],
    from: usize,
    boundary: Option<usize>,
    target: &dyn ReplayTarget,
    same: &Same<'_>,
) -> (HashSet<(usize, usize)>, bool) {
    let mut open: HashMap<String, (usize, usize)> = HashMap::new();
    let mut paired = HashSet::new();
    let mut open_at_boundary = false;
    for (at, message) in history.iter().enumerate().skip(from) {
        if Some(at) == boundary {
            open_at_boundary = !open.is_empty();
        }
        let Message::Assistant(turn) = &**message else {
            continue;
        };
        if turn.stop.as_ref().is_some_and(|stop| stop.is_failure())
            || !turn.origin.as_ref().is_some_and(|origin| same.is(origin))
        {
            continue;
        }
        for (index, block) in turn.content.iter().enumerate() {
            let AssistantContent::Opaque(opaque) = block else {
                continue;
            };
            if !opaque.replay {
                continue;
            }
            match target.hosted_pair(&opaque.item) {
                Some((Pairing::Use, id)) => {
                    open.insert(id, (at, index));
                }
                Some((Pairing::Result, id)) => {
                    if let Some(used) = open.remove(&id) {
                        paired.insert(used);
                        paired.insert((at, index));
                    }
                }
                None => {}
            }
        }
    }
    (paired, open_at_boundary)
}

fn paired<'a>(
    mut content: Vec<Option<Cow<'a, AssistantContent>>>,
    target: &dyn ReplayTarget,
    tools: bool,
    hosted: &HashSet<usize>,
    last: bool,
) -> Vec<Cow<'a, AssistantContent>> {
    let pair = |block: &AssistantContent| match block {
        AssistantContent::Opaque(opaque) if opaque.replay => target.hosted_pair(&opaque.item),
        _ => None,
    };
    for at in 0..content.len() {
        let Some((side, _)) = content.get(at).and_then(Option::as_deref).and_then(pair) else {
            continue;
        };
        let running = last
            && side == Pairing::Use
            && content
                .get(at + 1..)
                .into_iter()
                .flatten()
                .flatten()
                .all(|block| {
                    matches!(
                        **block,
                        AssistantContent::ToolCall(_) | AssistantContent::Opaque(_)
                    )
                });
        if !hosted.contains(&at)
            && !running
            && let Some(slot) = content.get_mut(at)
        {
            *slot = None;
        }
    }
    // The item each block goes back as, read once: its item while current,
    // or a rebuild that keeps the item's identity (an edited block rebuilt
    // under it needs its partner as much as the item itself); whether it
    // goes back as one, and whether that item needs the block after it.
    let (kept, needs): (Vec<bool>, Vec<bool>) = content
        .iter()
        .map(|block| {
            let item = block.as_deref().and_then(|block| match block {
                AssistantContent::Opaque(opaque) => opaque.replay.then_some(&opaque.item),
                block => block
                    .native()
                    .filter(|(item, current)| *current || !target.identity(item).is_empty())
                    .map(|(item, _)| item),
            });
            (
                item.is_some(),
                item.is_some_and(|item| target.needs_next(item)),
            )
        })
        .unzip();
    for at in (0..content.len()).rev() {
        // The partner must go back as the item the provider issued: its item,
        // or a rebuild that keeps its identity, never a block with neither.
        // A request without tools gets a call as text.
        let next_gone = content
            .get(at + 1)
            .and_then(Option::as_deref)
            .is_none_or(|next| {
                !kept.get(at + 1).copied().unwrap_or_default()
                    || (!tools && matches!(next, AssistantContent::ToolCall(_)))
            });
        if needs.get(at).copied().unwrap_or_default()
            && next_gone
            && let Some(slot) = content.get_mut(at)
        {
            *slot = None;
        }
    }
    content.into_iter().flatten().collect()
}

struct Form<'a> {
    target: &'a dyn ReplayTarget,
    model: &'a str,
    accepts: Accepts,
}

impl Form<'_> {
    fn sends(&self, image: &crate::codecs::message::Image, place: Place) -> bool {
        let reads = match place {
            Place::User => self.accepts.user_images,
            Place::ToolResult => self.accepts.tool_result_images,
            Place::Assistant => self.accepts.assistant_images,
        };
        reads
            && !matches!(image.data, DocumentSourceKind::Unknown)
            && self.target.encodes(self.model, Media::Image(image, place))
    }
}

fn user<'a>(
    message: &Arc<Message>,
    content: &'a [UserContent],
    ids: &mut Renamed<'a>,
    form: &Form<'_>,
) -> Vec<Arc<Message>> {
    let mut shaped: Vec<Cow<'_, UserContent>> = Vec::with_capacity(content.len());
    let mut attached = Vec::new();
    for part in content {
        let placeholder = match part {
            UserContent::Image(image)
                if sendable_as_is(image) && form.sends(image, Place::User) =>
            {
                shaped.push(Cow::Borrowed(part));
                continue;
            }
            UserContent::Image(image) => {
                let image = sendable_image(image.clone());
                if form.sends(&image, Place::User) {
                    shaped.push(Cow::Owned(UserContent::Image(image)));
                    continue;
                }
                if form.accepts.user_images {
                    IMAGE_UNSENDABLE
                } else {
                    IMAGE_OMITTED
                }
            }
            UserContent::Audio(audio) => {
                let mut audio = audio.clone();
                audio.data = sendable(audio.data);
                if !matches!(audio.data, DocumentSourceKind::Unknown)
                    && form.target.encodes(form.model, Media::Audio(&audio))
                {
                    shaped.push(Cow::Owned(UserContent::Audio(audio)));
                    continue;
                }
                AUDIO_UNSENDABLE
            }
            UserContent::Video(video) => {
                let mut video = video.clone();
                video.data = sendable(video.data);
                if !matches!(video.data, DocumentSourceKind::Unknown)
                    && form.target.encodes(form.model, Media::Video(&video))
                {
                    shaped.push(Cow::Owned(UserContent::Video(video)));
                    continue;
                }
                VIDEO_UNSENDABLE
            }
            UserContent::Document(document) => {
                let mut document = document.clone();
                if let DocumentData::File(source) = document.data {
                    document.data = DocumentData::File(sendable(source));
                }
                if document.data != DocumentData::File(DocumentSourceKind::Unknown)
                    && form.target.encodes(form.model, Media::Document(&document))
                {
                    shaped.push(Cow::Owned(UserContent::Document(document)));
                    continue;
                }
                // Base64 of a media type other than PDF that decodes as
                // UTF-8 is text the wire can still read.
                let text = match document.data {
                    DocumentData::Text(text) => Some(text),
                    DocumentData::File(DocumentSourceKind::Base64(data))
                        if !matches!(document.media_type, None | Some(DocumentMediaType::PDF)) =>
                    {
                        BASE64_STANDARD
                            .decode(data.as_bytes())
                            .ok()
                            .and_then(|bytes| String::from_utf8(bytes).ok())
                    }
                    DocumentData::File(_) => None,
                };
                if let Some(text) = text {
                    shaped.push(Cow::Owned(UserContent::text(text)));
                    continue;
                }
                DOCUMENT_UNSENDABLE
            }
            UserContent::ToolResult(result) => {
                let answer = ids.answer(&result.call);
                let parts = form.accepts.tool_result_images || form.target.result_parts(form.model);
                if answer.as_ref().is_none_or(|id| **id == result.call)
                    && result_as_is(result, form, parts)
                {
                    shaped.push(Cow::Borrowed(part));
                    continue;
                }
                let mut result = result.clone();
                if let Some(id) = answer {
                    result.call = id.into_owned();
                }
                result.content = result_images(result.content, form, &mut attached);
                result.content = result_text_parts(result.content, parts, result.is_error);
                shaped.push(Cow::Owned(UserContent::ToolResult(result)));
                continue;
            }
            // Blank text says nothing, and several providers reject it.
            UserContent::Text(text) if text.text.trim().is_empty() => continue,
            UserContent::Text(_) => {
                shaped.push(Cow::Borrowed(part));
                continue;
            }
        };
        let repeated = matches!(
            shaped.last().map(|part| &**part),
            Some(UserContent::Text(text)) if text.text == placeholder
        );
        if !repeated {
            shaped.push(Cow::Owned(UserContent::text(placeholder)));
        }
    }
    let unchanged =
        shaped.len() == content.len() && shaped.iter().all(|part| matches!(part, Cow::Borrowed(_)));
    if !shaped.is_empty() && unchanged && attached.is_empty() {
        return vec![message.clone()];
    }
    let mut messages = Vec::new();
    if !shaped.is_empty() {
        let content = shaped.into_iter().map(Cow::into_owned).collect();
        messages.push(Arc::new(Message::User { content }));
    }
    if !attached.is_empty() {
        let mut content = vec![UserContent::text(TOOL_IMAGES_HEADING)];
        content.extend(attached.into_iter().map(UserContent::Image));
        messages.push(Arc::new(Message::User { content }));
    }
    messages
}

fn result_as_is(result: &ToolResult, form: &Form<'_>, parts: bool) -> bool {
    let images = result.content.iter().all(|part| match part {
        ToolResultContent::Image(image) => {
            sendable_as_is(image) && form.sends(image, Place::ToolResult)
        }
        ToolResultContent::Text(_) | ToolResultContent::Json { .. } => true,
    });
    images && !says_nothing(&result.content) && (parts || texts(&result.content) < 2)
}

pub const NO_TOOL_OUTPUT: &str = "(no tool output)";

fn result_text_parts(
    content: Vec<ToolResultContent>,
    parts: bool,
    is_error: bool,
) -> Vec<ToolResultContent> {
    if says_nothing(&content) {
        let text = if is_error {
            format!("[tool error] {NO_TOOL_OUTPUT}")
        } else {
            NO_TOOL_OUTPUT.to_owned()
        };
        return vec![ToolResultContent::text(text)];
    }
    if parts || texts(&content) < 2 {
        return content;
    }
    let mut joined: Vec<String> = Vec::new();
    let mut images = Vec::new();
    for part in content {
        match part {
            ToolResultContent::Text(text) => joined.push(text.text),
            ToolResultContent::Json { value } => joined.push(value.to_string()),
            image @ ToolResultContent::Image(_) => images.push(image),
        }
    }
    std::iter::once(ToolResultContent::text(joined.join("\n")))
        .chain(images)
        .collect()
}

fn says_nothing(content: &[ToolResultContent]) -> bool {
    content.iter().all(|part| match part {
        ToolResultContent::Text(text) => text.text.trim().is_empty(),
        ToolResultContent::Json { .. } | ToolResultContent::Image(_) => false,
    })
}

fn texts(content: &[ToolResultContent]) -> usize {
    content
        .iter()
        .filter(|part| !matches!(part, ToolResultContent::Image(_)))
        .count()
}

fn result_text(result: &ToolResult) -> String {
    let text: Vec<String> = result
        .content
        .iter()
        .map(|part| match part {
            ToolResultContent::Text(text) => text.text.clone(),
            ToolResultContent::Json { value } => value.to_string(),
            ToolResultContent::Image(_) => TOOL_IMAGE_OMITTED.to_owned(),
        })
        .collect();
    let kind = if result.is_error { "error" } else { "result" };
    format!("[tool {} {kind}] {}", result.name, text.join("\n"))
}

fn result_images(
    content: Vec<ToolResultContent>,
    form: &Form<'_>,
    attached: &mut Vec<crate::codecs::message::Image>,
) -> Vec<ToolResultContent> {
    let mut shaped: Vec<ToolResultContent> = Vec::with_capacity(content.len());
    for part in content {
        match part {
            ToolResultContent::Image(image) => {
                let image = sendable_image(image);
                if form.sends(&image, Place::ToolResult) {
                    shaped.push(ToolResultContent::Image(image));
                    continue;
                }
                let placeholder = if form.sends(&image, Place::User) {
                    attached.push(image);
                    TOOL_IMAGE_ATTACHED
                } else {
                    TOOL_IMAGE_OMITTED
                };
                if shaped.last().and_then(ToolResultContent::as_text) != Some(placeholder) {
                    shaped.push(ToolResultContent::text(placeholder));
                }
            }
            part => shaped.push(part),
        }
    }
    shaped
}

fn sendable(source: DocumentSourceKind) -> DocumentSourceKind {
    match source {
        DocumentSourceKind::Raw(bytes) => DocumentSourceKind::Base64(BASE64_STANDARD.encode(bytes)),
        source => source,
    }
}

fn sendable_as_is(image: &crate::codecs::message::Image) -> bool {
    match &image.data {
        DocumentSourceKind::Raw(_) => false,
        DocumentSourceKind::Base64(data) => image.media_type.is_some() || sniffed(data).is_none(),
        _ => true,
    }
}

fn sendable_image(mut image: crate::codecs::message::Image) -> crate::codecs::message::Image {
    image.data = sendable(image.data);
    if image.media_type.is_none()
        && let DocumentSourceKind::Base64(data) = &image.data
    {
        image.media_type = sniffed(data);
    }
    image
}

fn sniffed(data: &str) -> Option<ImageMediaType> {
    let head: String = data.chars().take(24).collect();
    let bytes = BASE64_STANDARD
        .decode(head.as_bytes())
        .or_else(|_| BASE64_STANDARD_NO_PAD.decode(head.as_bytes()))
        .ok()?;
    ImageMediaType::sniff(&bytes)
}

fn merge_users(history: Vec<Message>) -> Vec<Message> {
    let mut merged: Vec<Message> = Vec::with_capacity(history.len());
    for message in history {
        match (merged.last_mut(), message) {
            (Some(Message::User { content: previous }), Message::User { content })
                if !previous.is_empty() && !content.is_empty() =>
            {
                previous.extend(content);
            }
            (_, message) => merged.push(message),
        }
    }
    merged
}
