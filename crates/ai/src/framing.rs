use crate::protocol::wire;
use crate::{Error, Protocol};
use serde_json::Value;

mod ndjson;
mod sse;

use ndjson::NdjsonFramer;
use sse::SseFramer;

pub(crate) enum Frame {
    Json { name: Option<String>, value: Value },
    Done,
    Invalid,
}

/// Transport framing only. Protocol state machines decide terminal semantics.
pub(crate) struct Framer {
    lines: Lines,
}

enum Lines {
    Sse(SseFramer),
    Ndjson(NdjsonFramer),
}

impl Framer {
    pub fn new(protocol: Protocol) -> Self {
        Self {
            lines: if protocol == Protocol::Ollama {
                Lines::Ndjson(NdjsonFramer::new())
            } else {
                Lines::Sse(SseFramer::new())
            },
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Frame>, Error> {
        Ok(match &mut self.lines {
            Lines::Sse(framer) => framer
                .push(bytes)
                .filter_map(|event| {
                    if event.invalid {
                        Some(Frame::Invalid)
                    } else if event.data.trim().is_empty() {
                        None
                    } else if event.data == "[DONE]" {
                        Some(Frame::Done)
                    } else {
                        Some(json(event.event, event.data.as_bytes()).unwrap_or(Frame::Invalid))
                    }
                })
                .collect(),
            Lines::Ndjson(framer) => framer
                .push(bytes)
                .map(|line| json(None, &line).unwrap_or(Frame::Invalid))
                .collect(),
        })
    }

    pub fn finish(&mut self) -> Result<Vec<Frame>, Error> {
        match &mut self.lines {
            Lines::Ndjson(framer) => framer
                .finish()
                .map(|line| json(None, &line))
                .transpose()
                .map(|frame| frame.into_iter().collect()),
            Lines::Sse(framer) if framer.pending() == 0 => Ok(vec![]),
            Lines::Sse(_) => Err(wire()),
        }
    }
}

fn json(name: Option<String>, bytes: &[u8]) -> Result<Frame, Error> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| wire())?;
    if !value.is_object() {
        return Err(wire());
    }
    Ok(Frame::Json { name, value })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn arbitrary_utf8_crlf_and_multiline_splits_keep_values() {
        let wire = ": heartbeat\r\n\r\nevent: future\r\ndata: {\"type\":\"future\",\r\ndata: \"opaque\":\"äñ\"}\r\n\r\ndata: [DONE]\r\n\r\n";
        for split in 0..=wire.len() {
            let mut framer = Framer::new(Protocol::Responses);
            let mut frames = framer.push(&wire.as_bytes()[..split]).unwrap();
            frames.extend(framer.push(&wire.as_bytes()[split..]).unwrap());
            assert_eq!(frames.len(), 2);
            let Frame::Json { name, value } = &frames[0] else {
                panic!("JSON event required")
            };
            assert_eq!(name.as_deref(), Some("future"));
            assert_eq!(value, &json!({"type":"future", "opaque":"äñ"}));
            assert!(matches!(frames[1], Frame::Done));
            assert!(framer.finish().unwrap().is_empty());
        }
    }

    #[test]
    fn incomplete_sse_json_and_nonobject_frames_refuse() {
        for body in [
            "data: {\"type\":\"x\"}",
            "data: {\"type\":\"x\"}\n",
            "data: [1]\n\n",
            "data: invalid\n\n",
        ] {
            let mut framer = Framer::new(Protocol::Responses);
            let frames = framer.push(body.as_bytes()).unwrap();
            assert!(
                frames.iter().any(|frame| matches!(frame, Frame::Invalid))
                    || framer.finish().is_err()
            );
        }
    }

    #[test]
    fn ndjson_accepts_final_line_without_lf() {
        let mut framer = Framer::new(Protocol::Ollama);
        assert_eq!(
            framer
                .push(b"{\"done\":false}\n{\"done\":true}")
                .unwrap()
                .len(),
            1
        );
        assert_eq!(framer.finish().unwrap().len(), 1);
    }

    #[test]
    fn utf8_errors_are_deferred_frames_and_never_replacement_characters() {
        let body = b"data: {\"type\":\"known\"}\n\ndata: {\"text\":\"\xff\"}\n\n";
        for split in 0..=body.len() {
            let mut framer = Framer::new(Protocol::Responses);
            let mut frames = framer.push(&body[..split]).unwrap();
            frames.extend(framer.push(&body[split..]).unwrap());
            assert_eq!(frames.len(), 2);
            assert!(matches!(frames[0], Frame::Json { .. }));
            assert!(matches!(frames[1], Frame::Invalid));
        }
    }

    #[test]
    fn whitespace_heartbeats_are_ignored_but_named_message_events_are_explicit() {
        let mut framer = Framer::new(Protocol::Responses);
        let frames = framer
            .push(b"data:  \r\n\r\nevent: message\rdata: {}\r\r")
            .unwrap();
        assert_eq!(frames.len(), 1);
        let Frame::Json { name, .. } = &frames[0] else {
            panic!("JSON frame required")
        };
        assert_eq!(name.as_deref(), Some("message"));
    }
}
