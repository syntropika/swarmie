use std::borrow::{Borrow, Cow};
use std::collections::{HashMap, HashSet};

use crate::codecs::message::{AssistantContent, CallId, Message, UserContent};

#[derive(Debug, Default)]
pub struct WireIds<'h> {
    by_call: HashMap<&'h CallId, Cow<'h, str>>,
}

impl<'h> WireIds<'h> {
    pub fn for_target(
        history: &'h [impl Borrow<Message>],
        target: &dyn crate::codecs::completion::ReplayTarget,
        model: &str,
    ) -> Self {
        let mut ids: Vec<&CallId> = Vec::new();
        let mut used: HashSet<Cow<'h, str>> = HashSet::new();
        for message in history {
            match message.borrow() {
                Message::Assistant(turn) => {
                    for block in &turn.content {
                        match block {
                            AssistantContent::ToolCall(call) => ids.push(&call.id),
                            AssistantContent::Opaque(opaque) => used.extend(
                                target
                                    .hosted_pair(&opaque.item)
                                    .map(|(_, id)| Cow::Owned(id)),
                            ),
                            _ => {}
                        }
                    }
                }
                Message::User { content } => {
                    ids.extend(content.iter().filter_map(|part| match part {
                        UserContent::ToolResult(result) => Some(&result.call),
                        _ => None,
                    }))
                }
                Message::System { .. } => {}
            }
        }
        used.extend(
            ids.iter()
                .filter_map(|id| id.provider().map(|id| Cow::Borrowed(id.as_str()))),
        );
        let mut by_call = HashMap::new();
        let mut next = 0usize;
        for id in ids {
            if by_call.contains_key(id) {
                continue;
            }
            let spelled = match id {
                CallId::Provider(provider) => Cow::Borrowed(provider.as_str()),
                CallId::Local(_) => loop {
                    let alias = target.normalize_tool_call_id(&format!("tool-{next}"), model, None);
                    next += 1;
                    if used.insert(Cow::Owned(alias.clone())) {
                        break Cow::Owned(alias);
                    }
                },
            };
            by_call.insert(id, spelled);
        }
        Self { by_call }
    }

    pub fn of(&self, call: &CallId) -> Option<&str> {
        self.by_call.get(call).map(|spelled| &**spelled)
    }

    pub fn spell(&self, call: &CallId) -> String {
        self.of(call)
            .map_or_else(|| call.wire().into_owned(), str::to_owned)
    }
}

pub fn legal_call_id(id: &str, max: usize) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(max)
        .collect()
}

pub(crate) fn short_hash(text: &str) -> String {
    fn base36(mut value: u32) -> String {
        let mut digits = Vec::new();
        loop {
            digits.push(char::from_digit(value % 36, 36).unwrap_or('0'));
            value /= 36;
            if value == 0 {
                break;
            }
        }
        digits.iter().rev().collect()
    }
    let (mut h1, mut h2) = (0xdead_beef_u32, 0x41c6_ce57_u32);
    for unit in text.encode_utf16().map(u32::from) {
        h1 = (h1 ^ unit).wrapping_mul(2_654_435_761);
        h2 = (h2 ^ unit).wrapping_mul(1_597_334_677);
    }
    h1 = (h1 ^ (h1 >> 16)).wrapping_mul(2_246_822_507)
        ^ (h2 ^ (h2 >> 13)).wrapping_mul(3_266_489_909);
    h2 = (h2 ^ (h2 >> 16)).wrapping_mul(2_246_822_507)
        ^ (h1 ^ (h1 >> 13)).wrapping_mul(3_266_489_909);
    format!("{}{}", base36(h2), base36(h1))
}
