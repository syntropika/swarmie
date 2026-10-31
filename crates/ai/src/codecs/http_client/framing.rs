use std::borrow::Cow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    Sse,
    Ndjson,
    Whole,
}

impl Framing {
    pub fn framer(self) -> Option<Framer> {
        let lines = match self {
            Self::Sse => Lines::Sse(SseFramer::new()),
            Self::Ndjson => Lines::Ndjson(NdjsonFramer::new()),
            Self::Whole => return None,
        };
        Some(Framer { lines })
    }

    pub fn split(&self, bytes: &[u8]) -> Vec<WireFrame> {
        let Some(mut framer) = self.framer() else {
            return (!bytes.is_empty())
                .then(|| WireFrame::from_bytes(bytes.to_vec()))
                .into_iter()
                .collect();
        };
        let mut frames: Vec<WireFrame> = framer.push(bytes).collect();
        frames.extend(framer.finish());
        frames
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireFrame {
    Text(String),
    Bytes(Vec<u8>),
}

impl WireFrame {
    pub fn as_str(&self) -> Cow<'_, str> {
        match self {
            Self::Text(text) => Cow::Borrowed(text),
            Self::Bytes(bytes) => String::from_utf8_lossy(bytes),
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Text(text) => text.as_bytes(),
            Self::Bytes(bytes) => bytes,
        }
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        match String::from_utf8(bytes) {
            Ok(text) => Self::Text(text),
            Err(error) => Self::Bytes(error.into_bytes()),
        }
    }
}

#[derive(Debug)]
pub struct Framer {
    lines: Lines,
}

#[derive(Debug)]
enum Lines {
    Sse(SseFramer),
    Ndjson(NdjsonFramer),
}

impl Framer {
    pub fn push(&mut self, chunk: &[u8]) -> impl Iterator<Item = WireFrame> + '_ {
        let (sse, ndjson) = match &mut self.lines {
            Lines::Sse(framer) => (Some(framer.push(chunk)), None),
            Lines::Ndjson(framer) => (None, Some(framer.push(chunk))),
        };
        let events = sse.into_iter().flatten();
        let events = events.filter(|event| !event.data.trim().is_empty());
        let lines = ndjson.into_iter().flatten().map(WireFrame::from_bytes);
        events.map(|event| WireFrame::Text(event.data)).chain(lines)
    }

    pub fn finish(&mut self) -> Option<WireFrame> {
        match &mut self.lines {
            Lines::Sse(_) => None,
            Lines::Ndjson(framer) => framer.finish().map(WireFrame::from_bytes),
        }
    }

