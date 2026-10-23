use rusqlite::{OptionalExtension, params};
use swarmie_session_contracts::*;
use tembra::{Result, RuntimeError};

use crate::{
    database::{Store, Transaction, commit_sequence, decode_stored, invalid, io},
    reads::{record, session_head},
};

impl Store {
    pub(crate) fn create(&self, request: CreateSession) -> Result<SessionRecord> {
        identity(&request.session_id)?;
        bounded(&request, 64 * 1024)?;
        let encoded = canonical(&request.metadata)?;
        self.with(|connection| {
            let transaction = Transaction::immediate(connection)?;
            if let Some(record) = record(&transaction, &self.store_id, &request.session_id)? {
                if canonical(&record.metadata)? != encoded {
                    return Err(RuntimeError::invalid(
                        "session_store_duplicate",
                        "session identity has different metadata",
                    ));
                }
                return Ok(record);
            }
            let sequence = next(commit_sequence(&transaction)?, 1)?;
            transaction
                .prepare_cached("INSERT INTO sessions VALUES(?1,0,?2)")
                .and_then(|mut statement| statement.execute(params![request.session_id, encoded]))
                .map_err(io)?;
            transaction
                .prepare_cached("UPDATE session_store SET sequence=?1 WHERE singleton=1")
                .and_then(|mut statement| statement.execute([sequence]))
                .map_err(io)?;
            transaction.commit()?;
            Ok(SessionRecord {
                store_id: self.store_id.clone(),
                session_id: request.session_id,
                head: 0,
                metadata: serde_json::from_str(&encoded).map_err(io)?,
            })
        })
    }

    pub(crate) fn append(&self, request: AppendEvents) -> Result<AppendReceipt> {
        // Encode every event once. The canonical request is assembled from those texts for
        // exact retry equality, and each stored envelope reuses its event's text. Canonical
        // text also fixes numeric spelling, so no decoded copy of the request is needed.
        let encoded = encode_append(&request)?;
        self.with(|connection| {
            let transaction = Transaction::immediate(connection)?;
            let prior: Option<(String, String)> = transaction
                .prepare_cached("SELECT request,receipt FROM session_batches WHERE session_id=?1 AND batch_id=?2")
                .and_then(|mut statement| statement.query_row(params![request.session_id, request.batch_id], |row| Ok((row.get(0)?, row.get(1)?))).optional())
                .map_err(io)?;
            if let Some((prior, receipt)) = prior {
                if prior != encoded.request() { return Err(RuntimeError::invalid("session_store_duplicate", "batch identity has a different request")); }
                return decode_stored(&receipt);
            }
            let head = session_head(&transaction, &request.session_id)?.ok_or_else(|| RuntimeError::invalid("session_store_missing", "session does not exist"))?;
            if request.expected_head.is_some_and(|n| n != head) { return Err(RuntimeError::invalid("session_store_conflict", format!("expected head differs from committed head {head}"))); }
            let last_sequence = next(head, request.events.len() as u64)?;
            let receipt = AppendReceipt { store_id: self.store_id.clone(), session_id: request.session_id.clone(), batch_id: request.batch_id.clone(), first_sequence: head + 1, last_sequence, commit_sequence: next(commit_sequence(&transaction)?, 1)? };
            // Validate every complete persisted envelope before inserting the first row.
            let mut rows = Vec::with_capacity(request.events.len());
            for (index, event) in request.events.iter().enumerate() {
                let sequence = receipt.first_sequence + index as u64;
                if event.kind == COMPACTION && event.version == 1 {
                    let compaction: Compaction = serde_json::from_value(event.payload.clone()).map_err(io)?;
                    if compaction.through_sequence >= sequence { return Err(invalid("compaction must refer to an earlier committed prefix")); }
                }
                let stored = encoded.event(index, sequence);
                bounded_size(stored.len(), MAX_EVENT_BYTES)?;
                let duplicate: bool = transaction
                    .prepare_cached("SELECT EXISTS(SELECT 1 FROM session_events WHERE session_id=?1 AND event_id=?2)")
                    .and_then(|mut statement| statement.query_row(params![request.session_id, event.event_id], |row| row.get(0)))
                    .map_err(io)?;
                if duplicate { return Err(RuntimeError::invalid("session_store_duplicate", "event identity already exists; retry its original batch")); }
                rows.push((event, sequence, stored));
            }
            {
                let mut insert = transaction.prepare_cached("INSERT INTO session_events(session_id,sequence,event_id,conversation_id,turn_id,event) VALUES(?1,?2,?3,?4,?5,?6)").map_err(io)?;
                for (event, sequence, stored) in rows {
                    insert.execute(params![request.session_id,sequence,event.event_id,event.correlation.conversation_id,event.correlation.turn_id,stored]).map_err(io)?;
                }
            }
            transaction.prepare_cached("UPDATE sessions SET head=?2 WHERE id=?1").and_then(|mut statement| statement.execute(params![request.session_id,last_sequence])).map_err(io)?;
            let receipt_text = canonical(&receipt)?;
            transaction.prepare_cached("INSERT INTO session_batches VALUES(?1,?2,?3,?4)").and_then(|mut statement| statement.execute(params![request.session_id,request.batch_id,encoded.request(),receipt_text])).map_err(io)?;
            transaction.prepare_cached("UPDATE session_store SET sequence=?1 WHERE singleton=1").and_then(|mut statement| statement.execute([receipt.commit_sequence])).map_err(io)?;
            transaction.commit()?;
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
