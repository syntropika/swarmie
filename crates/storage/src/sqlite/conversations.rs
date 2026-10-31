use crate::sqlite::{SqliteOwner, SqliteWrite, owner::Lease};
use futures::{FutureExt, future::LocalBoxFuture};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::{collections::BTreeSet, path::Path};
use swarmie_conversations::{
    Conversation, ConversationId, ConversationStorage, Entry, EntryId, Head, HistoryPoint, Page,
    Read, Rejection, StorageError, Write, WriteConfirmation,
};
use tembra::Context;

/// Conversation-level branching by reference. Forks store metadata and local
/// entries only; reads resolve immutable ancestor prefixes without copying payloads.
pub struct SqliteConversationStorage {
    pub(crate) lease: Lease,
}

impl SqliteConversationStorage {
    /// Opens history storage. Use [`SqliteOwner::conversations`] to share an owner.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        SqliteOwner::new(path.as_ref().to_path_buf()).conversations()
    }

    fn check(&self) -> Result<(), StorageError> {
        self.lease.check()?;
        self.owner().check()?;
        Ok(())
    }
}

impl ConversationStorage for SqliteConversationStorage {
    fn owner(&self) -> swarmie_storage_core::StorageOwner {
        self.lease.database.health.clone()
    }

    fn get<'a>(
        &'a self,
        principal: &'a str,
        id: ConversationId,
    ) -> LocalBoxFuture<'a, Result<Option<Conversation>, StorageError>> {
        async move {
            self.check()?;
            let stored = self.lease.database.connection.borrow();
            let connection = stored.as_ref().ok_or(StorageError::Closed)?;
            record(connection, principal, id)
        }
        .boxed_local()
    }

    fn read<'a>(
        &'a self,
        principal: &'a str,
        request: Read,
    ) -> LocalBoxFuture<'a, Result<Page, StorageError>> {
        async move {
            self.check()?;
            let stored = self.lease.database.connection.borrow();
            let connection = stored.as_ref().ok_or(StorageError::Closed)?;
            page(connection, principal, request)
        }
        .boxed_local()
    }

    fn write<'a>(
        &'a self,
        context: &'a Context,
        write: Write,
    ) -> LocalBoxFuture<'a, tembra::Result<WriteConfirmation>> {
        async move {
            context.check()?;
            self.check()?;
            write.head()?;
            let principal = context.principal()?;
            let command =
                SqliteWrite::new(move |tx| apply(tx, &principal, write).map_err(Into::into));
            let Some(mut command) = self.owner().try_stage(context, command)? else {
                return Ok(WriteConfirmation::Staged);
            };
            let mut stored = self.lease.database.connection.borrow_mut();
            let connection = stored.as_mut().ok_or(StorageError::Closed)?;
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(sql_error)?;
            command.apply(&transaction)?;
            if let Err(error) = transaction.commit() {
                drop(stored);
                self.owner().require_recovery();
                return Err(StorageError::Unknown(error.to_string()).into());
            }
            Ok(WriteConfirmation::Committed)
        }
        .boxed_local()
    }

    fn close(&self) -> LocalBoxFuture<'_, Result<(), StorageError>> {
        async move { self.lease.close().map_err(StorageError::Unavailable) }.boxed_local()
    }
}

fn refused(reason: Rejection) -> StorageError {
    StorageError::Refused(reason)
}

fn sql_error(error: rusqlite::Error) -> StorageError {
    if crate::sqlite::owner::invalid_sql(&error) {
        StorageError::Invalid(error.to_string())
    } else {
        StorageError::Unavailable(error.to_string())
    }
}

fn decode<T: std::str::FromStr>(value: String) -> Result<T, StorageError>
where
    T::Err: std::fmt::Display,
{
    value
        .parse()
        .map_err(|e: T::Err| StorageError::Invalid(e.to_string()))
}

