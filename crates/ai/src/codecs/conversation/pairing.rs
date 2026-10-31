use std::borrow::Borrow;

use crate::codecs::message::{
    AssistantContent, AssistantMessage, CallId, Message, ToolCall, ToolName, UserContent,
};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PairingError {
    #[error("consecutive assistant messages at index {index}")]
    ConsecutiveAssistant { index: usize },
    #[error("tool call `{call_id}` at index {index} has no result in the following message")]
    UnansweredToolCall { index: usize, call_id: CallId },
    #[error(
        "tool result `{call_id}` at index {index} answers no call from the preceding assistant message"
    )]
    OrphanToolResult { index: usize, call_id: CallId },
}

pub fn validate(messages: &[impl Borrow<Message>]) -> Result<(), PairingError> {
    repair(
        messages
            .iter()
            .map(|message| message.borrow().clone())
            .collect(),
    )
    .repairs
    .into_iter()
    .next()
    .map_or(Ok(()), Err)
}

pub const NO_RESULT_PROVIDED: &str = "No result provided";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pairing {
    pub stored: bool,
    pub answers: bool,
}

impl Pairing {
    pub const fn canonical() -> Self {
        Self {
            stored: false,
            answers: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Paired {
    pub messages: Vec<Message>,
    pub repairs: Vec<PairingError>,
}

pub fn repair(messages: Vec<Message>) -> Paired {
    let shaped = shape(messages, true);
    Paired {
        messages: shaped.messages,
        repairs: shaped.repairs,
    }
}

#[derive(Debug, Clone, PartialEq)]

struct Shaped {
    messages: Vec<Message>,
    sources: Vec<Vec<usize>>,
    repairs: Vec<PairingError>,
}

fn shape(messages: Vec<Message>, merge_users: bool) -> Shaped {
    let canonical = |messages: Vec<Message>| {
        let history = messages.into_iter().map(Some).collect();
        walk(history, Pairing::canonical(), merge_users, true)
    };
    let mut shaped = canonical(messages);
    // Each merge removes a turn, so this ends.
    while merge_turns(&mut shaped) {
        let again = canonical(std::mem::take(&mut shaped.messages));
        let sources = again.sources.iter().map(|sources| {
            let earlier = sources.iter().filter_map(|at| shaped.sources.get(*at));
            earlier.flatten().copied().collect()
        });
        shaped.sources = sources.collect();
        shaped.messages = again.messages;
    }
    shaped
}

fn merge_turns(shaped: &mut Shaped) -> bool {
    let before = shaped.messages.len();
    let mut messages: Vec<Message> = Vec::with_capacity(before);
    let mut sources: Vec<Vec<usize>> = Vec::with_capacity(before);
    let all = shaped.messages.drain(..).zip(shaped.sources.drain(..));
    for (mut message, mut from) in all {
        if let Message::Assistant(turn) = &mut message
            && let Some(Message::Assistant(previous)) = messages.last_mut()
        {
            previous.content.append(&mut turn.content);
            previous.stop = turn.stop.take();
            if let Some(into) = sources.last_mut() {
                into.append(&mut from);
            }
            continue;
        }
        messages.push(message);
        sources.push(from);
    }
    shaped.messages = messages;
    shaped.sources = sources;
    shaped.messages.len() < before
}

pub fn pair(history: Vec<Option<Message>>, pairing: Pairing) -> Paired {
    let shaped = walk(history, pairing, true, false);
    Paired {
        messages: shaped.messages,
        repairs: shaped.repairs,
    }
}

fn walk(
    history: Vec<Option<Message>>,
    pairing: Pairing,
    merge_users: bool,
    sourced: bool,
) -> Shaped {
    // The ids results before the first turn took, when calls are stored.
    let mut stored = pairing.stored.then(Vec::new);
    let mut walk = Walk {
        answers: pairing.answers,
        merge_users,
        sourced,
        ..Walk::default()
    };
    for step in steps(history) {
        match step {
            Step::Turn(index, turn) => {
                if walk.after_assistant && turn.is_some() {
                    walk.repairs
                        .push(PairingError::ConsecutiveAssistant { index });
                }
                walk.close();
                stored = None;
                walk.gap = turn.is_none();
                walk.after_assistant = turn.is_some();
                let Some(turn) = turn else { continue };
                if !turn.stop.as_ref().is_some_and(|stop| stop.is_failure()) {
                    let calls = turn.tool_calls();
                    let calls = calls.map(|call| (call.id.clone(), call.function.name.clone()));
                    walk.waiting = calls.collect();
                    walk.waiting_at = index;
                }
                walk.put(Message::Assistant(turn), [index]);
            }
            Step::System(index, system) => {
                walk.after_assistant = false;
                if walk.waiting.is_empty() {
                    walk.gap = false;
                    walk.put(system, [index]);
                } else {
                    walk.held.push((index, system));
                }
            }
            Step::User(first, parts) => {
                walk.after_assistant = false;
                if parts.is_empty() {
                    walk.close();
                    let empty = Message::User {
                        content: Vec::new(),
                    };
                    walk.put(empty, [first]);
                    walk.gap = false;
                    continue;
                }
                let mut content = Vec::with_capacity(parts.len());
                for (index, part) in parts {
                    let UserContent::ToolResult(result) = part else {
                        content.push((Some(index), part));
                        continue;
                    };
                    let answers = match &mut stored {
                        Some(taken) if !taken.contains(&result.call) => {
                            taken.push(result.call.clone());
                            true
                        }
                        Some(_) => false,
                        None => answer(&mut walk.waiting, &result.call, |(id, _)| id),
                    };
                    if answers {
                        content.push((Some(index), UserContent::ToolResult(result)));
                    } else {
                        walk.repairs.push(PairingError::OrphanToolResult {
                            index,
                            call_id: result.call,
                        });
                    }
                }
                if content.is_empty() && walk.waiting.is_empty() {
                    // Only results nothing waits for: the gap stays open.
                    continue;
                }
                let only_results =
                    !content.is_empty() && content.iter().all(|(_, part)| is_result(part));
                if walk.pending.is_empty() {
                    walk.pending_gap = walk.gap;
                }
                walk.pending.extend(content);
                walk.gap = false;
                // Hold a system message while calls wait, so results
                // split around one still answer the turn.
                if only_results && !walk.waiting.is_empty() {
                    continue;
                }
                walk.close();
            }
        }
    }
    walk.close();
    Shaped {
        messages: walk.shaped,
        sources: walk.sources,
        repairs: walk.repairs,
    }
}

pub(crate) fn is_result(part: &UserContent) -> bool {
    matches!(part, UserContent::ToolResult(_))
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnswerError {
    #[error("received content that is not a tool result")]
    NotAResult,
    #[error("received a result for unknown or already-answered tool call id `{0}`")]
    Unknown(CallId),
    #[error("left pending tool call id(s) unanswered: {0:?}")]
    Unanswered(Vec<CallId>),
}

pub fn answers(pending: &[CallId], results: &[UserContent]) -> Result<(), AnswerError> {
    let mut waiting = pending.to_vec();
    for result in results {
        let UserContent::ToolResult(result) = result else {
            return Err(AnswerError::NotAResult);
        };
        if !answer(&mut waiting, &result.call, |id| id) {
            return Err(AnswerError::Unknown(result.call.clone()));
        }
    }
    if waiting.is_empty() {
        Ok(())
    } else {
        Err(AnswerError::Unanswered(waiting))
    }
}

pub fn close_pending<'a>(calls: impl IntoIterator<Item = &'a ToolCall>) -> Message {
    close_pending_with(calls, NO_RESULT_PROVIDED)
}

pub fn close_pending_with<'a>(calls: impl IntoIterator<Item = &'a ToolCall>, why: &str) -> Message {
    let calls = calls.into_iter();
    Message::User {
        content: unanswered(calls.map(|call| (&call.id, &call.function.name)), why),
    }
}

fn unanswered<'a>(
    calls: impl IntoIterator<Item = (&'a CallId, &'a ToolName)>,
    why: &str,
) -> Vec<UserContent> {
    calls
        .into_iter()
        .map(|(id, name)| {
            UserContent::ToolResult(crate::codecs::message::ToolResult {
                call: id.clone(),
                name: name.clone(),
                content: vec![crate::codecs::message::ToolResultContent::text(why)],
                is_error: true,
            })
        })
        .collect()
}

fn answer<T>(waiting: &mut Vec<T>, id: &CallId, id_of: impl Fn(&T) -> &CallId) -> bool {
    match waiting.iter().position(|call| id_of(call) == id) {
        Some(at) => {
            waiting.remove(at);
            true
        }
        None => false,
    }
}

enum Step {
    Turn(usize, Option<AssistantMessage>),
    System(usize, Message),
    User(usize, Vec<(usize, UserContent)>),
}

fn steps(history: Vec<Option<Message>>) -> Vec<Step> {
    let mut steps: Vec<Step> = Vec::with_capacity(history.len());
    for (index, message) in history.into_iter().enumerate() {
        match message {
            None => steps.push(Step::Turn(index, None)),
            Some(Message::Assistant(turn)) => steps.push(Step::Turn(index, Some(turn))),
            Some(system @ Message::System { .. }) => steps.push(Step::System(index, system)),
            Some(Message::User { content }) => {
                let joins = !content.is_empty();
                let parts = content.into_iter().map(|part| (index, part));
                match steps.last_mut() {
                    Some(Step::User(_, previous)) if joins && !previous.is_empty() => {
                        previous.extend(parts)
                    }
                    _ => steps.push(Step::User(index, parts.collect())),
                }
            }
        }
    }
    steps
}

#[derive(Default)]
struct Walk {
    answers: bool,
    merge_users: bool,
    sourced: bool,
    shaped: Vec<Message>,
    sources: Vec<Vec<usize>>,
    repairs: Vec<PairingError>,
    waiting: Vec<(CallId, ToolName)>,
    waiting_at: usize,
    held: Vec<(usize, Message)>,
    gap: bool,
    pending: Vec<(Option<usize>, UserContent)>,
    pending_gap: bool,
    after_assistant: bool,
}

impl Walk {
    fn put(&mut self, message: Message, sources: impl IntoIterator<Item = usize>) {
        self.shaped.push(message);
        if self.sourced {
            self.sources.push(sources.into_iter().collect());
        }
    }

