#[derive(Debug)]
pub(super) struct SseEvent {
    pub event: Option<String>,
    pub data: String,
    pub invalid: bool,
}

const BOM: [u8; 3] = [0xef, 0xbb, 0xbf];

/// A push parser for the WHATWG `text/event-stream` grammar.
#[derive(Debug, Default)]
pub struct SseFramer {
    /// Bytes not yet consumed as a complete line.
    buffer: Vec<u8>,
    /// How many bytes at the start of `buffer` hold no line end.
    scanned: usize,
    /// The last line ended at a CR that was the last byte pushed, so an LF
    /// opening the next chunk completes that CRLF.
    after_cr: bool,
    /// Events completed by the current `push`.
    ready: Vec<SseEvent>,
    invalid: bool,
    event_type: String,
    data: String,
    /// How many BOM bytes have matched so far, while the prefix is undecided.
    bom_prefix: usize,
    bom_done: bool,
    /// Bytes of complete lines consumed since the last blank line.
    since_dispatch: usize,
}

impl SseFramer {
    /// A framer at the start of a stream.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one transport chunk; yields every event it completed.
    pub fn push(&mut self, chunk: &[u8]) -> std::vec::Drain<'_, SseEvent> {
        self.push_inner(chunk);
        self.ready.drain(..)
    }

    fn push_inner(&mut self, chunk: &[u8]) {
        let mut chunk = self.strip_bom(chunk);
        if self.after_cr
            && let Some((&first, rest)) = chunk.split_first()
        {
            self.after_cr = false;
            if first == b'\n' {
                chunk = rest;

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

            self.since_dispatch = self.since_dispatch.saturating_add(rest.len() - next.len());
            self.after_cr = next.is_empty() && rest.last() == Some(&b'\r');
            match std::str::from_utf8(line) {
                Ok(line) => self.line(line),
                Err(_) => self.invalid = true,
            }
            rest = next;
        }
        self.scanned = rest.len();
        let consumed = buffer.len() - rest.len();
        self.buffer = buffer;
        self.buffer.drain(..consumed);
    }

    /// Saturating count of buffered bytes and complete lines since the last
    /// blank line, including any undecided BOM prefix. Exposes no frame data.
    pub fn pending(&self) -> usize {
        self.since_dispatch
            .saturating_add(self.buffer.len())
            .saturating_add(self.bom_prefix)
    }

    /// Consume the stream's optional leading BOM, which may itself be split
    /// across chunks. Returns the chunk with any BOM bytes removed.
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
                self.bom_prefix = 0;
                self.bom_done = true;
                return rest;
            }
        }
        rest
    }

    /// Read only data and event names. Other fields cannot cause reconnection.
    fn line(&mut self, line: &str) {
        if line.is_empty() {
            self.dispatch();
            return;
        }
        if line.starts_with(':') {
            return;
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "event" => {
                self.event_type.clear();
                self.event_type.push_str(value);
            }
            "data" => {
                self.data.push_str(value);
                self.data.push('\n');
            }
            _ => {}
        }
    }

    /// A blank line ends an event. An event with no data is not dispatched:
    /// it only resets the event type, as the grammar says.
    fn dispatch(&mut self) {
        self.since_dispatch = 0;
        if self.data.is_empty() && !self.invalid {
            self.event_type.clear();
            return;
        }
        self.data.pop();
        let event = (!self.event_type.is_empty()).then(|| std::mem::take(&mut self.event_type));
        self.ready.push(SseEvent {
            event,
            data: std::mem::take(&mut self.data),
            invalid: std::mem::take(&mut self.invalid),
        });
        self.event_type.clear();
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

#[cfg(test)]
mod tests {
    use super::*;
    fn sse_all(body: &[u8]) -> Vec<(String, String, String)> {
        let mut framer = SseFramer::new();
        framer
            .push(body)
            .map(|event| {
                (
                    event.event.unwrap_or_else(|| "message".into()),
                    event.data,
                    String::new(),
                )
            })
            .collect()
    }
    #[test]
    fn every_line_ending_terminates_a_line() {
        let message = |data: &str| ("message".to_string(), data.to_string(), String::new());
        assert_eq!(sse_all(b"data: one\n\n"), [message("one")]);
        assert_eq!(sse_all(b"data: one\r\n\r\n"), [message("one")]);
        // A CR ends its line at once, so a body ending on one dispatches.
        assert_eq!(
            sse_all(b"data: one\r\rdata: two\r\r"),
            [message("one"), message("two")]
        );
    }

    #[test]
    fn a_leading_bom_is_not_data() {
        assert_eq!(
            sse_all("\u{feff}data: one\n\n".as_bytes()),
            vec![("message".into(), "one".into(), String::new())]
        );
    }

    #[test]
    fn a_bom_split_across_chunks_is_still_ignored() {
        let mut framer = SseFramer::new();
        assert_eq!(framer.push(&[0xef]).len(), 0);
        assert_eq!(framer.push(&[0xbb]).len(), 0);
        let events: Vec<_> = framer.push(&[0xbf]).collect();
        assert!(events.is_empty());
        let events: Vec<_> = framer.push(b"data: one\n\n").collect();
        assert_eq!(events.first().map(|event| event.data.as_str()), Some("one"));
    }

    #[test]
    fn a_leading_colon_is_a_comment() {
        assert_eq!(
            sse_all(b": keep-alive\ndata: one\n\n"),
            vec![("message".into(), "one".into(), String::new())]
        );
    }

    #[test]
    fn a_blank_line_without_data_dispatches_nothing_and_resets_the_type() {
        assert_eq!(
            sse_all(b"event: ping\n\ndata: one\n\n"),
            vec![("message".into(), "one".into(), String::new())]
        );
    }

    #[test]
    fn a_truncated_trailing_event_is_pending_bytes_never_a_frame() {
        let mut framer = SseFramer::new();
        let events: Vec<_> = framer
            .push(b"data: one\n\nevent: message_stop\ndata: {}")
            .collect();
        assert_eq!(events.len(), 1);
        assert_eq!(framer.pending(), b"event: message_stop\ndata: {}".len());
    }

    #[test]
    fn a_body_whose_last_event_lacks_its_blank_line_delivers_one_event_fewer() {
        let body = concat!(
            "event: message_start\ndata: {\"type\":\"message_start\"}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}"
        );
        assert_eq!(sse_all(body.as_bytes()).len(), 1);
        let terminated = format!("{body}\n\n");
        assert_eq!(sse_all(terminated.as_bytes()).len(), 2);
    }

    #[test]
    fn splitting_a_body_at_every_offset_yields_the_same_events() {
        let body = concat!(
            "\u{feff}: hello\r\n",
            "event: a\r\ndata: {\"n\":1}\r\n\r\n",
            "id: x\ndata: line one\ndata: line two\n\n",
            "\rdata: after a bare cr\r\r",
            "retry: 12\nevent: b\r\ndata: last\r\n\r\n",
            "data: open\r\n",
        )
        .as_bytes();
        let mut whole = SseFramer::new();
        let events = |framer: &mut SseFramer, chunk: &[u8]| -> Vec<_> {
            framer
                .push(chunk)
                .map(|event| {
                    (
                        event.event.unwrap_or_else(|| "message".into()),
                        event.data,
                        String::new(),
                    )
                })
                .collect()
        };
        let expected = events(&mut whole, body);
        assert_eq!(expected.len(), 4);
        assert_eq!(whole.pending(), b"data: open\r\n".len());
        for split in 0..=body.len() {
            let (head, tail) = body.split_at(split);
            let mut framer = SseFramer::new();
            let mut split_events = events(&mut framer, head);
            split_events.extend(events(&mut framer, b""));
            split_events.extend(events(&mut framer, tail));
            assert_eq!(split_events, expected, "split at {split}");
            assert_eq!(framer.pending(), whole.pending(), "split at {split}");
        }
        let mut framer = SseFramer::new();
        let mut bytewise = Vec::new();
        for byte in body {
            bytewise.extend(events(&mut framer, &[*byte]));
        }
        assert_eq!(bytewise, expected, "one byte at a time");
        assert_eq!(framer.pending(), whole.pending(), "one byte at a time");
    }
}
