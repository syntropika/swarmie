//! Byte budgets for task values crossing the existing portable Tembra boundary.

use tembra::prelude::*;

use crate::{MAX_PORTABLE_INTEGER, TaskRecord};

/// Maximum canonical wire size of one complete task record.
pub const MAX_TASK_RECORD_BYTES: usize = 768 * 1024;

/// Space reserved inside a record for outcome and blocking diagnostics.
pub const MAX_TASK_CONTROL_BYTES: usize = 8 * 1024;

/// Maximum canonical task page, invocation, transaction or receipt size. The remaining eight
/// KiB below Tembra's one MiB ingestion ceiling accommodates its surrounding call metadata.
pub const MAX_TASK_PAGE_BYTES: usize = 1024 * 1024 - 8 * 1024;

/// Measures the complete canonical UTF-8 value, including escaped strings and object wrappers.
///
/// # Errors
/// Returns `task_storage_invalid` when the value is outside Tembra's portable JSON domain.
pub fn task_wire_bytes<T: Serialize + ?Sized>(value: &T) -> Result<usize> {
    let value = tembra::contracts::to_portable_value(value)
        .map_err(|error| RuntimeError::invalid("task_storage_invalid", error.to_string()))?;
    tembra::contracts::canonical_json(&value)
        .map(|encoded| encoded.len())
        .map_err(|error| RuntimeError::invalid("task_storage_invalid", error.to_string()))
}

fn within(size: usize, maximum: usize, name: &str) -> Result<usize> {
    if size > maximum {
        return Err(RuntimeError::invalid(
            "task_storage_too_large",
            format!("{name} contains {size} canonical wire bytes; maximum is {maximum}"),
        ));
    }
    Ok(size)
}

/// Refuses oversized complete portable payloads before admission or authoritative mutation.
///
/// # Errors
/// Returns `task_storage_too_large` for an oversized payload, or `task_storage_invalid` for
/// nonportable JSON. The borrowed value and external storage remain unchanged.
pub fn validate_task_payload_size<T: Serialize + ?Sized>(value: &T) -> Result<usize> {
    within(task_wire_bytes(value)?, MAX_TASK_PAGE_BYTES, "task payload")
}

/// Checks a full record and reserves space for later outcome or blocking diagnostics.
///
/// Removing only `outcome` and `blocking_reason` for the second measurement preserves every
/// input, checkpoint and ownership field. The second measurement uses the maximum portable
/// revision, maximum version and retry counters, and longest status/cancellation encodings,
/// so counter growth cannot consume the diagnostic reserve. Control data must also fit the
/// complete record ceiling; callers should bound diagnostics to the reserved space.
///
/// # Errors
/// Returns `task_storage_too_large` when the complete record or its non-control fields exceed
/// their fixed budgets, or `task_storage_invalid` for nonportable JSON. No value is modified.
pub fn validate_task_record_size(record: &TaskRecord) -> Result<usize> {
    let size = within(
        task_wire_bytes(record)?,
        MAX_TASK_RECORD_BYTES,
        "task record",
    )?;
    let mut value = tembra::contracts::to_portable_value(record)
        .map_err(|error| RuntimeError::invalid("task_storage_invalid", error.to_string()))?;
    let fields = value
        .as_object_mut()
        .expect("TaskRecord serializes as an object");
    fields.remove("outcome");
    fields.remove("blocking_reason");
    fields.insert("revision".into(), MAX_PORTABLE_INTEGER.into());
    for field in ["definition_version", "attempts", "restarts"] {
        fields.insert(field.into(), u32::MAX.into());
    }
    fields.insert("cancellation_requested".into(), false.into());
    fields.insert("status".into(), "completing".into());
    within(
        task_wire_bytes(&value)?,
        MAX_TASK_RECORD_BYTES - MAX_TASK_CONTROL_BYTES,
        "task record without control fields",
    )?;
    Ok(size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TaskOutcome, TaskSpec, TaskStatus};

    fn record() -> TaskRecord {
        TaskRecord::new(
            TaskSpec {
                id: "boundary".into(),
                kind: "test.size".into(),
                definition_version: 9,
                input: serde_json::Value::Null,
                checkpoint: serde_json::Value::Null,
                phase: "prepare".into(),
            },
            None,
        )
    }

    #[test]
    fn canonical_bytes_include_utf8_escaping_and_wrappers() -> Result<()> {
        let value = serde_json::json!({"text":"é\n\""});
        assert_eq!(task_wire_bytes(&value)?, r#"{"text":"é\n\""}"#.len());
        assert_eq!(task_wire_bytes(&serde_json::json!(1.0))?, 1);
        Ok(())
    }

    #[test]
    fn near_boundary_checkpoint_preserves_counter_and_diagnostic_headroom() -> Result<()> {
        let mut record = record();
        record.revision = 99;
        record.attempts = 9;
        record.restarts = 9;
        // Find the exact accepted data boundary without relying on private field lengths.
        let mut low = 0;
        let mut high = MAX_TASK_RECORD_BYTES;
        while low < high {
            let middle = low + (high - low).div_ceil(2);
            record.checkpoint = serde_json::json!("x".repeat(middle));
            if validate_task_record_size(&record).is_ok() {
                low = middle;
            } else {
                high = middle - 1;
            }
        }
        record.checkpoint = serde_json::json!("x".repeat(low));
        validate_task_record_size(&record)?;
        let mut changed = record.clone();
        changed.revision = 100;
        changed.attempts = 10;
        changed.restarts = 10;
        changed.definition_version = 10;
        changed.cancellation_requested = true;
        changed.status = TaskStatus::Parked;
        validate_task_record_size(&changed)?;
        changed.status = TaskStatus::Terminal;
        changed.outcome = Some(TaskOutcome::Faulted {
            error: "x".repeat(4096),
        });
        validate_task_record_size(&changed)?;
        record.checkpoint = serde_json::json!("x".repeat(low + 1));
        assert_eq!(
            validate_task_record_size(&record).unwrap_err().reason,
            "task_storage_too_large"
        );
        Ok(())
    }
}
