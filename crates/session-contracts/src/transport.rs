use tembra::prelude::*;

use crate::{
    AppendEvents, COMPACTION, Compaction, Correlation, INSTRUCTIONS, Instructions, MESSAGE,
    Message, NewEvent,
};

/// Tembra's portable ingestion ceiling is one MiB. Reserve headroom for call and projection
/// wrappers; row limits independently bound the number of decoded structures in memory.
pub const MAX_PAGE_BYTES: usize = 768 * 1024;

/// Maximum complete stored event, leaving room for cursor/page wrappers.
pub const MAX_EVENT_BYTES: usize = 256 * 1024;

/// Largest exact portable JavaScript integer. SQL counters never exceed this boundary.
pub const MAX_SEQUENCE: u64 = 9_007_199_254_740_991;

/// Canonical portable encoding used both for admission and equality across Rust/TypeScript.
///
/// # Errors
/// Rejects values outside the pinned Tembra portable JSON domain, including unsafe numbers.
pub fn canonical<T: Serialize + ?Sized>(value: &T) -> Result<String> {
    let portable =
        tembra::contracts::to_portable_value(value).map_err(|e| invalid(e.to_string()))?;
    tembra::contracts::canonical_json(&portable).map_err(|e| invalid(e.to_string()))
}

/// Measures encoded canonical UTF-8, including escaping and all object wrappers.
///
/// # Errors
/// Returns `session_store_invalid` for nonportable values.
pub fn wire_bytes<T: Serialize + ?Sized>(value: &T) -> Result<usize> {
    canonical(value).map(|encoded| encoded.len())
}

/// Checks complete payload size before authoritative mutation.
///
/// # Errors
/// Returns `session_store_too_large` without mutation when the budget is exceeded.
pub fn bounded<T: Serialize + ?Sized>(value: &T, maximum: usize) -> Result<()> {
    bounded_size(wire_bytes(value)?, maximum)
}

/// Checks an already measured canonical size, such as the length of a stored canonical text.
///
/// # Errors
/// Returns `session_store_too_large`, exactly as [`bounded`] does, when `size` exceeds
/// `maximum`.
pub fn bounded_size(size: usize, maximum: usize) -> Result<()> {
    if size > maximum {
        return Err(RuntimeError::invalid(
            "session_store_too_large",
            "canonical byte budget exceeded; use a durable blob reference",
        ));
    }
    Ok(())
}

/// Validates opaque IDs and kind names without normalization.
///
/// # Errors
/// Refuses empty, oversized, control-containing or whitespace-surrounded identities.
pub fn identity(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 256
        || value.chars().any(char::is_control)
        || value.starts_with(char::is_whitespace)
        || value.ends_with(char::is_whitespace)
    {
        return Err(invalid(
            "identity requires 1..=256 bytes without controls or surrounding whitespace",
        ));
    }
    Ok(())
}

