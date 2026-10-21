use rusqlite::{OptionalExtension, TransactionBehavior, params};
use swarmie_session_contracts::*;
use tembra::{Result, RuntimeError};

use crate::{
    database::{Store, invalid, io, metadata},
    reads::record,
};

impl Store {
    pub(crate) fn create(&self, request: CreateSession) -> Result<SessionRecord> {
        identity(&request.session_id)?;
        bounded(&request, 64 * 1024)?;
        let encoded = canonical(&request.metadata)?;
        self.with(|connection| {
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(io)?;
            if let Some(record) = record(&transaction, &request.session_id)? {
                if canonical(&record.metadata)? != encoded {
                    return Err(RuntimeError::invalid(
                        "session_store_duplicate",
                        "session identity has different metadata",
                    ));
                }
                return Ok(record);
            }
            let head = metadata(&transaction)?;
            let sequence = next(head.commit_sequence, 1)?;
            transaction
                .execute(
                    "INSERT INTO sessions VALUES(?1,0,?2)",
                    params![request.session_id, encoded],
                )
                .map_err(io)?;
            transaction
                .execute(
                    "UPDATE session_store SET sequence=?1 WHERE singleton=1",
                    [sequence],
                )
                .map_err(io)?;
            transaction.commit().map_err(io)?;
            Ok(SessionRecord {
                store_id: head.store_id,
                session_id: request.session_id,
                head: 0,
                metadata: serde_json::from_str(&encoded).map_err(io)?,
            })
        })
    }

    pub(crate) fn append(&self, request: AppendEvents) -> Result<AppendReceipt> {
        validate_append(&request)?;
        let encoded = canonical(&request)?;
        // Normalize numeric JSON before preserving or comparing it across language boundaries.
        let request: AppendEvents = serde_json::from_str(&encoded).map_err(io)?;
        self.with(|connection| {
            let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate).map_err(io)?;
            let prior: Option<(String, String)> = transaction.query_row(
                "SELECT request,receipt FROM session_batches WHERE session_id=?1 AND batch_id=?2",
                params![request.session_id, request.batch_id], |row| Ok((row.get(0)?, row.get(1)?))).optional().map_err(io)?;
            if let Some((prior, receipt)) = prior {
                if prior != encoded { return Err(RuntimeError::invalid("session_store_duplicate", "batch identity has a different request")); }
                return crate::database::decode(&receipt);
            }
            let session = record(&transaction, &request.session_id)?.ok_or_else(|| RuntimeError::invalid("session_store_missing", "session does not exist"))?;
            if request.expected_head.is_some_and(|n| n != session.head) { return Err(RuntimeError::invalid("session_store_conflict", format!("expected head differs from committed head {}", session.head))); }
            let head = metadata(&transaction)?;
            let last_sequence = next(session.head, request.events.len() as u64)?;
            let receipt = AppendReceipt { store_id: head.store_id, session_id: request.session_id.clone(), batch_id: request.batch_id.clone(), first_sequence: session.head + 1, last_sequence, commit_sequence: next(head.commit_sequence,1)? };
            // Validate every complete persisted envelope before inserting the first row.
            let mut events = Vec::with_capacity(request.events.len());
            for (index, event) in request.events.into_iter().enumerate() {
                let sequence = receipt.first_sequence + index as u64;
                if event.kind == COMPACTION && event.version == 1 {
                    let compaction: Compaction = serde_json::from_value(event.payload.clone()).map_err(io)?;
                    if compaction.through_sequence >= sequence { return Err(invalid("compaction must refer to an earlier committed prefix")); }
                }
                let event = SessionEvent { session_id: request.session_id.clone(), sequence, event };
                bounded(&event, MAX_EVENT_BYTES)?;
                let duplicate: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM session_events WHERE session_id=?1 AND event_id=?2)", params![request.session_id, event.event.event_id], |row| row.get(0)).map_err(io)?;
                if duplicate { return Err(RuntimeError::invalid("session_store_duplicate", "event identity already exists; retry its original batch")); }
                events.push(event);
            }
            for event in events {
                transaction.execute("INSERT INTO session_events VALUES(?1,?2,?3,?4,?5)", params![request.session_id,event.sequence,event.event.event_id,event.event.correlation.turn_id,canonical(&event)?]).map_err(io)?;
            }
            transaction.execute("UPDATE sessions SET head=?2 WHERE id=?1", params![request.session_id,last_sequence]).map_err(io)?;
            transaction.execute("INSERT INTO session_batches VALUES(?1,?2,?3,?4)", params![request.session_id,request.batch_id,encoded,canonical(&receipt)?]).map_err(io)?;
            transaction.execute("UPDATE session_store SET sequence=?1 WHERE singleton=1", [receipt.commit_sequence]).map_err(io)?;
            transaction.commit().map_err(io)?;
            Ok(receipt)
        })
    }
}

fn next(sequence: u64, count: u64) -> Result<u64> {
    sequence
        .checked_add(count)
        .filter(|n| *n <= MAX_SEQUENCE)
        .ok_or_else(|| invalid("portable sequence exhausted"))
}
