//! Byte budgets for task values crossing the existing portable Tembra boundary.

use tembra::prelude::*;

use crate::{CommitResult, MAX_PORTABLE_INTEGER, TaskRecord, TaskStatus, TaskTransaction};

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
    let value = tembra::contracts::to_portable_value(value).map_err(nonportable)?;
    tembra::contracts::canonical_json(&value)
        .map(|encoded| encoded.len())
        .map_err(nonportable)
}

fn nonportable(error: tembra::contracts::ContractError) -> RuntimeError {
    RuntimeError::invalid("task_storage_invalid", error.to_string())
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
    validate_task_payload_bytes(task_wire_bytes(value)?)
}

/// Refuses an already measured complete canonical payload before admission or mutation.
///
/// Use it when the exact canonical size was derived from canonical parts instead of encoding
/// the complete payload again, for example with [`encode_task_transaction`].
///
/// # Errors
/// Returns `task_storage_too_large` when `size` exceeds [`MAX_TASK_PAGE_BYTES`].
pub fn validate_task_payload_bytes(size: usize) -> Result<usize> {
    within(size, MAX_TASK_PAGE_BYTES, "task payload")
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
    validate_task_record_encoding(record, task_wire_bytes(record)?)
}

/// Applies [`validate_task_record_size`] to a record whose canonical length is already known.
///
/// `size` must be the byte length of the record's canonical portable encoding, such as a text
/// returned by [`encode_task_transaction`]. The second measurement is derived exactly from that
/// length: canonical JSON encodes every object member independently of its position, so
/// removing a member subtracts its quoted name, colon, value and one separating comma, and
/// replacing a value changes the length only by the difference between both value encodings.
/// Only the small control and counter values are encoded; the record is never encoded again.
///
/// # Errors
/// Returns `task_storage_too_large` exactly when [`validate_task_record_size`] does for the
/// same record, and `task_storage_invalid` for nonportable control values or a `size` that
/// cannot be the canonical length of `record`. No value is modified.
pub fn validate_task_record_encoding(record: &TaskRecord, size: usize) -> Result<usize> {
    within(size, MAX_TASK_RECORD_BYTES, "task record")?;
    let removed = member_bytes("outcome", task_wire_bytes(&record.outcome)?)
        + member_bytes("blocking_reason", task_wire_bytes(&record.blocking_reason)?);
    let current = integer_bytes(record.revision)
        + [record.definition_version, record.attempts, record.restarts]
            .into_iter()
            .map(|value| integer_bytes(value.into()))
            .sum::<usize>()
        + boolean_bytes(record.cancellation_requested)
        + task_wire_bytes(&record.status)?;
    let widest = integer_bytes(MAX_PORTABLE_INTEGER)
        + 3 * integer_bytes(u32::MAX.into())
        + boolean_bytes(false)
        + task_wire_bytes(&TaskStatus::Completing)?;
    // The removed and replaced encodings are parts of the record's canonical text.
    let reserved = size.checked_sub(removed + current).ok_or_else(|| {
        RuntimeError::invalid(
            "task_storage_invalid",
            "the canonical record length is inconsistent with its fields",
        )
    })? + widest;
    within(
        reserved,
        MAX_TASK_RECORD_BYTES - MAX_TASK_CONTROL_BYTES,
        "task record without control fields",
    )?;
    Ok(size)
}

/// Canonical bytes of one object member with a plain ASCII name: the quoted name, its colon,
/// the value and the single comma that separates it from a remaining member.
fn member_bytes(name: &str, value: usize) -> usize {
    name.len() + 2 + 1 + value + 1
}

/// Portable integers encode as plain decimal digits without sign, exponent or fraction.
fn integer_bytes(value: u64) -> usize {
    value
        .checked_ilog10()
        .map_or(1, |digits| digits as usize + 1)
}

fn boolean_bytes(value: bool) -> usize {
    if value { "true".len() } else { "false".len() }
}

