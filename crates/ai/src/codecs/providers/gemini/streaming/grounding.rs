use serde_json::Value;

use crate::codecs::json_utils::Lenient;
use crate::codecs::message::{Source, SourceLocation};
use crate::codecs::wire::{SpanUnit, WireCitation, WireSpan};

#[derive(Debug, Default)]
pub(super) struct AnswerText {
    blocks: Vec<(usize, String)>,
}

impl AnswerText {
    pub(super) fn open(&mut self, index: usize, text: &str) {
        self.blocks.push((index, text.to_owned()));
    }

    pub(super) fn push(&mut self, index: usize, text: &str) -> Option<usize> {
        let (_, held) = self.blocks.iter_mut().find(|(at, _)| *at == index)?;
        let offset = held.len();
        held.push_str(text);
        Some(offset)
    }

    fn locate(&self, at: usize) -> Option<(usize, usize)> {
        let mut start = 0;
        for (index, text) in &self.blocks {
            if at < start + text.len() {
                return Some((*index, at - start));
            }
            start += text.len();
        }
        None
    }

    fn absolute(&self, index: usize, offset: usize) -> Option<usize> {
        let mut start: usize = 0;
        for (at, text) in &self.blocks {
            if *at == index {
                return start.checked_add(offset);
            }
            start += text.len();
        }
        None
    }

    fn holds(&self, index: usize, range: std::ops::Range<usize>, quoted: &str) -> bool {
        self.blocks
            .iter()
            .find(|(at, _)| *at == index)
            .and_then(|(_, text)| text.get(range))
            == Some(quoted)
    }

    fn find(&self, quoted: &str, near: usize) -> Option<(usize, usize)> {
        let mut start = 0;
        let mut best: Option<(usize, usize, usize)> = None;
        for (index, text) in &self.blocks {
            for (offset, _) in text.match_indices(quoted) {
                let distance = (start + offset).abs_diff(near);
                if best.is_none_or(|(_, _, held)| distance < held) {
                    best = Some((*index, offset, distance));
                }
            }
            start += text.len();
        }
        best.map(|(index, offset, _)| (index, offset))
    }
}

pub(super) fn grounding(
    metadata: &Value,
    placed: &[Option<(usize, usize)>],
    answer: &AnswerText,
    streamed: bool,
) -> Vec<(usize, WireCitation)> {
    let chunks = metadata.arr("groundingChunks");
    metadata
        .arr("groundingSupports")
        .iter()
        .filter_map(|support| {
            let scores = support.arr("confidenceScores");
            let sources: Vec<Source> = support
                .arr("groundingChunkIndices")
                .iter()
                .enumerate()
                .filter_map(|(at, chunk)| {
                    let chunk = chunks.get(usize::try_from(chunk.as_u64()?).ok()?)?;
                    let source = chunk_source(chunk)?;
                    Some(match scores.get(at).and_then(Value::as_f64) {
                        Some(score) => source.confidence(score as f32),
                        None => source,
                    })
                })
                .collect();
            if sources.is_empty() {
                return None;
            }
            let (index, span) = segment(support.get("segment")?, placed, answer, streamed)?;
            Some((index, WireCitation::new(Some(span), sources)))
        })
        .collect()
}

fn segment(
    segment: &Value,
    placed: &[Option<(usize, usize)>],
    answer: &AnswerText,
    streamed: bool,
) -> Option<(usize, WireSpan)> {
    let offset = |key: &str| usize::try_from(segment.u64(key).unwrap_or(0)).ok();
    let (start, end) = (offset("startIndex")?, offset("endIndex")?);
    // Provider bytes: a reversed or overflowing range drops the citation
    // rather than panic.
    let len = end.checked_sub(start)?;
    let in_part = offset("partIndex")
        .and_then(|part| placed.get(part).copied().flatten())
        .and_then(|(index, at)| Some((index, at.checked_add(start)?)));
    let in_answer = answer.locate(start);
    let quoted = segment.str("text").filter(|text| !text.is_empty());
    let (index, at) = match quoted {
        // Nothing to check the span against: a reply over several chunks
        // counts bytes in the whole answer, a whole reply in its part.
        None if streamed => in_answer?,
        None => in_part.or(in_answer)?,
        Some(quoted) => [in_part, in_answer]
            .into_iter()
            .flatten()
            .find(|(index, at)| {
                at.checked_add(len)
                    .is_some_and(|stop| answer.holds(*index, *at..stop, quoted))
            })
            .or_else(|| {
                let near = in_part
                    .and_then(|(index, at)| answer.absolute(index, at))
                    .unwrap_or(start);
                answer.find(quoted, near)
            })
            .or(in_part)
            .or(in_answer)?,
    };
    let span = WireSpan::new(at as u64, at.checked_add(len)? as u64, SpanUnit::Bytes);
    Some((
        index,
        match quoted {
            Some(quoted) => span.quoted(quoted),
            None => span,
        },
    ))
}

fn chunk_source(chunk: &Value) -> Option<Source> {
    let (kind, fields) = ["web", "retrievedContext", "maps"]
        .into_iter()
        .find_map(|kind| chunk.get(kind).map(|fields| (kind, fields)))?;
    let location = match (fields.str("uri"), kind) {
        (Some(uri), _) => SourceLocation::Url {
            url: uri.to_owned(),
        },
        (None, "retrievedContext") => SourceLocation::Document {
            index: None,
            id: fields.str("documentName").map(str::to_owned),
            within: None,
        },
        (None, _) => return None,
    };
    let mut source = Source::new(location);
    if let Some(title) = fields.str("title") {
        source = source.title(title);
    }
    if kind == "retrievedContext"
        && let Some(text) = fields.str("text")
    {
        source = source.cited_text(text);
    }
    Some(source)
}

pub(super) fn recitations(metadata: &Value, answer: &AnswerText) -> Vec<(usize, WireCitation)> {
    let source = |citation: &Value| {
        let mut source = Source::new(SourceLocation::Url {
            url: citation.str("uri")?.to_owned(),
        });
        if let Some(title) = citation.str("title") {
            source = source.title(title);
        }
        Some(source)
    };
    let gemini = metadata
        .arr("citationSources")
        .iter()
        .filter_map(|citation| {
            let start = usize::try_from(citation.u64("startIndex").unwrap_or(0)).ok()?;
            let end = usize::try_from(citation.u64("endIndex")?).ok()?;
            let len = end.checked_sub(start)?;
            let (index, at) = answer.locate(start)?;
            let span = WireSpan::new(at as u64, at.checked_add(len)? as u64, SpanUnit::Bytes);
            Some((
                index,
                WireCitation::new(Some(span), vec![source(citation)?]),
            ))
        });
    let first = answer.blocks.first().map(|(index, _)| *index);
    let vertex = metadata
        .arr("citations")
        .iter()
        .filter_map(|citation| Some((first?, WireCitation::new(None, vec![source(citation)?]))));
    gemini.chain(vertex).collect()
}
