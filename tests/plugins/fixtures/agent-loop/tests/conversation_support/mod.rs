//! Conversation-scoped helpers retain source event identities across paging and restarts.

use serde_json::json;
use swarmie_agent_contracts::{StartTurn, TurnLimits, TurnLookup, TurnRecord};
use swarmie_model_contracts::{ContentPart, Message, ModelIdentity};
use swarmie_session_contracts::{
    AppendEvents, Correlation, CreateSession, MESSAGE, NewEvent, SessionStoreRef,
};
use swarmie_task_contracts::{DurableTasksRef, TaskLookup};
use tembra::Result;

pub async fn seed(
    store: &SessionStoreRef,
    session: &str,
    conversation: &str,
    id: &str,
    text: &str,
) -> Result<()> {
    store
        .create(CreateSession {
            session_id: session.into(),
            metadata: json!({}),
        })
        .await?;
    store
        .append(AppendEvents {
            session_id: session.into(),
            batch_id: id.into(),
            expected_head: None,
            events: vec![event(conversation, id, text)],
        })
        .await?;
    Ok(())
}

pub fn event(conversation: &str, id: &str, text: &str) -> NewEvent {
    NewEvent {
        event_id: id.into(),
        kind: MESSAGE.into(),
        version: 1,
        correlation: Correlation {
            conversation_id: Some(conversation.into()),
            ..Default::default()
        },
        occurred_at_ms: None,
        payload: json!({"role":"user","content":text,"reply_to":null,"tool_call_id":null}),
    }
}

pub fn request(session: &str, conversation: &str, turn: &str, input: &str) -> StartTurn {
    StartTurn {
        session_id: session.into(),
        conversation_id: conversation.into(),
        turn_id: turn.into(),
        input_event_id: input.into(),
        model: ModelIdentity {
            connection: "fixture".into(),
            model: "scripted".into(),
        },
        limits: TurnLimits::default(),
    }
}

pub fn lookup(session: &str, conversation: &str, turn: &str) -> TurnLookup {
    TurnLookup {
        session_id: session.into(),
        conversation_id: conversation.into(),
        turn_id: turn.into(),
    }
}

pub async fn settle(tasks: &DurableTasksRef, record: &TurnRecord) -> Result<()> {
    tasks
        .wait(TaskLookup {
            id: record.task.id.clone(),
        })
        .await?;
    Ok(())
}

pub fn texts(messages: &[Message]) -> Vec<&str> {
    messages
        .iter()
        .flat_map(|message| &message.content)
        .filter_map(|part| match part {
            ContentPart::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}
