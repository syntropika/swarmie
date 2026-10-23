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
/// complete record ceiling; callers should bound diagnostics to the reserved space. The record
/// is encoded once, and both measurements are derived from that encoding.
///
/// # Errors
/// Returns `task_storage_too_large` when the complete record or its non-control fields exceed
/// their fixed budgets, or `task_storage_invalid` for nonportable JSON. No value is modified.
pub fn validate_task_record_size(record: &TaskRecord) -> Result<usize> {
    let record = tembra::contracts::to_portable_value(record).map_err(nonportable)?;
    validate_task_record_encoding(&encode_record(record)?)
}

/// Applies [`validate_task_record_size`] to a record that [`encode_task_transaction`] already
/// encoded. Both measurements come from that single encoding; no part of the record, including
/// a large outcome or blocking diagnostic, is encoded or measured again.
///
/// # Errors
/// Returns `task_storage_too_large` exactly when [`validate_task_record_size`] does for the
/// same record. No value is modified.
pub fn validate_task_record_encoding(record: &EncodedTaskRecord) -> Result<usize> {
    let size = within(record.text.len(), MAX_TASK_RECORD_BYTES, "task record")?;
    within(
        record.reserved,
        MAX_TASK_RECORD_BYTES - MAX_TASK_CONTROL_BYTES,
        "task record without control fields",
    )?;
    Ok(size)
}

/// One task record encoded once for storage and exact size accounting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedTaskRecord {
    text: String,
    /// Canonical size of the same record without `outcome` and `blocking_reason`, with the
    /// widest revision, version, retry, cancellation and status encodings.
    reserved: usize,
}

impl EncodedTaskRecord {
    /// Canonical portable text of the record. It equals the encoding that [`task_wire_bytes`]
    /// measures, so its length is the record's exact wire size.
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// Encodes a portable record once by joining its canonically encoded members.
///
/// RFC 8785 writes an object as its members, each a canonical name, a colon and its
/// independently encoded value, sorted by the UTF-16 code units of their names, separated by
/// commas and enclosed in braces. Joining members encoded once in that order reproduces the
/// complete encoding. The same member texts give the exact size of the record without its
/// control members and with the widest counter encodings, which is the second measurement of
/// [`validate_task_record_size`], so no value is encoded or measured twice.
fn encode_record(record: serde_json::Value) -> Result<EncodedTaskRecord> {
    let serde_json::Value::Object(members) = record else {
        return Err(RuntimeError::invalid(
            "task_storage_invalid",
            "a task record must encode as an object",
        ));
    };
    let mut members = members
        .into_iter()
        .map(|(name, value)| {
            let value = tembra::contracts::canonical_json(&value).map_err(nonportable)?;
            Ok((name, value))
        })
        .collect::<Result<Vec<_>>>()?;
    members.sort_unstable_by(|(left, _), (right, _)| left.encode_utf16().cmp(right.encode_utf16()));
    let widest_status = task_wire_bytes(&TaskStatus::Completing)?;
    // Braces, then each member's quoted name, colon, value and separating comma.
    let mut text = String::with_capacity(
        2 + members
            .iter()
            .map(|(name, value)| name.len() + value.len() + 4)
            .sum::<usize>(),
    );
    let mut reserved = "{}".len();
    let mut kept = 0;
    text.push('{');
    for (index, (name, value)) in members.iter().enumerate() {
        if index > 0 {
            text.push(',');
        }
        let start = text.len();
        push_name(&mut text, name)?;
        let name_bytes = text.len() - start;
        text.push(':');
        text.push_str(value);
        let width = match name.as_str() {
            "outcome" | "blocking_reason" => continue,
            "revision" => integer_bytes(MAX_PORTABLE_INTEGER),
            "definition_version" | "attempts" | "restarts" => integer_bytes(u32::MAX.into()),
            "cancellation_requested" => boolean_bytes(false),
            "status" => widest_status,
            _ => value.len(),
        };
        // The reserved object keeps this member: one comma after its first member, the
        // canonical name, a colon and the value at its reserved width.
        reserved += usize::from(kept > 0) + name_bytes + ":".len() + width;
        kept += 1;
    }
    text.push('}');
    Ok(EncodedTaskRecord { text, reserved })
}

/// Appends the canonical text of a member name. RFC 8785 escapes only quotation marks, reverse
/// solidi and control characters, so a name of ASCII letters, digits and underscores, such as
/// every task record member, is quoted verbatim. Any other name is encoded as a string.
fn push_name(text: &mut String, name: &str) -> Result<()> {
    if name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        text.push('"');
        text.push_str(name);
        text.push('"');
    } else {
        let name = serde_json::Value::String(name.to_owned());
        text.push_str(&tembra::contracts::canonical_json(&name).map_err(nonportable)?);
    }
    Ok(())
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
    /// Each written record encoded once, in write order.
    pub records: Vec<EncodedTaskRecord>,
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
        let record = encode_record(record)?;
        let envelope = tembra::contracts::canonical_json(&envelope)
            .map_err(nonportable)?
            .len();
        bytes += usize::from(index > 0) + envelope - "null".len() + record.text.len();
        records.push(record);
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

    /// Compares both derived paths with the reference: a standalone record and the same record
    /// encoded inside a transaction, whose writes are converted at their nested depth.
    fn assert_reference_result(record: &TaskRecord) {
        let encoded = encode_task_transaction(&TaskTransaction {
            owner: "owner".into(),
            writes: vec![TaskWrite {
                provenance: None,
                expected_revision: None,
                record: record.clone(),
            }],
        })
        .expect("a shallow portable record encodes inside a transaction");
        for actual in [
            validate_task_record_size(record),
            validate_task_record_encoding(&encoded.records[0]),
        ] {
            match (reference_record_size(record), actual) {
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
        Ok(())
    }

    #[test]
    fn assembled_members_equal_the_complete_canonical_object() -> Result<()> {
        // Escaped names use the general path. The emoji's UTF-16 surrogates sort before the
        // half-width full stop, although its UTF-8 bytes sort after it.
        let object = serde_json::json!({
            "plain_name": 1.0,
            "a": [2.5, "é\"\\\u{2028}", {"z": null, "b": -0.5}],
            "": "empty",
            "é": true,
            "quoted\"name": {"nested": [false]},
            "control\u{7}": -3,
            "\u{1F600}": "supplementary",
            "\u{FF61}": "half-width",
        });
        let encoded = encode_record(object.clone())?;
        assert_eq!(
            encoded.text(),
            tembra::contracts::canonical_json(&object).map_err(nonportable)?
        );
        let record = tembra::contracts::to_portable_value(&record()).map_err(nonportable)?;
        assert_eq!(
            encode_record(record.clone())?.text(),
            tembra::contracts::canonical_json(&record).map_err(nonportable)?
        );
        assert_eq!(
            encode_record(serde_json::json!([])).unwrap_err().reason,
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
                for (written, record) in encoded.records.iter().zip(&records) {
                    let value =
                        tembra::contracts::to_portable_value(record).map_err(nonportable)?;
                    assert_eq!(
                        written.text(),
                        tembra::contracts::canonical_json(&value).map_err(nonportable)?
                    );
                    assert_eq!(
                        validate_task_record_encoding(written)?,
                        reference_record_size(record)?
                    );
                }
                for sequence in [1, 4096, MAX_PORTABLE_INTEGER] {
                    assert_eq!(
                        committed_receipt_bytes(
                            sequence,
                            encoded.records.iter().map(|record| record.text().len())
                        )?,
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
