use std::ops::Range;

use serde::{Deserialize, Serialize};

use super::{Fingerprint, Text};
use crate::codecs::wire::{SpanUnit, WireCitation, WireSpan};

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Citation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
    #[serde(default)]
    pub sources: Vec<Source>,
}

impl Citation {
    pub fn new(sources: impl IntoIterator<Item = Source>) -> Self {
        Self {
            span: None,
            sources: sources.into_iter().collect(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Span {
    start: usize,
    end: usize,
}

impl Span {
    pub fn start(&self) -> usize {
        self.start
    }

    pub fn end(&self) -> usize {
        self.end
    }

    pub fn range(&self) -> Range<usize> {
        self.start..self.end
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub location: SourceLocation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cited_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
}

impl Source {
    pub fn new(location: SourceLocation) -> Self {
        Self {
            location,
            title: None,
            cited_text: None,
            confidence: None,
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn cited_text(mut self, text: impl Into<String>) -> Self {
        self.cited_text = Some(text.into());
        self
    }

    pub fn confidence(mut self, confidence: f32) -> Self {
        self.confidence = Some(confidence);
        self
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SourceLocation {
    Document {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        index: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        within: Option<DocumentRange>,
    },
    Url {
        url: String,
    },
    File {
        file_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        container_id: Option<String>,
    },
    SearchResult {
        index: u32,
        source: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        blocks: Option<Range<u32>>,
    },
    ToolOutput {
        id: String,
    },
}

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "unit", rename_all = "snake_case")]
pub enum DocumentRange {
    Chars(Range<u64>),
    Pages(Range<u32>),
    Blocks(Range<u32>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct Citations {
    fingerprint: Fingerprint,
    list: Vec<Citation>,
}

pub(super) fn lenient<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Citations>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| Citations::deserialize(value).ok()))
}

fn fingerprint(text: &str) -> Fingerprint {
    Fingerprint::of(&text)
}

fn fits(text: &str, citation: &Citation) -> bool {
    citation
        .span
        .is_none_or(|span| text.get(span.range()).is_some())
}

impl Text {
    pub fn citations(&self) -> &[Citation] {
        match &self.citations {
            Some(citations) if citations.fingerprint == fingerprint(&self.text) => &citations.list,
            _ => &[],
        }
    }

    pub fn with_citations(mut self, citations: impl IntoIterator<Item = Citation>) -> Self {
        let list = citations
            .into_iter()
            .filter(|citation| {
                let fits = fits(&self.text, citation);
                if !fits {
                    tracing::warn!(
                        span = ?citation.span,
                        "dropped a citation whose span does not fit its text"
                    );
                }
                fits
            })
            .collect();
        self.bind(list);
        self
    }

    pub fn cited(&self, citation: &Citation) -> Option<&str> {
        match citation.span {
            Some(span) => self.text.get(span.range()),
            None => Some(&self.text),
        }
    }

    pub fn span(&self, range: Range<usize>) -> Option<Span> {
        self.text.get(range.clone()).map(|_| Span {
            start: range.start,
            end: range.end,
        })
    }

    fn bind(&mut self, list: Vec<Citation>) {
        self.citations = (!list.is_empty()).then(|| Citations {
            fingerprint: fingerprint(&self.text),
            list,
        });
    }

    pub(super) fn checked(mut self) -> Self {
        if let Some(citations) = &mut self.citations {
            citations.list.retain(|citation| fits(&self.text, citation));
            if citations.list.is_empty() {
                self.citations = None;
            }
        }
        self
    }
}

pub(crate) fn attach(
    text: &mut Text,
    kept: Vec<Citation>,
    wire: Vec<WireCitation>,
    provider: &str,
    index: usize,
) {
    let mut list = kept;
    for citation in wire {
        let WireCitation { span, sources } = citation;
        let span = match span.map(|span| resolve(&text.text, &span)).transpose() {
            Ok(span) => span,
            Err(reason) => {
                tracing::warn!(provider, index, reason, "dropped a citation");
                continue;
            }
        };
        list.push(Citation { span, sources });
    }
    text.bind(list);
}

fn resolve(text: &str, span: &WireSpan) -> Result<Span, &'static str> {
    let wire = |offset: u64| usize::try_from(offset).ok();
    let (Some(start), Some(end)) = (wire(span.start), wire(span.end)) else {
        return Err("the span is past the end of the text");
    };
    let byte = |offset: usize| match span.unit {
        SpanUnit::Bytes => Some(offset),
        SpanUnit::Chars => char_offset(text, offset),
        SpanUnit::Utf16 => utf16_offset(text, offset),
    };
    let (Some(start), Some(end)) = (byte(start), byte(end)) else {
        return Err("the span is past the end of the text or splits a character");
    };
    let Some(covered) = text.get(start..end) else {
        return Err("the span is reversed, past the end of the text or splits a character");
    };
    if span
        .quoted
        .as_deref()
        .is_some_and(|quoted| quoted != covered)
    {
        return Err("the span covers other text than the provider quoted");
    }
    Ok(Span { start, end })
}

fn char_offset(text: &str, chars: usize) -> Option<usize> {
    text.char_indices()
        .map(|(at, _)| at)
        .chain(std::iter::once(text.len()))
        .nth(chars)
}

fn utf16_offset(text: &str, units: usize) -> Option<usize> {
    let mut seen = 0;
    for (at, character) in text.char_indices() {
        if seen == units {
            return Some(at);
        }
        if seen > units {
            return None;
        }
        seen += character.len_utf16();
    }
    (seen == units).then_some(text.len())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{Citation, Source, SourceLocation, Span, attach, resolve};
    use crate::codecs::message::{AssistantContent, Text};
    use crate::codecs::wire::{SpanUnit, WireCitation, WireSpan};

    fn url(url: &str) -> Source {
        Source::new(SourceLocation::Url {
            url: url.to_owned(),
        })
    }

    fn span(start: u64, end: u64, unit: SpanUnit) -> WireSpan {
        WireSpan::new(start, end, unit)
    }

    const TEXT: &str = "é 𝄞 Dock Seven";

    #[test]
    fn each_unit_resolves_to_the_same_bytes() {
        let bytes = TEXT.find("Dock").expect("the text holds it");
        let expected = Span {
            start: bytes,
            end: bytes + "Dock Seven".len(),
        };
        for wire in [
            span(8, 18, SpanUnit::Bytes),
            span(4, 14, SpanUnit::Chars),
            span(5, 15, SpanUnit::Utf16),
        ] {
            assert_eq!(
                resolve(TEXT, &wire.clone().quoted("Dock Seven")),
                Ok(expected),
                "{wire:?}"
            );
        }
    }

    #[test]
    fn a_span_that_splits_a_character_or_overruns_does_not_resolve() {
        for wire in [
            span(1, 3, SpanUnit::Bytes),
            span(3, 4, SpanUnit::Utf16),
            span(0, 99, SpanUnit::Chars),
            span(0, 99, SpanUnit::Utf16),
            span(4, 2, SpanUnit::Chars),
            span(0, u64::MAX, SpanUnit::Bytes),
        ] {
            assert!(resolve(TEXT, &wire).is_err(), "{wire:?}");
        }
    }

    #[test]
    fn a_span_must_cover_the_text_the_provider_quoted() {
        // Character offsets read as bytes land elsewhere: the quote catches it.
        let read_as_bytes = span(4, 14, SpanUnit::Bytes).quoted("Dock Seven");
        assert!(resolve(TEXT, &read_as_bytes).is_err());
    }

    #[test]
    fn attach_keeps_whole_block_citations_and_drops_unresolved_ones() {
        let mut text = Text::new(TEXT);
        attach(
            &mut text,
            Vec::new(),
            vec![
                WireCitation::new(None, vec![url("https://a.example")]),
                WireCitation::new(
                    Some(span(4, 14, SpanUnit::Chars).quoted("Dock Seven")),
                    vec![url("https://b.example")],
                ),
                WireCitation::new(
                    Some(span(4, 14, SpanUnit::Chars).quoted("Dock Eight")),
                    vec![url("https://c.example")],
                ),
            ],
            "test",
            0,
        );
        let cited: Vec<_> = text
            .citations()
            .iter()
            .map(|citation| text.cited(citation))
            .collect();
        assert_eq!(cited, [Some(TEXT), Some("Dock Seven")]);
    }

    #[test]
    fn an_edited_text_reads_no_citations() {
        let text = Text::new("Dock Seven");
        let citation = Citation::new([url("https://a.example")]);
        let mut text = text.with_citations([citation]);
        assert_eq!(text.citations().len(), 1);
        text.text.push('!');
        assert!(text.citations().is_empty());
        text.text.pop();
        assert_eq!(
            text.citations().len(),
            1,
            "the original text reads them again"
        );
        text = text.with_citations([]);
        assert!(text.citations().is_empty());
    }

    #[test]
    fn a_span_is_made_only_on_character_boundaries() {
        let text = Text::new(TEXT);
        assert!(text.span(0..2).is_some());
        assert!(text.span(0..1).is_none());
        let (start, end) = (2, 0);
        assert!(text.span(start..end).is_none());
        assert!(text.span(0..99).is_none());

        // A span made for another text is dropped when it does not fit.
        let other = Text::new("a longer text than the first").span(20..28);
        let mut citation = Citation::new([url("https://a.example")]);
        citation.span = other;
        assert!(
            Text::new("short")
                .with_citations([citation])
                .citations()
                .is_empty()
        );
    }

    #[test]
    fn citations_never_enter_the_fingerprint() {
        let plain = AssistantContent::Text(Text::new("Dock Seven"));
        let cited = AssistantContent::Text(
            Text::new("Dock Seven").with_citations([Citation::new([url("https://a.example")])]),
        );
        assert_eq!(plain.fingerprint(), cited.fingerprint());
    }

    #[test]
    fn text_without_citations_serializes_as_before() {
        let block = AssistantContent::Text(Text::new("hi"));
        assert_eq!(
            serde_json::to_value(&block).expect("serializes"),
            json!({"type": "text", "text": "hi"})
        );
    }

    #[test]
    fn citations_round_trip_and_unreadable_ones_load_as_none() {
        let text = Text::new("Dock Seven");
        let mut citation = Citation::new([url("https://a.example").title("Docks")]);
        citation.span = text.span(0..4);
        let text = text.with_citations([citation]);
        let stored = serde_json::to_value(&text).expect("serializes");
        let loaded: Text = serde_json::from_value(stored.clone()).expect("loads");
        assert_eq!(loaded, text);
        assert_eq!(loaded.citations().len(), 1);

        let mut malformed = stored.clone();
        malformed["citations"] = json!({"list": "not a list"});
        let loaded: Text = serde_json::from_value(malformed).expect("loads");
        assert!(loaded.citations().is_empty());
        assert_eq!(loaded.text, "Dock Seven");

        // A stored span that does not fit its text is dropped on load.
        let mut overrun = stored;
        overrun["citations"]["list"][0]["span"] = json!({"start": 0, "end": 99});
        let loaded: Text = serde_json::from_value(overrun).expect("loads");
        assert!(loaded.citations().is_empty());
    }
}
