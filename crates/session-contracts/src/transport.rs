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
    if wire_bytes(value)? > maximum {
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
    bounded(request, MAX_PAGE_BYTES)?;
    let mut ids = std::collections::BTreeSet::new();
    for event in &request.events {
        validate_event(event)?;
        if !ids.insert(&event.event_id) {
            return Err(invalid("repeated event ID within batch"));
        }
    }
    Ok(())
}

pub(crate) fn invalid(message: impl Into<String>) -> RuntimeError {
    RuntimeError::invalid("session_store_invalid", message)
}
