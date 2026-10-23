//! Session history reads and loop-owned appends of one admitted root.
//!
//! Every read and append borrows the [`Admitted`] proof of the root that performs it instead of
//! confirming the association again. The proof does not make the I/O atomic with confirmation:
//! every returned page and receipt is still checked against the proof's routed destination
//! store, session and turn selector, and a foreign identity never establishes a checkpoint.

use swarmie_agent_contracts::TurnContext;
use swarmie_session_contracts::{
    AppendEvents, Correlation, EventQuery, NewEvent, SessionEvent, SessionStoreRef,
};
use tembra::{Context, Result, RuntimeError};

use crate::{
    admission::Admitted,
    state::{TurnIdentity, stable},
};

/// Loop admission budget for one turn's receipts, in canonical wire bytes.
const MAX_HISTORY_BYTES: usize = 4 * 1024 * 1024;

/// Loop admission budget for one turn's receipts, in events.
const MAX_HISTORY_EVENTS: usize = 4096;

/// Current-turn receipts that one root read once, extended with that root's own receipted
/// appends. Events that other roots append after the read, such as the receipts of a model
/// terminal invoked by a phase, appear only to a later root's read. A root decides only on
/// receipts committed before its read or appended by itself.
pub(crate) struct HistoryView {
    events: Vec<SessionEvent>,
    bytes: usize,
}

impl HistoryView {
    /// Returns the prefix, refusing it once this root's own appends have grown it past the
    /// admission budget that a fresh read of the same prefix would enforce.
    pub(crate) fn events(&self) -> Result<&[SessionEvent]> {
        if self.bytes > MAX_HISTORY_BYTES || self.events.len() > MAX_HISTORY_EVENTS {
            return Err(history_limit());
        }
        Ok(&self.events)
    }

    /// Adds one receipted append of this root at its committed sequence. An exact retry that the
    /// store answered with its original receipt is already present and is not added twice.
    fn record(&mut self, event: SessionEvent) -> Result<()> {
        if self
            .events
            .iter()
            .any(|known| known.event.event_id == event.event.event_id)
        {
            return Ok(());
        }
        self.bytes += swarmie_session_contracts::wire_bytes(&event)?;
        let position = self
            .events
            .partition_point(|known| known.sequence < event.sequence);
        self.events.insert(position, event);
        Ok(())
    }
}

/// Session access of one root: its admission proof and the current-turn history it reads at
/// most once. Later reads return that prefix together with this root's own appends.
pub(crate) struct Journal {
    admitted: Admitted,
    history: Option<HistoryView>,
}

impl Journal {
    pub(crate) fn new(admitted: Admitted) -> Self {
        Self {
            admitted,
            history: None,
        }
    }

    pub(crate) fn admitted(&self) -> &Admitted {
        &self.admitted
    }

    /// Reads the confirmed turn's receipts on first use; later uses perform no Session I/O.
    pub(crate) async fn history(
        &mut self,
        store: &SessionStoreRef,
        context: &Context,
    ) -> Result<&[SessionEvent]> {
        let view = match &mut self.history {
            Some(view) => view,
            empty @ None => empty.insert(history(store, &self.admitted, context).await?),
        };
        view.events()
    }

    /// Appends through this root's proof and extends its history view with the stored event.
    pub(crate) async fn append(
        &mut self,
        store: &SessionStoreRef,
        turn: &TurnContext,
        suffix: &str,
        kind: &str,
        payload: serde_json::Value,
        context: &Context,
    ) -> Result<String> {
        let event = event(turn, suffix, kind, payload);
        let id = event.event_id.clone();
        if self.history.is_none() {
            commit(store, &self.admitted, turn, event, context).await?;
            return Ok(id);
        }
        let sequence = commit(store, &self.admitted, turn, event.clone(), context).await?;
        if let Some(view) = &mut self.history {
            view.record(SessionEvent {
                session_id: turn.session_id.clone(),
                sequence,
                event,
            })?;
        }
        Ok(id)
    }
}