    fn close(&mut self) {
        let mut content = std::mem::take(&mut self.pending);
        let merge = std::mem::take(&mut self.pending_gap);
        let waiting = std::mem::take(&mut self.waiting);
        let missing =
            if self.answers {
                let index = self.waiting_at;
                self.repairs.extend(waiting.iter().map(|(id, _)| {
                    PairingError::UnansweredToolCall {
                        index,
                        call_id: id.clone(),
                    }
                }));
                unanswered(
                    waiting.iter().map(|(id, name)| (id, name)),
                    NO_RESULT_PROVIDED,
                )
            } else {
                Vec::new()
            };
        if !self.merge_users {
            self.close_apart(content, missing);
            return;
        }
        let at = content
            .iter()
            .position(|(_, part)| !is_result(part))
            .unwrap_or(content.len());
        content.splice(at..at, missing.into_iter().map(|part| (None, part)));
        // Results come first: Anthropic requires it, and Chat sends them as
        // tool messages that must follow the turn directly.
        content.sort_by_key(|(_, part)| !is_result(part));
        // A held system message goes right after the results, before the
        // user's own text.
        let at = content
            .iter()
            .position(|(_, part)| !is_result(part))
            .unwrap_or(content.len());
        let text = if self.held.is_empty() {
            Vec::new()
        } else {
            content.split_off(at)
        };
        if !content.is_empty() {
            let (sources, content) = sourced(content);
            match self.shaped.last_mut() {
                // Only `pair` leaves a gap to merge across, and it keeps no
                // sources.
                Some(Message::User { content: previous }) if merge => previous.extend(content),
                _ => self.put(Message::User { content }, sources),
            }
        }
        for (index, system) in std::mem::take(&mut self.held) {
            self.put(system, [index]);
        }
        if !text.is_empty() {
            let (sources, content) = sourced(text);
            self.put(Message::User { content }, sources);
        }
    }

