use swarmie_agent_contracts::TurnContext;
use swarmie_session_contracts::{
    AppendEvents, Correlation, EventQuery, NewEvent, SessionEvent, SessionStoreRef,
};
use swarmie_task_contracts::{TaskStorage, TaskStorageInput};
use swarmie_task_session_contracts::{
    TaskSessionBinding, TaskSessionBindings, TaskSessionBindingsInput,
};
use tembra::{Context, Result, RuntimeError};

use crate::{
    admission::Admission,
    state::{TurnIdentity, stable, task_id},
};

/// Reads one immutable prefix with bounded total admission. The Session provider owns paging.
/// Only current-turn receipts are materialized here; conversation context uses selection first.
pub(crate) async fn history(
    store: &SessionStoreRef,
    turn: &impl TurnIdentity,
    context: &Context,
) -> Result<Vec<SessionEvent>> {
    let binding = guard(
        store,
        &TurnContext {
            session_id: turn.session_id().into(),
            conversation_id: turn.conversation_id().into(),
            turn_id: turn.turn_id().into(),
            task_id: task_id(turn),
            step: 0,
        },
        context,
    )
    .await?;
    let mut query = EventQuery::new(turn.session_id());
    query.conversation_id = Some(turn.conversation_id().into());
    query.turn_id = Some(turn.turn_id().into());
    let mut events = Vec::new();
    let mut bytes = 0;
    loop {
        let page = store.with_context(context).events(query.clone()).await?;
        if page.store_id != binding.destination.session_store_id {
            return Err(crate::state::invalid(
                "session history belongs to a different routed destination",
            ));
        }
        for event in page.events {
            if event.session_id != turn.session_id()
                || event.event.correlation.conversation_id.as_deref()
                    != Some(turn.conversation_id())
                || event.event.correlation.turn_id.as_deref() != Some(turn.turn_id())
            {
                return Err(crate::state::invalid(
                    "session returned an event outside its turn selector",
                ));
            }
            bytes += swarmie_session_contracts::wire_bytes(&event)?;
            if bytes > 4 * 1024 * 1024 || events.len() >= 4096 {
                return Err(RuntimeError::invalid(
                    "agent_history_limit",
                    "session prefix exceeds loop admission budget",
                ));
            }
            events.push(event);
        }
        let Some(cursor) = page.next else {
            break;
        };
        query.cursor = Some(cursor);
    }
    Ok(events)
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
    turn: &TurnContext,
    suffix: &str,
    kind: &str,
    payload: serde_json::Value,
    context: &Context,
) -> Result<String> {
    let event_id = stable(turn, suffix);
    let event = NewEvent {
        event_id: event_id.clone(),
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
    };
    let binding = guard(store, turn, context).await?;
    let receipt = store
        .with_context(context)
        .append(AppendEvents {
            session_id: turn.session_id.clone(),
            batch_id: event_id.clone(),
            expected_head: None,
            events: vec![event],
        })
        .await?;
    if receipt.store_id != binding.destination.session_store_id
        || receipt.session_id != turn.session_id
        || receipt.batch_id != event_id
    {
        return Err(crate::state::invalid(
            "session append receipt differs from the routed batch identity",
        ));
    }
    Ok(event_id)
}

/// Rechecks real selected References immediately before history I/O, including cleanup roots.
/// Store replacement during a call is not atomic with this check; returned source identities
/// are checked separately and never establish a checkpoint for a foreign destination.
pub(crate) async fn guard(
    sessions: &SessionStoreRef,
    turn: &TurnContext,
    context: &Context,
) -> Result<TaskSessionBinding> {
    let bindings = context
        .get(TaskSessionBindings, TaskSessionBindingsInput {})
        .await?;
    let storage = context.get(TaskStorage, TaskStorageInput {}).await?;
    Admission::new(bindings, storage, sessions.clone())
        .require(turn, context)
        .await
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
