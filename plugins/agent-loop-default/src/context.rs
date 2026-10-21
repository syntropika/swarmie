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
) -> Result<Vec<Message>> {
    let mut messages = Vec::new();
    for event in events {
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

/// Runs bounded policy selection before allocating source history. Selection sees a frozen
/// durable head, not a materialized transcript. Session cursors preserve that immutable prefix.
pub(crate) async fn select_messages(
    store: &SessionStoreRef,
    hooks: &tembra_hooks::HookManager,
    request: &StartTurn,
    cp: &Checkpoint,
    context: &Context,
) -> Result<Vec<Message>> {
    use swarmie_agent_contracts::{ContextBudget, ContextRange, ContextSelection, select_context};
    use swarmie_session_contracts::{EventQuery, SessionLookup};

    let record = store
        .with_context(context)
        .open(SessionLookup {
            session_id: request.session_id.clone(),
        })
        .await?
        .ok_or_else(|| crate::state::invalid("context session is absent"))?;
    let mut ranges = vec![ContextRange {
        after_sequence: 0,
        through_sequence: cp.input_sequence,
        current_turn_only: false,
    }];
    if record.head > cp.input_sequence {
        ranges.push(ContextRange {
            after_sequence: cp.input_sequence,
            through_sequence: record.head,
            current_turn_only: true,
        });
    }
    let original = ContextSelection {
        turn: cp.turn(request),
        input_sequence: cp.input_sequence,
        through_sequence: record.head,
        ranges,
        budget: ContextBudget::default(),
    };
    let selected = hooks
        .run_waterfall(&select_context()?, original.clone(), context.core())
        .await?;
    validate_selection(&original, &selected)?;
    let mut events = Vec::new();
    let mut bytes = 0;
    for range in &selected.ranges {
        let mut query = EventQuery::new(&request.session_id);
        query.conversation_id = Some(request.conversation_id.clone());
        query.turn_id = range.current_turn_only.then(|| request.turn_id.clone());
        query.after_sequence = range.after_sequence;
        query.through_sequence = Some(range.through_sequence);
        query.limit = selected.budget.max_events.min(128);
        query.max_bytes = selected
            .budget
            .max_bytes
            .min(swarmie_session_contracts::MAX_PAGE_BYTES as u32);
        loop {
            context.check(context.host_id())?;
            let page = store.with_context(context).events(query.clone()).await?;
            for event in page.events {
                if event.session_id != request.session_id
                    || event.event.correlation.conversation_id.as_deref()
                        != Some(&request.conversation_id)
                    || event.sequence <= range.after_sequence
                    || event.sequence > range.through_sequence
                    || (range.current_turn_only
                        && event.event.correlation.turn_id.as_deref() != Some(&request.turn_id))
                {
                    return Err(crate::state::invalid(
                        "selected context escaped its source interval or conversation",
                    ));
                }
                bytes += swarmie_session_contracts::wire_bytes(&event)?;
                if bytes > selected.budget.max_bytes as usize
                    || events.len() >= selected.budget.max_events as usize
                {
                    return Err(tembra::RuntimeError::invalid(
                        "agent_context_limit",
                        "selected context exceeds its explicit materialization budget",
                    ));
                }
                events.push(event);
            }
            let Some(cursor) = page.next else {
                break;
            };
            query.cursor = Some(cursor);
        }
    }
    if !events.iter().any(|event| {
        event.sequence == cp.input_sequence && event.event.event_id == request.input_event_id
    }) {
        return Err(crate::state::invalid(
            "selected context omitted the admitted input",
        ));
    }
    context_messages(&events)
}

fn validate_selection(
    original: &swarmie_agent_contracts::ContextSelection,
    selected: &swarmie_agent_contracts::ContextSelection,
) -> Result<()> {
    use swarmie_agent_contracts::MAX_CONTEXT_RANGES;
    if selected.turn != original.turn
        || selected.input_sequence != original.input_sequence
        || selected.through_sequence != original.through_sequence
        || selected.budget.max_events == 0
        || selected.budget.max_bytes == 0
        || selected.budget.max_events > original.budget.max_events
        || selected.budget.max_bytes > original.budget.max_bytes
        || selected.ranges.is_empty()
        || selected.ranges.len() > MAX_CONTEXT_RANGES
    {
        return Err(crate::state::invalid(
            "context selection changed immutable identity or exceeded its hard budget",
        ));
    }
    let mut after = 0;
    let mut input_included = false;
    for range in &selected.ranges {
        if range.after_sequence < after
            || range.after_sequence >= range.through_sequence
            || range.through_sequence > original.through_sequence
        {
            return Err(crate::state::invalid(
                "context ranges must be ordered, nonoverlapping and within the frozen head",
            ));
        }
        input_included |= !range.current_turn_only
            && range.after_sequence < original.input_sequence
            && original.input_sequence <= range.through_sequence;
        after = range.through_sequence;
    }
    if !input_included {
        return Err(crate::state::invalid(
            "context selection must retain the admitted input",
        ));
    }
    Ok(())
}
