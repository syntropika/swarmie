//! Bounded presentation only. Canonical conversation entries are never rewritten.
use super::types::{Message, Snapshot};

pub(super) const BUDGET: usize = 44 * 1024;

pub(super) fn clip(message: &mut Message, limit: usize) {
    if message.text.len() > limit {
        let mut end = limit;
        while !message.text.is_char_boundary(end) {
            end -= 1;
        }
        message.text.truncate(end);
        message.truncated = true;
    }
}

pub(super) fn bound(mut snapshot: Snapshot) -> Snapshot {
    for message in &mut snapshot.messages {
        clip(message, 8 * 1024);
    }
    if snapshot.admitted.len() > 32 {
        snapshot.admitted.drain(..snapshot.admitted.len() - 32);
    }
    loop {
        let bytes =
            serde_json::to_vec(&snapshot).expect("presentation values are JSON serializable");
        if bytes.len() <= BUDGET {
            break;
        }
        if snapshot.messages.len() > 1 {
            snapshot.messages.remove(0);
            snapshot.omitted_messages += 1;
        } else if let Some(message) = snapshot
            .messages
            .first_mut()
            .filter(|message| !message.text.is_empty())
        {
            clip(message, message.text.len() / 2);
        } else if snapshot.admitted.len() > 1 {
            snapshot.admitted.remove(0);
        } else {
            break;
        }
    }
    snapshot
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversations::types::Phase;

    #[test]
    fn escaped_large_history_keeps_recent_messages_with_explicit_omissions() {
        let snapshot = Snapshot {
            conversation_id: "projection-fixture".into(),
            phase: Phase::Completed,
            label: String::new(),
            admitted: vec!["receipt".into()],
            omitted_messages: 0,
            can_acknowledge: false,
            messages: (0..100)
                .map(|id| Message {
                    id: id.to_string(),
                    role: "assistant".into(),
                    text: "\"\n暖".repeat(20_000),
                    truncated: false,
                })
                .collect(),
        };
        let snapshot = bound(snapshot);
        assert!(serde_json::to_vec(&snapshot).unwrap().len() <= BUDGET);
        assert_eq!(snapshot.messages.last().unwrap().id, "99");
        assert!(snapshot.messages.last().unwrap().truncated);
        assert_eq!(snapshot.messages.len() + snapshot.omitted_messages, 100);
        assert_eq!(snapshot.admitted, ["receipt"]);
        assert!(snapshot.phase == Phase::Completed);
    }
}