    fn close_apart(
        &mut self,
        content: Vec<(Option<usize>, UserContent)>,
        missing: Vec<UserContent>,
    ) {
        let mut messages: Vec<(Option<usize>, Vec<UserContent>)> = Vec::new();
        for (source, part) in content {
            match messages.last_mut() {
                Some((last, parts)) if *last == source => parts.push(part),
                _ => messages.push((source, vec![part])),
            }
        }
        match messages.first_mut() {
            Some((_, parts)) => {
                let at = parts.iter().position(|part| !is_result(part));
                let at = at.unwrap_or(parts.len());
                parts.splice(at..at, missing);
            }
            None if !missing.is_empty() => messages.push((None, missing)),
            None => {}
        }
        for (source, mut content) in messages {
            content.sort_by_key(|part| !is_result(part));
            self.put(Message::User { content }, source);
        }
        for (index, system) in std::mem::take(&mut self.held) {
            self.put(system, [index]);
        }
    }
}

fn sourced(content: Vec<(Option<usize>, UserContent)>) -> (Vec<usize>, Vec<UserContent>) {
    let mut sources: Vec<usize> = Vec::new();
    let parts = content.into_iter().map(|(source, part)| {
        if let Some(source) = source
            && !sources.contains(&source)
        {
            sources.push(source);
        }
        part
    });
    let parts = parts.collect();
    (sources, parts)
}

pub fn pending_calls(messages: &[impl Borrow<Message>]) -> Vec<ToolCall> {
    let Some((at, Message::Assistant(reply))) = messages
        .iter()
        .map(Borrow::borrow)
        .enumerate()
        .rfind(|(_, message)| matches!(message, Message::Assistant(_)))
    else {
        return Vec::new();
    };
    let answered: Vec<&CallId> = messages
        .iter()
        .skip(at + 1)
        .flat_map(|message| match message.borrow() {
            Message::User { content } => content.as_slice(),
            _ => &[],
        })
        .filter_map(|item| match item {
            UserContent::ToolResult(result) => Some(&result.call),
            _ => None,
        })
        .collect();
    reply
        .content
        .iter()
        .filter_map(|item| match item {
            AssistantContent::ToolCall(call) if !answered.contains(&&call.id) => Some(call.clone()),
            _ => None,
        })
        .collect()
}

pub fn safe_start(messages: &[impl Borrow<Message>], at: usize) -> usize {
    let mut start = at.min(messages.len());
    for (index, message) in messages.iter().enumerate().skip(at) {
        match message.borrow() {
            Message::Assistant(_) => break,
            Message::User { content }
                if content
                    .iter()
                    .any(|item| matches!(item, UserContent::ToolResult(_))) =>
            {
                start = index + 1;
            }
            _ => {}
        }
    }
    start
}