/// Checks immutable event envelope and known version-one payloads. Future payloads are opaque.
///
/// # Errors
/// Returns explicit validation/size errors without modifying the event or external state.
pub fn validate_event(event: &NewEvent) -> Result<()> {
    identity(&event.event_id)?;
    identity(&event.kind)?;
    let Correlation {
        conversation_id,
        turn_id,
        task_id,
        step_id,
        attempt_id,
        parent_event_id,
    } = &event.correlation;
    for id in [
        conversation_id,
        turn_id,
        task_id,
        step_id,
        attempt_id,
        parent_event_id,
    ]
    .into_iter()
    .flatten()
    {
        identity(id)?;
    }
    if event.version == 0 || event.occurred_at_ms.is_some_and(|v| v > MAX_SEQUENCE) {
        return Err(invalid(
            "event version and timestamp are outside their domain",
        ));
    }
    if event.version == 1 {
        match event.kind.as_str() {
            MESSAGE => {
                let data: Message = serde_json::from_value(event.payload.clone())
                    .map_err(|e| invalid(e.to_string()))?;
                identity(&data.role)?;
                for id in [data.reply_to, data.tool_call_id].into_iter().flatten() {
                    identity(&id)?;
                }
            }
            INSTRUCTIONS => {
                let data: Instructions = serde_json::from_value(event.payload.clone())
                    .map_err(|e| invalid(e.to_string()))?;
                identity(&data.scope)?;
            }
            COMPACTION => {
                let data: Compaction = serde_json::from_value(event.payload.clone())
                    .map_err(|e| invalid(e.to_string()))?;
                identity(&data.policy)?;
                if data.through_sequence > MAX_SEQUENCE {
                    return Err(invalid("compaction sequence outside portable domain"));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Validates an entire append request before writing any rows.
///
/// # Errors
/// Rejects invalid identities, counters, empty/oversized batches and repeated event IDs.
pub fn validate_append(request: &AppendEvents) -> Result<()> {
    encode_append(request).map(|_| ())
}

/// An append request encoded once: the canonical request retained for exact retry equality and
/// each event's canonical text, reused for its stored envelope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedAppend {
    request: String,
    /// Each text is `[[[event]]]`, matching the event's nesting inside returned pages.
    events: Vec<String>,
    session: String,
}

impl EncodedAppend {
    /// Canonical text of the complete request, identical to [`canonical`] of that request.
    pub fn request(&self) -> &str {
        &self.request
    }

    /// Canonical stored envelope of the request's event `index` at `sequence`. It equals
    /// [`canonical`] of the corresponding [`crate::SessionEvent`] without encoding the event
    /// payload again. `sequence` must lie within the portable range, as receipts require.
    ///
    /// # Panics
    /// Panics when `index` does not name an event of the encoded request.
    pub fn event(&self, index: usize, sequence: u64) -> String {
        format!(
            "{{\"event\":{},\"sequence\":{sequence},\"session_id\":{}}}",
            unwrap_event(&self.events[index]),
            self.session
        )
    }
}

/// Removes the three array wrappers added by [`encode_append`].
fn unwrap_event(wrapped: &str) -> &str {
    &wrapped[3..wrapped.len() - 3]
}

/// Validates an entire append request and encodes it once before writing any rows.
///
/// Every event is encoded exactly once, nested in three arrays so Tembra's portable depth limit
/// includes the page, event list and stored envelope used by both [`crate::EventPage`] and
/// [`crate::ProjectionPage`]. Their event is one container deeper than in the append request;
/// admission reserves that depth so accepted events remain readable through the Contract.
/// The canonical request is assembled
/// from those texts in canonical member order (`batch_id`, `events`, `expected_head`,
/// `session_id`); canonical JSON encodes every member independently of its position, so the
/// request is never serialized a second time. Checks run in the order documented by
/// [`validate_append`] and report the same failures.
///
/// # Errors
/// Rejects invalid identities, counters, empty/oversized batches, nonportable values and
/// repeated event IDs without modifying the request or external state.
pub fn encode_append(request: &AppendEvents) -> Result<EncodedAppend> {
    identity(&request.session_id)?;
    identity(&request.batch_id)?;
    if request.events.is_empty()
        || request.events.len() > 128
        || request.expected_head.is_some_and(|n| n > MAX_SEQUENCE)
    {
        return Err(invalid(
            "append requires 1..=128 events and a portable head",
        ));
    }
    let events = request
        .events
        .iter()
        .map(|event| canonical(&[[[event]]]))
        .collect::<Result<Vec<_>>>()?;
    let batch = canonical(&request.batch_id)?;
    let session = canonical(&request.session_id)?;
    let head = request
        .expected_head
        .map_or_else(|| "null".to_owned(), |head| head.to_string());
    let mut encoded = String::with_capacity(
        events.iter().map(String::len).sum::<usize>() + batch.len() + session.len() + 64,
    );
    encoded.push_str("{\"batch_id\":");
    encoded.push_str(&batch);
    encoded.push_str(",\"events\":[");
    for (index, event) in events.iter().enumerate() {
        if index > 0 {
            encoded.push(',');
        }
        encoded.push_str(unwrap_event(event));
    }
    encoded.push_str("],\"expected_head\":");
    encoded.push_str(&head);
    encoded.push_str(",\"session_id\":");
    encoded.push_str(&session);
    encoded.push('}');
    debug_assert!(
        canonical(request).is_ok_and(|complete| complete == encoded),
        "an assembled append request must equal its complete canonical encoding"
    );
    bounded_size(encoded.len(), MAX_PAGE_BYTES)?;
    let mut ids = std::collections::BTreeSet::new();
    for event in &request.events {
        validate_event(event)?;
        if !ids.insert(&event.event_id) {
            return Err(invalid("repeated event ID within batch"));
        }
    }
    Ok(EncodedAppend {
        request: encoded,
        events,
        session,
    })
}

pub(crate) fn invalid(message: impl Into<String>) -> RuntimeError {
    RuntimeError::invalid("session_store_invalid", message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Correlation, MESSAGE, SessionEvent};

    fn event(id: &str, kind: &str, payload: serde_json::Value) -> NewEvent {
        NewEvent {
            event_id: id.into(),
            kind: kind.into(),
            version: 1,
            correlation: Correlation::default(),
            occurred_at_ms: None,
            payload,
        }
    }

    fn nested(depth: usize) -> serde_json::Value {
        let mut value = serde_json::Value::Null;
        for _ in 0..depth {
            value = serde_json::json!([value]);
        }
        value
    }

    #[test]
    fn assembled_requests_and_envelopes_equal_their_complete_encodings() -> Result<()> {
        let mut correlated = event(
            "event \"é\" \u{2028} \\ end",
            "plugin.measured",
            serde_json::json!({
                "integral": 1.0,
                "fraction": 2.5,
                "negative": -3,
                "text": "\u{1F600}\n\t\"\\\u{7}",
                "nested": [{"b": [null, true, false], "a": {"€": 0.000001}}],
            }),
        );
        correlated.version = 7;
        correlated.occurred_at_ms = Some(MAX_SEQUENCE);
        correlated.correlation = Correlation {
            conversation_id: Some("conversation:é".into()),
            turn_id: Some("turn".into()),
            task_id: Some("task".into()),
            step_id: Some("step".into()),
            attempt_id: Some("attempt".into()),
            parent_event_id: Some("parent".into()),
        };
        let message = event(
            "message",
            MESSAGE,
            serde_json::json!({"role":"user","content":[{"type":"text","text":"Hello"}],"reply_to":null,"tool_call_id":null}),
        );
        for expected_head in [None, Some(0), Some(MAX_SEQUENCE)] {
            for events in [
                vec![message.clone()],
                vec![correlated.clone(), message.clone()],
            ] {
                let request = AppendEvents {
                    session_id: "session \"é\" \\".into(),
                    batch_id: "batch:\u{1F600}".into(),
                    expected_head,
                    events,
                };
                let encoded = encode_append(&request)?;
                assert_eq!(encoded.request(), canonical(&request)?);
                for (index, event) in request.events.iter().enumerate() {
                    for sequence in [1, 4096, MAX_SEQUENCE] {
                        assert_eq!(
                            encoded.event(index, sequence),
                            canonical(&SessionEvent {
                                session_id: request.session_id.clone(),
                                sequence,
                                event: event.clone(),
                            })?
                        );
                    }
                }
            }
        }
        Ok(())
    }

    #[test]
    fn assembled_member_names_cover_every_serialized_field() {
        // The assembly spells these members in canonical order; a new field must update it.
        let request = serde_json::to_value(AppendEvents {
            session_id: "chat".into(),
            batch_id: "batch".into(),
            expected_head: None,
            events: vec![],
        })
        .expect("request object");
        let stored = serde_json::to_value(SessionEvent {
            session_id: "chat".into(),
            sequence: 1,
            event: event("event", MESSAGE, serde_json::Value::Null),
        })
        .expect("event object");
        let names = |value: &serde_json::Value| {
            value
                .as_object()
                .expect("object")
                .keys()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>()
        };
        assert_eq!(
            names(&request),
            ["batch_id", "events", "expected_head", "session_id"]
                .map(String::from)
                .into()
        );
        assert_eq!(
            names(&stored),
            ["event", "sequence", "session_id"].map(String::from).into()
        );
    }

    #[test]
    fn encoded_appends_keep_complete_request_size_and_depth_refusals() -> Result<()> {
        let request = |payload| AppendEvents {
            session_id: "chat".into(),
            batch_id: "batch".into(),
            expected_head: None,
            events: vec![event("deep", "plugin.deep", payload)],
        };
        // Pages add one container beyond the request's three-container payload envelope.
        let accepted = request(nested(124));
        assert!(canonical(&accepted).is_ok());
        assert!(encode_append(&accepted).is_ok());
        let refused = request(nested(125));
        assert!(canonical(&refused).is_ok());
        assert_eq!(
            encode_append(&refused).unwrap_err().reason,
            "session_store_invalid"
        );
        assert!(canonical(&request(nested(126))).is_err());
        let unsafe_number = request(serde_json::json!({"value": MAX_SEQUENCE + 1}));
        assert_eq!(
            encode_append(&unsafe_number).unwrap_err().reason,
            "session_store_invalid"
        );
        // Find the largest admitted payload with the complete reference measurement.
        let base = wire_bytes(&request(serde_json::json!("")))?;
        let fits = request(serde_json::json!("x".repeat(MAX_PAGE_BYTES - base)));
        assert_eq!(wire_bytes(&fits)?, MAX_PAGE_BYTES);
        assert_eq!(encode_append(&fits)?.request().len(), MAX_PAGE_BYTES);
        let over = request(serde_json::json!("x".repeat(MAX_PAGE_BYTES - base + 1)));
        assert_eq!(
            encode_append(&over).unwrap_err().reason,
            "session_store_too_large"
        );
        assert_eq!(
            validate_append(&over).unwrap_err().reason,
            "session_store_too_large"
        );
        let mut repeated = request(serde_json::json!(null));
        repeated.events.push(repeated.events[0].clone());
        assert_eq!(
            encode_append(&repeated).unwrap_err().message,
            "repeated event ID within batch"
        );
        Ok(())
    }
}