    pub fn pending(&self) -> usize {
        match &self.lines {
            Lines::Sse(framer) => framer.pending(),
            Lines::Ndjson(framer) => framer.pending(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Cow<'static, str>,
    pub data: String,
    pub id: String,
    pub retry: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SseField {
    Data,
    Event,
    Id,
    Blank,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseLine {
    pub span: std::ops::Range<usize>,
    pub content: std::ops::Range<usize>,
    pub value: Option<std::ops::Range<usize>>,
    pub field: SseField,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedSseEvent {
    pub event: SseEvent,
    pub end: usize,
}

#[derive(Debug, Default)]
pub struct SseBatch {
    pub events: Vec<RecordedSseEvent>,
    pub lines: Vec<SseLine>,
}

const BOM: [u8; 3] = [0xef, 0xbb, 0xbf];

#[derive(Debug, Default)]
pub struct SseFramer {
    buffer: Vec<u8>,
    scanned: usize,
    after_cr: bool,
    ready: Vec<SseEvent>,
    event_type: String,
    data: String,
    last_event_id: String,
    retry: Option<u64>,
    bom_prefix: usize,
    bom_done: bool,
    since_dispatch: usize,
    offset: usize,
}

impl SseFramer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, chunk: &[u8]) -> std::vec::Drain<'_, SseEvent> {
        self.push_inner(chunk, None);
        self.ready.drain(..)
    }

    pub fn push_recording(&mut self, chunk: &[u8]) -> SseBatch {
        let mut batch = SseBatch::default();
        self.push_inner(chunk, Some(&mut batch));
        batch
    }

    pub fn finish_recording(&mut self) -> SseBatch {
        let mut batch = SseBatch::default();
        if self.bom_prefix > 0 {
            self.buffer
                .extend_from_slice(BOM.get(..self.bom_prefix).unwrap_or_default());
            self.bom_prefix = 0;
            self.bom_done = true;
        }
        if !self.buffer.is_empty() {
            let buffer = std::mem::take(&mut self.buffer);
            let start = self.offset;
            self.offset = self.offset.saturating_add(buffer.len());
            let (field, value) = self.line(&String::from_utf8_lossy(&buffer));
            batch.lines.push(SseLine {
                span: start..self.offset,
                content: start..self.offset,
                value: value.map(|at| start.saturating_add(at)..self.offset),
                field,
            });
        }
        self.dispatch();
        if let Some(event) = self.ready.pop() {
            batch.events.push(RecordedSseEvent {
                event,
                end: self.offset,
            });
        }
        batch
    }

    fn push_inner(&mut self, chunk: &[u8], mut recording: Option<&mut SseBatch>) {
        let mut chunk = self.strip_bom(chunk);
        if self.after_cr
            && let Some((&first, rest)) = chunk.split_first()
        {
            self.after_cr = false;
            if first == b'\n' {
                chunk = rest;
                self.offset = self.offset.saturating_add(1);
                // Pending bytes are counted as if the CRLF had arrived whole.
                if self.since_dispatch > 0 {
                    self.since_dispatch = self.since_dispatch.saturating_add(1);
                }
            }
        }
        self.buffer.extend_from_slice(chunk);
        // Taken out, so its lines are read in place while `line` changes the rest.
        let buffer = std::mem::take(&mut self.buffer);
        let mut rest = buffer.as_slice();
        while let Some((line, next)) = terminated_line(rest, self.scanned) {
            self.scanned = 0;
            let start = self.offset;
            self.offset = self.offset.saturating_add(rest.len() - next.len());
            self.since_dispatch = self.since_dispatch.saturating_add(rest.len() - next.len());
            self.after_cr = next.is_empty() && rest.last() == Some(&b'\r');
            let (field, value) = self.line(&String::from_utf8_lossy(line));
            if let Some(batch) = recording.as_deref_mut() {
                batch.lines.push(SseLine {
                    span: start..self.offset,
                    content: start..start.saturating_add(line.len()),
                    value: value
                        .map(|at| start.saturating_add(at)..start.saturating_add(line.len())),
                    field,
                });
                if let Some(event) = self.ready.pop() {
                    batch.events.push(RecordedSseEvent {
                        event,
                        end: self.offset,
                    });
                }
            }
            rest = next;
        }
        self.scanned = rest.len();
        let consumed = buffer.len() - rest.len();
        self.buffer = buffer;
        self.buffer.drain(..consumed);
    }

    pub fn pending(&self) -> usize {
        self.since_dispatch.saturating_add(self.buffer.len())
    }

    fn strip_bom<'a>(&mut self, chunk: &'a [u8]) -> &'a [u8] {
        if self.bom_done {
            return chunk;
        }
        let mut rest = chunk;
        while let Some(&byte) = rest.first() {
            if BOM.get(self.bom_prefix) != Some(&byte) {
                // Not a BOM after all: replay the bytes that matched so far.
                let matched = std::mem::take(&mut self.bom_prefix);
                self.bom_done = true;
                self.buffer
                    .extend_from_slice(BOM.get(..matched).unwrap_or_default());
                return rest;
            }
            self.bom_prefix += 1;
            rest = rest.get(1..).unwrap_or_default();
            if self.bom_prefix == BOM.len() {
                self.offset = self.offset.saturating_add(BOM.len());
                self.bom_prefix = 0;
                self.bom_done = true;
                return rest;
            }
        }
        rest
    }

    fn line(&mut self, line: &str) -> (SseField, Option<usize>) {
        if line.is_empty() {
            self.dispatch();
            return (SseField::Blank, None);
        }
        // A line starting with a colon is a comment.
        if line.starts_with(':') {
            return (SseField::Other, None);
        }
        let (field, value, at) = match line.split_once(':') {
            Some((field, value)) => {
                let trimmed = value.strip_prefix(' ').unwrap_or(value);
                (
                    field,
                    trimmed,
                    Some(field.len() + 1 + value.len() - trimmed.len()),
                )
            }
            None => (line, "", None),
        };
        let field = match field {
            "event" => {
                self.event_type.clear();
                self.event_type.push_str(value);
                SseField::Event
            }
            "data" => {
                self.data.push_str(value);
                self.data.push('\n');
                SseField::Data
            }
            // A NUL in an id is ignored outright, per the grammar.
            "id" if !value.contains('\0') => {
                self.last_event_id.clear();
                self.last_event_id.push_str(value);
                SseField::Id
            }
            "retry" if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) => {
                // Digits only is a valid field, so a value past `u64` saturates.
                self.retry = Some(value.parse().unwrap_or(u64::MAX));
                SseField::Other
            }
            _ => SseField::Other,
        };
        (field, at)
    }

    fn dispatch(&mut self) {
        self.since_dispatch = 0;
        if self.data.is_empty() {
            self.event_type.clear();
            return;
        }
        self.data.pop();
        let event = if self.event_type.is_empty() {
            Cow::Borrowed("message")
        } else {
            Cow::Owned(std::mem::take(&mut self.event_type))
        };
        self.ready.push(SseEvent {
            event,
            data: std::mem::take(&mut self.data),
            id: self.last_event_id.clone(),
            retry: self.retry.take(),
        });
        self.event_type.clear();
    }
}

#[derive(Debug, Default)]
pub struct NdjsonFramer {
    buffer: Vec<u8>,
    scanned: usize,
    ready: Vec<Vec<u8>>,
}

impl NdjsonFramer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, chunk: &[u8]) -> std::vec::Drain<'_, Vec<u8>> {
        self.buffer.extend_from_slice(chunk);
        let mut rest = self.buffer.as_slice();
        while let Some((line, end)) = split_line(rest, self.scanned, |byte| *byte == b'\n') {
            self.scanned = 0;
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            if !line.is_empty() {
                self.ready.push(line.to_vec());
            }
            rest = end.get(1..).unwrap_or_default();
        }
        self.scanned = rest.len();
        let consumed = self.buffer.len() - rest.len();
        self.buffer.drain(..consumed);
        self.ready.drain(..)
    }

    pub fn finish(&mut self) -> Option<Vec<u8>> {
        let mut line = std::mem::take(&mut self.buffer);
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        (!line.is_empty()).then_some(line)
    }

    pub fn pending(&self) -> usize {
        self.buffer.len()
    }
}

fn terminated_line(buffer: &[u8], scanned: usize) -> Option<(&[u8], &[u8])> {
    let (line, end) = split_line(buffer, scanned, |byte| *byte == b'\n' || *byte == b'\r')?;
    let rest = end.strip_prefix(b"\r\n").or_else(|| end.get(1..));
    Some((line, rest.unwrap_or_default()))
}

fn split_line(buffer: &[u8], scanned: usize, ends: impl Fn(&u8) -> bool) -> Option<(&[u8], &[u8])> {
    buffer.split_at_checked(scanned + buffer.iter().skip(scanned).position(ends)?)
}