fn record(
    connection: &Connection,
    principal: &str,
    id: ConversationId,
) -> Result<Option<Conversation>, StorageError> {
    let raw = connection
        .query_row(
            "SELECT principal,parent,parent_entry,head FROM swarmie_conversations WHERE id=?1",
            [id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()
        .map_err(sql_error)?;
    let Some((owner, parent, parent_entry, head)) = raw else {
        return Ok(None);
    };
    if owner != principal {
        return Err(refused(Rejection::Forbidden));
    }
    let origin = match (parent, parent_entry) {
        (None, None) => None,
        (Some(parent), Some(entry)) => Some(HistoryPoint {
            conversation: decode(parent)?,
            entry: decode(entry)?,
        }),
        _ => return Err(StorageError::Invalid("incomplete history origin".into())),
    };
    Ok(Some(Conversation {
        head: Head {
            conversation: id,
            entry: head.map(decode).transpose()?,
        },
        origin,
    }))
}

fn position(connection: &Connection, id: EntryId) -> Result<(ConversationId, u64), StorageError> {
    let raw = connection
        .query_row(
            "SELECT conversation,position FROM swarmie_conversation_entries WHERE id=?1",
            [id.to_string()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?)),
        )
        .optional()
        .map_err(sql_error)?
        .ok_or_else(|| refused(Rejection::NotFound))?;
    Ok((decode(raw.0)?, raw.1))
}

struct Segment {
    conversation: ConversationId,
    through: u64,
}

/// Build only lineage metadata. Truncating an inherited prefix may remove whole
/// segments; ancestry and every cutoff are checked before returning any payload.
fn segments(
    connection: &Connection,
    principal: &str,
    at: Head,
) -> Result<Vec<Segment>, StorageError> {
    let mut lineage = Vec::new();
    let mut visited = BTreeSet::new();
    let mut current = at;
    loop {
        if !visited.insert(current.conversation) {
            return Err(StorageError::Invalid("cyclic conversation origin".into()));
        }
        let conversation = record(connection, principal, current.conversation)?
            .ok_or_else(|| refused(Rejection::NotFound))?;
        lineage.push(current);
        let Some(origin) = conversation.origin else {
            break;
        };
        current = origin.into();
    }
    let mut result: Vec<Segment> = Vec::new();
    for head in lineage.into_iter().rev() {
        let Some(entry) = head.entry else {
            if !result.is_empty() {
                return Err(refused(Rejection::Invalid));
            }
            continue;
        };
        let (conversation, through) = position(connection, entry)?;
        if conversation == head.conversation {
            result.push(Segment {
                conversation,
                through,
            });
        } else {
            let index = result
                .iter()
                .position(|segment| {
                    segment.conversation == conversation && through <= segment.through
                })
                .ok_or_else(|| refused(Rejection::Invalid))?;
            result.truncate(index + 1);
            result[index].through = through;
        }
    }
    Ok(result)
}

fn page(connection: &Connection, principal: &str, request: Read) -> Result<Page, StorageError> {
    let segments = segments(connection, principal, request.at)?;
    let cursor = request
        .after
        .map(|id| position(connection, id))
        .transpose()?;
    let start = if let Some((conversation, position)) = cursor {
        segments
            .iter()
            .position(|segment| segment.conversation == conversation && position <= segment.through)
            .ok_or_else(|| refused(Rejection::Invalid))?
    } else {
        0
    };
    let limit = usize::from(request.limit.min(Read::MAX_LIMIT).get());
    let mut entries = Vec::new();
    for (index, segment) in segments.iter().enumerate().skip(start) {
        let after = if index == start {
            cursor.map_or(0, |(_, position)| position)
        } else {
            0
        };
        let mut statement=connection.prepare("SELECT id,data FROM swarmie_conversation_entries WHERE conversation=?1 AND position>?2 AND position<=?3 ORDER BY position LIMIT ?4").map_err(sql_error)?;
        let rows = statement
            .query_map(
                params![
                    segment.conversation.to_string(),
                    after,
                    segment.through,
                    (limit + 1 - entries.len()) as u64
                ],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .map_err(sql_error)?;
        for row in rows {
            let (id, data) = row.map_err(sql_error)?;
            entries.push(Entry {
                id: decode(id)?,
                conversation: segment.conversation,
                data: serde_json::from_str(&data)
                    .map_err(|e| StorageError::Invalid(e.to_string()))?,
            });
        }
        if entries.len() > limit {
            break;
        }
    }
    let next = if entries.len() > limit {
        entries.pop();
        entries.last().map(|entry| entry.id)
    } else {
        None
    };
    Ok(Page { entries, next })
}

fn apply(connection: &Connection, principal: &str, write: Write) -> Result<(), StorageError> {
    match write {
        Write::Create { id, origin } => {
            let exists: bool = connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM swarmie_conversations WHERE id=?1)",
                    [id.to_string()],
                    |row| row.get(0),
                )
                .map_err(sql_error)?;
            if exists {
                return Err(refused(Rejection::AlreadyExists));
            }
            if let Some(point) = origin {
                segments(connection, principal, point.into())?;
            }
            connection.execute("INSERT INTO swarmie_conversations(id,principal,parent,parent_entry,head) VALUES(?1,?2,?3,?4,?4)",params![id.to_string(),principal,origin.map(|p|p.conversation.to_string()),origin.map(|p|p.entry.to_string())]).map_err(sql_error)?;
        }
        Write::Append { at, entries } => {
            if entries.is_empty() {
                return Err(refused(Rejection::Invalid));
            }
            let conversation = record(connection, principal, at.conversation)?
                .ok_or_else(|| refused(Rejection::NotFound))?;
            if conversation.head != at {
                return Err(refused(Rejection::Conflict));
            }
            let mut next:u64=connection.query_row("SELECT COALESCE(MAX(position),0) FROM swarmie_conversation_entries WHERE conversation=?1",[at.conversation.to_string()],|row|row.get(0)).map_err(sql_error)?;
            let mut seen = BTreeSet::new();
            for entry in &entries {
                if !seen.insert(entry.id) {
                    return Err(refused(Rejection::AlreadyExists));
                }
                let exists: bool = connection
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM swarmie_conversation_entries WHERE id=?1)",
                        [entry.id.to_string()],
                        |row| row.get(0),
                    )
                    .map_err(sql_error)?;
                if exists {
                    return Err(refused(Rejection::AlreadyExists));
                }
                next = next
                    .checked_add(1)
                    .filter(|position| *position <= i64::MAX as u64)
                    .ok_or_else(|| refused(Rejection::Exhausted))?;
                connection.execute("INSERT INTO swarmie_conversation_entries(id,conversation,position,data) VALUES(?1,?2,?3,?4)",params![entry.id.to_string(),at.conversation.to_string(),next,serde_json::to_string(&entry.data).map_err(|e|StorageError::Invalid(e.to_string()))?]).map_err(sql_error)?;
            }
            connection
                .execute(
                    "UPDATE swarmie_conversations SET head=?1 WHERE id=?2",
                    params![
                        entries.last().unwrap().id.to_string(),
                        at.conversation.to_string()
                    ],
                )
                .map_err(sql_error)?;
        }
    }
    Ok(())
}
