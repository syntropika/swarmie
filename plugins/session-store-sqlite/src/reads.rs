use rusqlite::{Connection, OptionalExtension, params};
use swarmie_session_contracts::*;
use tembra::{Result, RuntimeError};

use crate::database::{Store, corrupt, decode_stored, invalid, io};

/// Reads the committed head of a session without decoding its metadata.
pub(crate) fn session_head(connection: &Connection, id: &str) -> Result<Option<u64>> {
    let head: Option<u64> = connection
        .prepare_cached("SELECT head FROM sessions WHERE id=?1")
        .and_then(|mut statement| statement.query_row([id], |row| row.get(0)).optional())
        .map_err(io)?;
    if head.is_some_and(|head| head > MAX_SEQUENCE) {
        return Err(corrupt("invalid session record"));
    }
    Ok(head)
}

pub(crate) fn record(
    connection: &Connection,
    store_id: &str,
    id: &str,
) -> Result<Option<SessionRecord>> {
    let row: Option<(u64, String)> = connection
        .prepare_cached("SELECT head,metadata FROM sessions WHERE id=?1")
        .and_then(|mut statement| {
            statement
                .query_row([id], |row| Ok((row.get(0)?, row.get(1)?)))
                .optional()
        })
        .map_err(io)?;
    row.map(|(head, encoded)| {
        if head > MAX_SEQUENCE || encoded.len() > 64 * 1024 {
            return Err(corrupt("invalid session record"));
        }
        Ok(SessionRecord {
            store_id: store_id.into(),
            session_id: id.into(),
            head,
            metadata: decode_stored(&encoded)?,
        })
    })
    .transpose()
}

impl Store {
    pub(crate) fn get(&self, request: SessionLookup) -> Result<Option<SessionRecord>> {
        identity(&request.session_id)?;
        self.with(|connection| record(connection, &self.store_id, &request.session_id))
    }

    pub(crate) fn events(&self, query: EventQuery) -> Result<EventPage> {
        identity(&query.session_id)?;
        if let Some(conversation) = &query.conversation_id {
            identity(conversation)?;
        }
        if let Some(turn) = &query.turn_id {
            identity(turn)?;
        }
        if !(1..=128).contains(&query.limit)
            || !(4096..=MAX_PAGE_BYTES).contains(&(query.max_bytes as usize))
            || query.after_sequence > MAX_SEQUENCE
        {
            return Err(invalid("invalid page row/byte bounds or source sequence"));
        }
        self.with(|connection| {
            let head = session_head(connection, &query.session_id)?.ok_or_else(|| RuntimeError::invalid("session_store_missing", "session does not exist"))?;
            let store_id = &self.store_id;
            let (after, through) = if let Some(cursor) = &query.cursor {
                if &cursor.store_id != store_id || cursor.session_id != query.session_id || cursor.conversation_id != query.conversation_id || cursor.turn_id != query.turn_id || cursor.through_sequence > head || cursor.after_sequence > cursor.through_sequence || query.after_sequence != 0 || query.through_sequence.is_some_and(|n| n != cursor.through_sequence) {
                    return Err(RuntimeError::invalid("session_store_cursor", "cursor belongs to another store, session, prefix or query"));
                }
                (cursor.after_sequence, cursor.through_sequence)
            } else { (query.after_sequence, query.through_sequence.unwrap_or(head)) };
            if after > through || through > head { return Err(invalid("requested prefix is outside committed history")); }
            let continuation = |sequence| EventCursor { store_id: store_id.clone(), session_id: query.session_id.clone(), after_sequence: sequence, through_sequence: through, conversation_id: query.conversation_id.clone(), turn_id: query.turn_id.clone() };
            let mut page = EventPage { store_id: store_id.clone(), session_id: query.session_id.clone(), through_sequence: through, events: vec![], next: None };
            // Reserve a real maximum-width cursor. Incremental row accounting avoids repeatedly
            // serializing the accumulated page; one lookahead row suffices for continuation.
            page.next = Some(continuation(MAX_SEQUENCE));
            let overhead = wire_bytes(&page)?;
            page.next = None;
            let mut bytes = overhead;
            let mut sql = String::from("SELECT sequence,event_id,event,conversation_id,turn_id FROM session_events WHERE session_id=?1 AND sequence>?2 AND sequence<=?3");
            sql.push_str(if query.conversation_id.is_some() { " AND conversation_id=?4" } else { " AND ?4 IS NULL" });
            sql.push_str(if query.turn_id.is_some() { " AND turn_id=?5" } else { " AND ?5 IS NULL" });
            sql.push_str(" ORDER BY sequence LIMIT ?6");
            let mut statement = connection.prepare_cached(&sql).map_err(io)?;
            let mut rows = statement.query(params![query.session_id,after,through,query.conversation_id,query.turn_id,query.limit + 1]).map_err(io)?;
            let mut last = after;
            while let Some(row) = rows.next().map_err(io)? {
                let sequence: u64 = row.get(0).map_err(io)?;
                let encoded: String = row.get(2).map_err(io)?;
                if encoded.len() > MAX_EVENT_BYTES { return Err(corrupt("oversized stored event")); }
                // Rows were admitted as canonical text, so its length is the exact wire size.
                // Budget and row limits are decided before decoding, never for a skipped row.
                let size = encoded.len() + usize::from(!page.events.is_empty());
                if page.events.len() == query.limit as usize || bytes + size > query.max_bytes as usize {
                    if page.events.is_empty() { return Err(RuntimeError::invalid("session_store_too_large", "page budget cannot hold the next event")); }
                    page.next = Some(continuation(last));
                    break;
                }
                let event_id: String = row.get(1).map_err(io)?;
                let conversation_id: Option<String> = row.get(3).map_err(io)?;
                let turn_id: Option<String> = row.get(4).map_err(io)?;
                let event: SessionEvent = decode_stored(&encoded)?;
                if event.sequence != sequence || event.session_id != query.session_id || event.event.event_id != event_id || event.event.correlation.conversation_id != conversation_id || event.event.correlation.turn_id != turn_id { return Err(corrupt("event index disagrees with immutable envelope")); }
                bytes += size;
                last = sequence;
                page.events.push(event);
            }
            Ok(page)
        })
    }
}