/// Reads the confirmed turn's immutable prefix with bounded total admission. The Session
/// provider owns paging. Only current-turn receipts are materialized here; conversation context
/// uses selection first.
pub(crate) async fn history(
    store: &SessionStoreRef,
    admitted: &Admitted,
    context: &Context,
) -> Result<HistoryView> {
    let mut query = EventQuery::new(admitted.session_id());
    query.conversation_id = Some(admitted.conversation_id().into());
    query.turn_id = Some(admitted.turn_id().into());
    let mut events = Vec::new();
    let mut bytes = 0;
    loop {
        let page = store.with_context(context).events(query.clone()).await?;
        if page.store_id != admitted.destination().session_store_id {
            return Err(crate::state::invalid(
                "session history belongs to a different routed destination",
            ));
        }
        for event in page.events {
            if event.session_id != admitted.session_id()
                || event.event.correlation.conversation_id.as_deref()
                    != Some(admitted.conversation_id())
                || event.event.correlation.turn_id.as_deref() != Some(admitted.turn_id())
            {
                return Err(crate::state::invalid(
                    "session returned an event outside its turn selector",
                ));
            }
            bytes += swarmie_session_contracts::wire_bytes(&event)?;
            if bytes > MAX_HISTORY_BYTES || events.len() >= MAX_HISTORY_EVENTS {
                return Err(history_limit());
            }
            events.push(event);
        }
        let Some(cursor) = page.next else {
            break;
        };
        query.cursor = Some(cursor);
    }
    Ok(HistoryView { events, bytes })
}

fn history_limit() -> RuntimeError {
    RuntimeError::invalid(
        "agent_history_limit",
        "session prefix exceeds loop admission budget",
    )
}

/// Finds admission evidence with bounded page memory, without materializing conversation
/// history. Lookup cost is proportional to the conversation prefix; every page remains
/// cancelable and explicitly conversation-filtered.
pub(crate) async fn input(
    store: &SessionStoreRef,
    turn: &impl TurnIdentity,
    event_id: &str,
    expected_store_id: &str,
    context: &Context,
) -> Result<Option<SessionEvent>> {
    let mut query = EventQuery::new(turn.session_id());
    query.conversation_id = Some(turn.conversation_id().into());
    loop {
        context.check(context.host_id())?;
        let page = store.with_context(context).events(query.clone()).await?;
        if page.store_id != expected_store_id {
            return Err(crate::state::invalid(
                "admission input belongs to a different session store",
            ));
        }
        for event in page.events {
            if event.session_id != turn.session_id()
                || event.event.correlation.conversation_id.as_deref()
                    != Some(turn.conversation_id())
            {
                return Err(crate::state::invalid(
                    "session returned an event outside its conversation selector",
                ));
            }
            if event.event.event_id == event_id {
                return Ok(Some(event));
            }
        }
        let Some(cursor) = page.next else {
            return Ok(None);
        };
        query.cursor = Some(cursor);
    }
}

pub(crate) fn find<'a>(events: &'a [SessionEvent], id: &str) -> Option<&'a NewEvent> {
    events
        .iter()
        .find(|event| event.event.event_id == id)
        .map(|event| &event.event)
}

/// Exact immutable batch with no changing timestamp or expected head. A lost append response
/// must be retried with this same request; publication never substitutes for the storage commit.
pub(crate) async fn append(
    store: &SessionStoreRef,
    admitted: &Admitted,
    turn: &TurnContext,
    suffix: &str,
    kind: &str,
    payload: serde_json::Value,
    context: &Context,
) -> Result<String> {
    let event = event(turn, suffix, kind, payload);
    let id = event.event_id.clone();
    commit(store, admitted, turn, event, context).await?;
    Ok(id)
}

fn event(turn: &TurnContext, suffix: &str, kind: &str, payload: serde_json::Value) -> NewEvent {
    NewEvent {
        event_id: stable(turn, suffix),
        kind: kind.into(),
        version: if kind.starts_with("swarmie.agent.") {
            2
        } else {
            1
        },
        correlation: Correlation {
            conversation_id: Some(turn.conversation_id.clone()),
            turn_id: Some(turn.turn_id.clone()),
            task_id: Some(turn.task_id.clone()),
            step_id: Some(stable(turn, &format!("step:{}", turn.step))),
            ..Default::default()
        },
        occurred_at_ms: None,
        payload,
    }
}