/// One task transaction encoded once for storage and exact size accounting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedTaskTransaction {
    /// Canonical portable text of each written record, in write order. Each text equals the
    /// encoding measured by [`task_wire_bytes`] for that record.
    pub records: Vec<String>,
    /// Exact canonical wire size of the complete transaction.
    pub bytes: usize,
}

/// Encodes each written record exactly once and derives the exact canonical size of the
/// complete transaction from those texts, without serializing any record a second time.
///
/// Each write is converted nested in two arrays, the depth of a write inside the transaction,
/// so Tembra's portable profile, including its nesting limit, refuses exactly the values that
/// it refuses in the complete request. Every write envelope is measured with a `null` record
/// placeholder whose four bytes are then replaced by the record's text length; canonical
/// members encode independently of their position. Budgets are not checked here: refuse
/// oversized requests with [`validate_task_payload_bytes`] and oversized records with
/// [`validate_task_record_encoding`].
///
/// # Errors
/// Returns `task_storage_invalid` when a value is outside Tembra's portable JSON domain,
/// including unsafe integers. The borrowed transaction is not modified.
pub fn encode_task_transaction(transaction: &TaskTransaction) -> Result<EncodedTaskTransaction> {
    let mut bytes = task_wire_bytes(&TaskTransaction {
        owner: transaction.owner.clone(),
        writes: vec![],
    })?;
    let mut records = Vec::with_capacity(transaction.writes.len());
    let missing =
        || RuntimeError::invalid("task_storage_invalid", "a task write requires a record");
    for (index, write) in transaction.writes.iter().enumerate() {
        let mut nested = tembra::contracts::to_portable_value(&[[write]]).map_err(nonportable)?;
        let mut envelope = nested
            .get_mut(0)
            .and_then(|writes| writes.get_mut(0))
            .map(serde_json::Value::take)
            .ok_or_else(missing)?;
        let record = envelope
            .get_mut("record")
            .map(serde_json::Value::take)
            .ok_or_else(missing)?;
        let text = tembra::contracts::canonical_json(&record).map_err(nonportable)?;
        let envelope = tembra::contracts::canonical_json(&envelope)
            .map_err(nonportable)?
            .len();
        bytes += usize::from(index > 0) + envelope - "null".len() + text.len();
        records.push(text);
    }
    Ok(EncodedTaskTransaction { records, bytes })
}

