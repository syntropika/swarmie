/// Splits nonempty newline-delimited payloads without validating JSON.
/// A nonempty unterminated last line is available through [`Self::finish`].
#[derive(Debug, Default)]
pub struct NdjsonFramer {
    buffer: Vec<u8>,
    /// How many bytes at the start of `buffer` hold no line end.
    scanned: usize,
    ready: Vec<Vec<u8>>,
}

impl NdjsonFramer {
    /// A framer at the start of a stream.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one transport chunk; yields every line it completed. Blank lines
    /// are not frames.
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

    /// The unterminated last line, if the stream ended with a nonempty one.
    /// A CR ending it is part of the line end, as in [`Self::push`].
    pub fn finish(&mut self) -> Option<Vec<u8>> {
        let mut line = std::mem::take(&mut self.buffer);
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        (!line.is_empty()).then_some(line)
    }
}

fn split_line(buffer: &[u8], scanned: usize, ends: impl Fn(&u8) -> bool) -> Option<(&[u8], &[u8])> {
    buffer.split_at_checked(scanned + buffer.iter().skip(scanned).position(ends)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ndjson_yields_terminated_lines_and_flushes_the_last_one() {
        let mut framer = NdjsonFramer::new();
        let lines: Vec<_> = framer.push(b"{\"a\":1}\n\n{\"b\":2}\n{\"c\"").collect();
        assert_eq!(lines, vec![b"{\"a\":1}".to_vec(), b"{\"b\":2}".to_vec()]);
        assert_eq!(framer.finish(), Some(b"{\"c\"".to_vec()));
        assert_eq!(framer.finish(), None);
    }

    #[test]
    fn ndjson_lines_split_across_chunks_rejoin() {
        let mut framer = NdjsonFramer::new();
        assert_eq!(framer.push(b"{\"a\":").len(), 0);
        let lines: Vec<_> = framer.push(b"1}\r\n").collect();
        assert_eq!(lines, vec![b"{\"a\":1}".to_vec()]);
        let mut lines = Vec::new();
        for byte in b"{\"b\":2}\r\n\n{\"c\":3}\n{\"d\"" {
            lines.extend(framer.push(&[*byte]));
        }
        assert_eq!(lines, vec![b"{\"b\":2}".to_vec(), b"{\"c\":3}".to_vec()]);
        assert_eq!(framer.finish(), Some(b"{\"d\"".to_vec()));
    }

    #[test]
    fn ndjson_finish_drops_a_trailing_cr() {
        let mut framer = NdjsonFramer::new();
        assert_eq!(framer.push(b"{}\n{\"a\":1}\r").count(), 1);
        assert_eq!(framer.finish(), Some(b"{\"a\":1}".to_vec()));
        let mut framer = NdjsonFramer::new();
        assert_eq!(framer.push(b"{}\n\r").count(), 1);
        assert_eq!(framer.finish(), None);
    }
}