/// Commits one single-event batch whose identity is the event's, returning its sequence.
async fn commit(
    store: &SessionStoreRef,
    admitted: &Admitted,
    turn: &TurnContext,
    event: NewEvent,
    context: &Context,
) -> Result<u64> {
    admitted.check_turn(turn)?;
    let batch_id = event.event_id.clone();
    let receipt = store
        .with_context(context)
        .append(AppendEvents {
            session_id: turn.session_id.clone(),
            batch_id: batch_id.clone(),
            expected_head: None,
            events: vec![event],
        })
        .await?;
    if receipt.store_id != admitted.destination().session_store_id
        || receipt.session_id != turn.session_id
        || receipt.batch_id != batch_id
    {
        return Err(crate::state::invalid(
            "session append receipt differs from the routed batch identity",
        ));
    }
    Ok(receipt.first_sequence)
}

pub(crate) fn own_event<'a>(
    events: &'a [SessionEvent],
    id: &str,
    turn: &TurnContext,
    kind: &str,
) -> Result<&'a NewEvent> {
    let event =
        find(events, id).ok_or_else(|| crate::state::invalid("referenced event is absent"))?;
    if event.version
        != if kind.starts_with("swarmie.agent.") {
            2
        } else {
            1
        }
        || event.kind != kind
        || event.correlation.conversation_id.as_deref() != Some(&turn.conversation_id)
        || event.correlation.turn_id.as_deref() != Some(&turn.turn_id)
        || event.correlation.task_id.as_deref() != Some(&turn.task_id)
    {
        return Err(crate::state::invalid(
            "event kind, version or correlation differs",
        ));
    }
    Ok(event)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(id: &str, sequence: u64) -> SessionEvent {
        SessionEvent {
            session_id: "session".into(),
            sequence,
            event: NewEvent {
                event_id: id.into(),
                kind: "swarmie.agent.turn-started".into(),
                version: 2,
                correlation: Correlation {
                    conversation_id: Some("conversation".into()),
                    turn_id: Some("turn".into()),
                    ..Default::default()
                },
                occurred_at_ms: None,
                payload: serde_json::json!({"value":sequence}),
            },
        }
    }

    fn view(events: Vec<SessionEvent>) -> HistoryView {
        let bytes = events
            .iter()
            .map(|event| swarmie_session_contracts::wire_bytes(event).unwrap())
            .sum();
        HistoryView { events, bytes }
    }

    #[test]
    fn own_appends_extend_the_view_once_in_sequence_order() {
        let mut history = view(vec![stored("first", 1), stored("third", 3)]);
        history.record(stored("fourth", 4)).unwrap();
        history.record(stored("second", 2)).unwrap();
        // An exact retry answered with its original receipt is already part of the prefix.
        history.record(stored("third", 3)).unwrap();
        let events = history.events().unwrap();
        assert_eq!(
            events
                .iter()
                .map(|event| event.event.event_id.as_str())
                .collect::<Vec<_>>(),
            ["first", "second", "third", "fourth"]
        );
        let expected: usize = events
            .iter()
            .map(|event| swarmie_session_contracts::wire_bytes(event).unwrap())
            .sum();
        assert_eq!(history.bytes, expected);
        assert!(find(events, "second").is_some());
    }

    #[test]
    fn own_appends_cannot_grow_the_view_past_the_read_budget() {
        let mut history = view(
            (1..=MAX_HISTORY_EVENTS as u64)
                .map(|sequence| stored(&sequence.to_string(), sequence))
                .collect(),
        );
        assert!(history.events().is_ok());
        history
            .record(stored("over", MAX_HISTORY_EVENTS as u64 + 1))
            .unwrap();
        assert_eq!(history.events().unwrap_err().reason, "agent_history_limit");
        let oversized = HistoryView {
            events: vec![stored("large", 1)],
            bytes: MAX_HISTORY_BYTES + 1,
        };
        assert_eq!(
            oversized.events().unwrap_err().reason,
            "agent_history_limit"
        );
    }
}
