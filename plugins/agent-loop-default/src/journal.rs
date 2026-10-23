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

    /// Adds one receipted append of this root at its committed sequence, in the canonical form
    /// a fresh read returns: the store keeps canonical JSON, so an integral number submitted as
    /// `2.0` reads back as `2`. An exact retry that the store answered with its original receipt
    /// is already present and is not added twice.
    fn record(&mut self, session_id: &str, sequence: u64, event: NewEvent) -> Result<()> {
        if self
            .events
            .iter()
            .any(|known| known.event.event_id == event.event_id)
        {
            return Ok(());
        }
        let encoded = swarmie_session_contracts::canonical(&SessionEvent {
            session_id: session_id.into(),
            sequence,
            event,
        })?;
        let stored: SessionEvent = serde_json::from_str(&encoded)
            .map_err(|_| crate::state::invalid("appended event has no canonical stored form"))?;
        self.bytes += encoded.len();
        let position = self
            .events
            .partition_point(|known| known.sequence < stored.sequence);
        self.events.insert(position, stored);
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

    /// Reloads committed evidence after the nested model terminal writes its effective request.
    /// Root-owned appends are already durable and are included in the next checked read.
    pub(crate) fn refresh(&mut self) {
        self.history = None;
    }

    /// Appends through this root's proof. A loaded history view is extended with the event at
    /// its receipted sequence, in the canonical form a fresh read of the store returns.
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
        // Only a loaded view keeps a copy; a root that has not read history reads it later.
        let recorded = self.history.is_some().then(|| event.clone());
        let sequence = commit(store, &self.admitted, turn, event, context).await?;
        if let (Some(view), Some(event)) = (&mut self.history, recorded) {
            view.record(&turn.session_id, sequence, event)?;
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

/// Commits one event with its exact immutable identity.
async fn commit(
    store: &SessionStoreRef,
    admitted: &Admitted,
    turn: &TurnContext,
    event: NewEvent,
    context: &Context,
) -> Result<u64> {
    commit_batch(store, admitted, turn, vec![event], context).await
}

/// Commits the model terminal's actual-request and started receipts at their shared
/// pre-effect fence. Validate both requests before any write if the pair needs splitting.
pub(crate) async fn append_pair(
    store: &SessionStoreRef,
    admitted: &Admitted,
    turn: &TurnContext,
    entries: [(&str, &str, serde_json::Value); 2],
    context: &Context,
) -> Result<()> {
    let events = entries
        .into_iter()
        .map(|(suffix, kind, payload)| event(turn, suffix, kind, payload))
        .collect::<Vec<_>>();
    let request = AppendEvents {
        session_id: turn.session_id.clone(),
        batch_id: events[0].event_id.clone(),
        expected_head: None,
        events: events.clone(),
    };
    match swarmie_session_contracts::validate_append(&request) {
        Ok(()) => {
            commit_batch(store, admitted, turn, events, context).await?;
        }
        Err(error) if error.reason == "session_store_too_large" => {
            // Each piece keeps its first stable event id. Refuse an invalid singleton before
            // writing either piece; no independent size/depth policy is recreated here.
            for event in &events {
                swarmie_session_contracts::validate_append(&AppendEvents {
                    session_id: turn.session_id.clone(),
                    batch_id: event.event_id.clone(),
                    expected_head: None,
                    events: vec![event.clone()],
                })?;
            }
            for event in events {
                commit(store, admitted, turn, event, context).await?;
            }
        }
        Err(error) => return Err(error),
    }
    Ok(())
}

async fn commit_batch(
    store: &SessionStoreRef,
    admitted: &Admitted,
    turn: &TurnContext,
    events: Vec<NewEvent>,
    context: &Context,
) -> Result<u64> {
    admitted.check_turn(turn)?;
    let count = events.len() as u64;
    let batch_id = events
        .first()
        .ok_or_else(|| crate::state::invalid("empty session fence"))?
        .event_id
        .clone();
    let receipt = store
        .with_context(context)
        .append(AppendEvents {
            session_id: turn.session_id.clone(),
            batch_id: batch_id.clone(),
            expected_head: None,
            events,
        })
        .await?;
    if receipt.store_id != admitted.destination().session_store_id
        || receipt.session_id != turn.session_id
        || receipt.batch_id != batch_id
        || receipt.first_sequence == 0
        || receipt.first_sequence.checked_add(count - 1) != Some(receipt.last_sequence)
    {
        return Err(crate::state::invalid(
            "session append receipt differs from the routed batch identity or sequence range",
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

    fn new_event(id: &str, payload: serde_json::Value) -> NewEvent {
        NewEvent {
            event_id: id.into(),
            kind: "swarmie.agent.turn-started".into(),
            version: 2,
            correlation: Correlation {
                conversation_id: Some("conversation".into()),
                turn_id: Some("turn".into()),
                ..Default::default()
            },
            occurred_at_ms: None,
            payload,
        }
    }

    fn stored(id: &str, sequence: u64) -> SessionEvent {
        SessionEvent {
            session_id: "session".into(),
            sequence,
            event: new_event(id, serde_json::json!({"value":sequence})),
        }
    }

    fn view(events: Vec<SessionEvent>) -> HistoryView {
        let bytes = events
            .iter()
            .map(|event| swarmie_session_contracts::wire_bytes(event).unwrap())
            .sum();
        HistoryView { events, bytes }
    }

    fn record(history: &mut HistoryView, id: &str, sequence: u64) {
        history
            .record(
                "session",
                sequence,
                new_event(id, serde_json::json!({"value":sequence})),
            )
            .unwrap();
    }

    #[test]
    fn own_appends_extend_the_view_once_in_sequence_order() {
        let mut history = view(vec![stored("first", 1), stored("third", 3)]);
        record(&mut history, "fourth", 4);
        record(&mut history, "second", 2);
        // An exact retry answered with its original receipt is already part of the prefix.
        record(&mut history, "third", 3);
        let events = history.events().unwrap();
        assert_eq!(
            events
                .iter()
                .map(|event| (event.event.event_id.as_str(), event.sequence))
                .collect::<Vec<_>>(),
            [("first", 1), ("second", 2), ("third", 3), ("fourth", 4)]
        );
        assert_eq!(events[1], stored("second", 2));
        let expected: usize = events
            .iter()
            .map(|event| swarmie_session_contracts::wire_bytes(event).unwrap())
            .sum();
        assert_eq!(history.bytes, expected);
        assert_eq!(find(events, "second"), Some(&stored("second", 2).event));
    }

    #[test]
    fn own_appends_are_recorded_as_a_fresh_read_returns_them() {
        let mut history = view(vec![]);
        let payload = serde_json::json!({"whole":2.0,"fraction":0.5,"nested":[3.0,-4.0]});
        history
            .record("session", 7, new_event("numbers", payload.clone()))
            .unwrap();
        let recorded = &history.events().unwrap()[0];
        // The store persists canonical JSON, so a fresh read decodes integral numbers as integers.
        let fresh: SessionEvent = serde_json::from_str(
            &swarmie_session_contracts::canonical(&SessionEvent {
                session_id: "session".into(),
                sequence: 7,
                event: new_event("numbers", payload.clone()),
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(recorded, &fresh);
        assert!(recorded.event.payload["whole"].is_u64());
        assert!(recorded.event.payload["nested"][1].is_i64());
        assert_ne!(recorded.event.payload, payload);
        assert_eq!(
            recorded.event.payload,
            serde_json::json!({"whole":2,"fraction":0.5,"nested":[3,-4]})
        );
        assert_eq!(
            history.bytes,
            swarmie_session_contracts::wire_bytes(recorded).unwrap()
        );
    }

    #[test]
    fn own_appends_cannot_grow_the_view_past_the_read_budget() {
        let mut history = view(
            (1..=MAX_HISTORY_EVENTS as u64)
                .map(|sequence| stored(&sequence.to_string(), sequence))
                .collect(),
        );
        assert!(history.events().is_ok());
        record(&mut history, "over", MAX_HISTORY_EVENTS as u64 + 1);
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
