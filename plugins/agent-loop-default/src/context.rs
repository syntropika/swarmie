//! Conversion between immutable Session messages and ephemeral model context.

use swarmie_agent_contracts::{StartTurn, TurnContext};
use swarmie_model_contracts::{ContentPart, Message, Role};
use swarmie_session_contracts::{INSTRUCTIONS, MESSAGE, SessionStoreRef};
use tembra::{Context, Result};

use crate::{
    journal,
    state::{Checkpoint, decode, encode},
};

pub(crate) fn context_messages(
    events: &[swarmie_session_contracts::SessionEvent],
    request: &StartTurn,
    cp: &Checkpoint,
) -> Result<Vec<Message>> {
    let mut messages = Vec::new();
    for event in events {
        if event.sequence > cp.input_sequence
            && event.event.correlation.turn_id.as_deref() != Some(&request.turn_id)
        {
            continue;
        }
        if event.event.version != 1 {
            continue;
        }
        if event.event.kind == MESSAGE {
            let message: swarmie_session_contracts::Message = decode(event.event.payload.clone())?;
            let role = match message.role.as_str() {
                "system" => Role::System,
                "developer" => Role::Developer,
                "user" => Role::User,
                "assistant" => Role::Assistant,
                "tool" => Role::Tool,
                _ => continue,
            };
            let content = if let Some(text) = message.content.as_str() {
                vec![ContentPart::Text { text: text.into() }]
            } else {
                decode(message.content)?
            };
            messages.push(Message { role, content });
        } else if event.event.kind == INSTRUCTIONS {
            let instructions: swarmie_session_contracts::Instructions =
                decode(event.event.payload.clone())?;
            if let Some(text) = instructions.content.as_str() {
                messages.push(Message::text(Role::System, text));
            }
        }
    }
    Ok(messages)
}

pub(crate) async fn append_message(
    store: &SessionStoreRef,
    turn: &TurnContext,
    suffix: &str,
    message: Message,
    tool_call_id: Option<String>,
    context: &Context,
) -> Result<()> {
    let role = match message.role {
        Role::System => "system",
        Role::Developer => "developer",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    };
    journal::append(
        store,
        turn,
        suffix,
        MESSAGE,
        encode(&swarmie_session_contracts::Message {
            role: role.into(),
            content: encode(&message.content)?,
            reply_to: None,
            tool_call_id,
        })?,
        context,
    )
    .await?;
    Ok(())
}
