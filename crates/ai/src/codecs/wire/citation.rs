use crate::codecs::message::Source;

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub struct WireCitation {
    pub span: Option<WireSpan>,
    pub sources: Vec<Source>,
}

impl WireCitation {
    pub fn new(span: Option<WireSpan>, sources: Vec<Source>) -> Self {
        Self { span, sources }
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WireSpan {
    pub start: u64,
    pub end: u64,
    pub unit: SpanUnit,
    pub quoted: Option<String>,
}

impl WireSpan {
    pub fn new(start: u64, end: u64, unit: SpanUnit) -> Self {
        Self {
            start,
            end,
            unit,
            quoted: None,
        }
    }

    pub fn quoted(mut self, text: impl Into<String>) -> Self {
        self.quoted = Some(text.into());
        self
    }
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SpanUnit {
    Bytes,
    Chars,
    Utf16,
}
