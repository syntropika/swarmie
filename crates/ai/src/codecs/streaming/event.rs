use serde::{Deserialize, Serialize};

use crate::codecs::message::{AssistantContent, ToolName};

use super::UnknownPayload;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Part(u32);

impl Part {
    pub const fn new(index: u32) -> Self {
        Self(index)
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartKind {
    Text,
    Reasoning,
    ToolCall,
    Image,
    Opaque,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum StreamEvent {
    Start {
        part: Part,
        kind: PartKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<ToolName>,
    },
    Text {
        part: Part,
        text: String,
    },
    Reasoning {
        part: Part,
        text: String,
    },
    Arguments {
        part: Part,
        json: String,
    },
    End {
        part: Part,
        content: Box<AssistantContent>,
    },
}

impl StreamEvent {
    pub const fn part(&self) -> Part {
        match self {
            Self::Start { part, .. }
            | Self::Text { part, .. }
            | Self::Reasoning { part, .. }
            | Self::Arguments { part, .. }
            | Self::End { part, .. } => *part,
        }
    }

    pub const fn name(&self) -> &'static str {
        match self {
            Self::Start { .. } => "Start",
            Self::Text { .. } => "Text",
            Self::Reasoning { .. } => "Reasoning",
            Self::Arguments { .. } => "Arguments",
            Self::End { .. } => "End",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "item", content = "value", rename_all = "snake_case")]
pub enum Item<E> {
    Event(E),
    Unknown(UnknownPayload),
}

#[derive(Debug, Clone, Default)]
pub struct StreamRecord {
    items: Vec<Item<StreamEvent>>,
    parts: std::collections::BTreeMap<usize, (PartKind, bool)>,
}

impl PartialEq for StreamRecord {
    fn eq(&self, other: &Self) -> bool {
        self.items == other.items
    }
}

impl Serialize for StreamRecord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.items.serialize(serializer)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SequenceError {
    #[error("not a list of stream items: {0}")]
    Shape(String),
    #[error("item {0} names a part that has not started")]
    UnknownPart(usize),
    #[error("item {0} touches a part that already ended")]
    EndedTwice(usize),
    #[error("item {0} does not match its part's kind")]
    WrongKind(usize),
    #[error("part {0} never ended")]
    Unclosed(usize),
}

impl StreamRecord {
    pub fn parse(value: serde_json::Value) -> Result<Self, SequenceError> {
        let transcript = Self::parse_prefix(value)?;
        transcript.check_closed()?;
        Ok(transcript)
    }

    pub fn parse_prefix(value: serde_json::Value) -> Result<Self, SequenceError> {
        let items: Vec<Item<StreamEvent>> = serde_json::from_value(value)
            .map_err(|error| SequenceError::Shape(error.to_string()))?;
        Self::from_items(items)
    }

    pub fn push(&mut self, item: Item<StreamEvent>) -> Result<(), SequenceError> {
        let position = self.items.len();
        if let Item::Event(event) = &item {
            let index = event.part().index();
            let kind = match event {
                StreamEvent::Start { kind, name, .. } => {
                    if (*kind == PartKind::ToolCall) != name.is_some() {
                        return Err(SequenceError::WrongKind(position));
                    }
                    if self.parts.insert(index, (*kind, false)).is_some() {
                        return Err(SequenceError::UnknownPart(position));
                    }
                    None
                }
                StreamEvent::Text { .. } => Some(PartKind::Text),
                StreamEvent::Reasoning { .. } => Some(PartKind::Reasoning),
                StreamEvent::Arguments { .. } => Some(PartKind::ToolCall),
                StreamEvent::End { content, .. } => Some(match content.as_ref() {
                    AssistantContent::Text(_) => PartKind::Text,
                    AssistantContent::Reasoning(_) => PartKind::Reasoning,
                    AssistantContent::ToolCall(_) => PartKind::ToolCall,
                    AssistantContent::Image(_) => PartKind::Image,
                    AssistantContent::Opaque(_) => PartKind::Opaque,
                }),
            };
            if let Some(kind) = kind {
                match self.parts.get_mut(&index) {
                    None => return Err(SequenceError::UnknownPart(position)),
                    Some((_, true)) => return Err(SequenceError::EndedTwice(position)),
                    Some((open, false)) if *open != kind => {
                        return Err(SequenceError::WrongKind(position));
                    }
                    Some((_, ended)) => *ended = matches!(event, StreamEvent::End { .. }),
                }
            }
        }
        self.items.push(item);
        Ok(())
    }

    fn check_closed(&self) -> Result<(), SequenceError> {
        match self.parts.iter().find(|(_, (_, ended))| !ended) {
            Some((part, _)) => Err(SequenceError::Unclosed(*part)),
            None => Ok(()),
        }
    }

    pub fn from_items(items: Vec<Item<StreamEvent>>) -> Result<Self, SequenceError> {
        let mut transcript = Self::default();
        for item in items {
            transcript.push(item)?;
        }
        Ok(transcript)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn items(&self) -> &[Item<StreamEvent>] {
        &self.items
    }

    pub fn into_items(self) -> Vec<Item<StreamEvent>> {
        self.items
    }

    pub fn events(&self) -> impl Iterator<Item = &StreamEvent> {
        self.items.iter().filter_map(|item| match item {
            Item::Event(event) => Some(event),
            Item::Unknown(_) => None,
        })
    }
}

impl From<StreamRecord> for Vec<Item<StreamEvent>> {
    fn from(transcript: StreamRecord) -> Self {
        transcript.items
    }
}

impl<'de> Deserialize<'de> for StreamRecord {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let items = Vec::<Item<StreamEvent>>::deserialize(deserializer)
            .map_err(|error| serde::de::Error::custom(SequenceError::Shape(error.to_string())))?;
        Self::from_items(items).map_err(serde::de::Error::custom)
    }
}

// The stream vocabulary crosses threads on every target: a handler sends it
// over a channel, the effect log records it, and a client reads it back.
const _: fn() = || {
    fn assert_wire<T: Clone + Send + Sync + 'static + Serialize + serde::de::DeserializeOwned>() {}
    assert_wire::<StreamEvent>();
    assert_wire::<StreamRecord>();
    assert_wire::<Item<StreamEvent>>();
};