/// Exact canonical size of a committed receipt whose records have the given canonical sizes,
/// in receipt order. Pair it with [`validate_task_payload_bytes`] before mutation.
///
/// # Errors
/// Returns `task_storage_invalid` when `sequence` is outside the portable integer range.
pub fn committed_receipt_bytes(
    sequence: u64,
    records: impl IntoIterator<Item = usize>,
) -> Result<usize> {
    let mut bytes = task_wire_bytes(&CommitResult::Committed {
        sequence,
        records: vec![],
    })?;
    for (index, size) in records.into_iter().enumerate() {
        bytes += usize::from(index > 0) + size;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BlockingReason, TaskLifecycleOrigin, TaskOutcome, TaskSpec, TaskTransitionProvenance,
        TaskWrite,
    };

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

    /// Re-encodes the edited record, exactly as the shared validator measured it before the
    /// reserve was derived from the canonical length.
    fn reference_record_size(record: &TaskRecord) -> Result<usize> {
        let size = within(
            task_wire_bytes(record)?,
            MAX_TASK_RECORD_BYTES,
            "task record",
        )?;
        let mut value = tembra::contracts::to_portable_value(record).map_err(nonportable)?;
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

    fn assert_reference_result(record: &TaskRecord) {
        match (
            reference_record_size(record),
            validate_task_record_size(record),
        ) {
            (Ok(expected), Ok(actual)) => assert_eq!(expected, actual),
            (Err(expected), Err(actual)) => {
                assert_eq!(expected.reason, actual.reason);
                assert_eq!(expected.message, actual.message);
            }
            (expected, actual) => panic!(
                "derived record budget differs from the reference: {:?} versus {:?}",
                expected.map_err(|error| error.reason),
                actual.map_err(|error| error.reason)
            ),
        }
    }

    #[test]
    fn derived_control_reserve_matches_the_complete_reencoding() {
        let statuses = [
            TaskStatus::Pending,
            TaskStatus::Running,
            TaskStatus::Waiting,
            TaskStatus::Sleeping,
            TaskStatus::Completing,
            TaskStatus::Blocked,
            TaskStatus::Parked,
            TaskStatus::Terminal,
        ];
        let outcomes = [
            None,
            Some(TaskOutcome::Completed {
                value: serde_json::json!({"nested":[1, 2.5, "é\"\\\u{2028}"], "z":null}),
            }),
            Some(TaskOutcome::Failed {
                error: "failed".into(),
            }),
            Some(TaskOutcome::Aborted {
                reason: "aborted\n".into(),
            }),
            Some(TaskOutcome::Faulted {
                error: "x".repeat(5000),
            }),
        ];
        let blocking = [
            None,
            Some(BlockingReason::MissingDefinition),
            Some(BlockingReason::IncompatibleDefinition {
                target_version: 7,
                detail: "é".into(),
            }),
            Some(BlockingReason::MigrationRejected {
                target_version: u32::MAX,
                detail: "\"quoted\"".into(),
            }),
            Some(BlockingReason::RestartLimit),
            Some(BlockingReason::HandlerFailure {
                detail: "x".repeat(100),
            }),
        ];
        let counters = [
            (1, 1, 0, 0),
            (99, 9, 9, 9),
            (1_000_000, 10, 100, 1000),
            (MAX_PORTABLE_INTEGER, u32::MAX, u32::MAX, u32::MAX),
        ];
        let mut record = record();
        record.input = serde_json::json!({"text":"\u{1F600}","float":1.0});
        record.checkpoint = serde_json::json!({"phase":"prepare","items":[0.5, -3]});
        record.parent = Some("parent".into());
        record.wait_for = vec!["child".into()];
        record.wake_at_ms = Some(1_792_536_326_221);
        for status in statuses {
            for outcome in &outcomes {
                for reason in &blocking {
                    for (revision, version, attempts, restarts) in counters {
                        for cancellation in [false, true] {
                            record.status = status;
                            record.outcome = outcome.clone();
                            record.blocking_reason = reason.clone();
                            record.revision = revision;
                            record.definition_version = version;
                            record.attempts = attempts;
                            record.restarts = restarts;
                            record.cancellation_requested = cancellation;
                            assert_reference_result(&record);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn derived_control_reserve_matches_the_reference_at_each_boundary() -> Result<()> {
        let mut shapes = vec![];
        let mut narrow = record();
        narrow.revision = 1;
        narrow.attempts = 0;
        shapes.push(narrow);
        let mut wide = record();
        wide.revision = MAX_PORTABLE_INTEGER;
        wide.definition_version = u32::MAX;
        wide.attempts = u32::MAX;
        wide.restarts = u32::MAX;
        wide.cancellation_requested = true;
        wide.status = TaskStatus::Terminal;
        wide.outcome = Some(TaskOutcome::Failed {
            error: "x".repeat(2048),
        });
        wide.blocking_reason = Some(BlockingReason::HandlerFailure {
            detail: "y".repeat(1024),
        });
        shapes.push(wide);
        for mut record in shapes {
            let mut low = 0;
            let mut high = MAX_TASK_RECORD_BYTES;
            while low < high {
                let middle = low + (high - low).div_ceil(2);
                record.checkpoint = serde_json::json!("x".repeat(middle));
                if reference_record_size(&record).is_ok() {
                    low = middle;
                } else {
                    high = middle - 1;
                }
            }
            for length in [low.saturating_sub(1), low, low + 1] {
                record.checkpoint = serde_json::json!("x".repeat(length));
                assert_reference_result(&record);
            }
            record.checkpoint = serde_json::json!("x".repeat(low + 1));
            assert_eq!(
                validate_task_record_size(&record).unwrap_err().reason,
                "task_storage_too_large"
            );
        }
        let mut total = record();
        total.checkpoint = serde_json::json!("x".repeat(MAX_TASK_RECORD_BYTES));
        assert_reference_result(&total);
        assert_eq!(
            validate_task_record_encoding(&record(), 3)
                .unwrap_err()
                .reason,
            "task_storage_invalid"
        );
        Ok(())
    }

    #[test]
    fn encoded_transactions_and_receipts_match_complete_measurements() -> Result<()> {
        let mut first = record();
        first.input = serde_json::json!({"€":"\u{1F600}","escaped":"\"\\\n\u{7}"});
        first.checkpoint = serde_json::json!({
            "integral": 1.0,
            "fraction": 2.5,
            "nested": [{"b": 1, "a": [null, true, -0.0]}],
            "big": MAX_PORTABLE_INTEGER,
        });
        let mut second = first.clone();
        second.id = "second".into();
        second.revision = 9;
        second.status = TaskStatus::Terminal;
        second.outcome = Some(TaskOutcome::Completed {
            value: serde_json::json!("done"),
        });
        let mut transaction = TaskTransaction {
            owner: "owner \"é\" \\".into(),
            writes: vec![],
        };
        for (index, record) in [first, second].into_iter().enumerate() {
            transaction.writes.push(TaskWrite {
                provenance: (index == 1).then_some(TaskTransitionProvenance {
                    origin: TaskLifecycleOrigin::Handler,
                    checkpoint_committed: true,
                }),
                expected_revision: (index == 1).then_some(8),
                record,
            });
            for count in [transaction.writes.len() - 1, transaction.writes.len()] {
                let prefix = TaskTransaction {
                    owner: transaction.owner.clone(),
                    writes: transaction.writes[..count].to_vec(),
                };
                let encoded = encode_task_transaction(&prefix)?;
                assert_eq!(encoded.bytes, task_wire_bytes(&prefix)?);
                assert_eq!(encoded.records.len(), count);
                let records: Vec<_> = prefix
                    .writes
                    .iter()
                    .map(|write| write.record.clone())
                    .collect();
                for (text, record) in encoded.records.iter().zip(&records) {
                    let value =
                        tembra::contracts::to_portable_value(record).map_err(nonportable)?;
                    assert_eq!(
                        text,
                        &tembra::contracts::canonical_json(&value).map_err(nonportable)?
                    );
                    validate_task_record_encoding(record, text.len())?;
                }
                for sequence in [1, 4096, MAX_PORTABLE_INTEGER] {
                    assert_eq!(
                        committed_receipt_bytes(sequence, encoded.records.iter().map(String::len))?,
                        task_wire_bytes(&CommitResult::Committed {
                            sequence,
                            records: records.clone(),
                        })?
                    );
                }
            }
        }
        // Inside a transaction, a record's input is nested within four containers.
        let nested = |depth| {
            let mut value = serde_json::Value::Null;
            for _ in 0..depth {
                value = serde_json::json!([value]);
            }
            value
        };
        transaction.writes[0].record.input = nested(124);
        assert!(task_wire_bytes(&transaction).is_ok());
        assert_eq!(
            encode_task_transaction(&transaction)?.bytes,
            task_wire_bytes(&transaction)?
        );
        transaction.writes[0].record.input = nested(125);
        assert!(task_wire_bytes(&transaction).is_err());
        assert_eq!(
            encode_task_transaction(&transaction).unwrap_err().reason,
            "task_storage_invalid"
        );
        transaction.writes[0].record.input = serde_json::json!([MAX_PORTABLE_INTEGER + 1]);
        assert_eq!(
            encode_task_transaction(&transaction).unwrap_err().reason,
            "task_storage_invalid"
        );
        assert_eq!(
            committed_receipt_bytes(MAX_PORTABLE_INTEGER + 1, [])
                .unwrap_err()
                .reason,
            "task_storage_invalid"
        );
        Ok(())
    }
}
