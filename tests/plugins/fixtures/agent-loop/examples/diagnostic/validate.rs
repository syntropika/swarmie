use std::collections::BTreeSet;

use serde_json::{Value, json};
use swarmie_agent_contracts::{StartTurn, ToolResult, TurnLookup};
use swarmie_session_contracts::{EventQuery, MESSAGE};
use swarmie_task_contracts::{TaskLookup, TaskOutcome, TaskQuery};
use swarmie_task_session_contracts::PublishRequest;
use tembra::{Result, RuntimeError};

use super::host::Harness;

pub fn ensure(valid: bool, message: &str) -> Result<()> {
    if valid {
        Ok(())
    } else {
        Err(RuntimeError::invalid("diagnostic_invariant", message))
    }
}

/// Drains the real publisher, then validates confirmed effects and the exact transcript.
/// This runs after the observed-terminal snapshot and must never be charged to that window.
pub async fn validate(h: &Harness, key: TaskLookup, outcome: &TaskOutcome) -> Result<Value> {
    ensure(
        h.tasks.graph(TaskQuery::default()).await?.records.len() == 1,
        "unexpected durable task count",
    )?;
    let publication = drain(h).await?;
    let mut value = turn(h, &h.request, key, outcome).await?;
    value["publisher_lag_before"] = publication["lag_before"].clone();
    value["publisher_lag_after"] = json!(0);
    value["repeated_publication_empty"] = json!(true);
    Ok(value)
}

/// Checks one exact turn after the shared publisher has drained.
pub async fn turn(
    h: &Harness,
    request: &StartTurn,
    key: TaskLookup,
    outcome: &TaskOutcome,
) -> Result<Value> {
    let terminal = h.storage.get(key.clone()).await?.ok_or_else(|| {
        RuntimeError::invalid("diagnostic_missing", "terminal source task disappeared")
    })?;
    ensure(
        terminal.is_terminal() && terminal.outcome.as_ref() == Some(outcome),
        "observer outcome differs from authoritative terminal record",
    )?;
    ensure(
        terminal.input
            == serde_json::to_value(request)
                .map_err(|error| RuntimeError::invalid("diagnostic_encode", error.to_string()))?,
        "task input changed",
    )?;
    ensure(
        !terminal.cancellation_requested
            && terminal.blocking_reason.is_none()
            && terminal.restarts == 0,
        "unexpected cancellation, blockage or restart",
    )?;
    let TaskOutcome::Completed { value } = outcome else {
        return Err(RuntimeError::invalid(
            "diagnostic_outcome",
            "expected completed tool turn",
        ));
    };
    ensure(
        value["steps"] == 2
            && value["tool_calls"] == 1
            && value["session_id"] == request.session_id
            && value["conversation_id"] == request.conversation_id
            && value["turn_id"] == request.turn_id,
        "incorrect completed turn counts or correlation",
    )?;
    let reconciled = h
        .agent
        .inspect(TurnLookup {
            session_id: request.session_id.clone(),
            conversation_id: request.conversation_id.clone(),
            turn_id: request.turn_id.clone(),
        })
        .await?
        .ok_or_else(|| {
            RuntimeError::invalid("diagnostic_missing", "Agent inspection lost the turn")
        })?;
    ensure(
        reconciled.task == terminal,
        "Agent inspection differs from TaskStorage",
    )?;
    let mut query = EventQuery::new(&request.session_id);
    let mut events = Vec::new();
    loop {
        let page = h.sessions.events(query.clone()).await?;
        events.extend(page.events);
        match page.next {
            Some(cursor) => query.cursor = Some(cursor),
            None => break,
        }
    }
    let facts = events
        .iter()
        .filter(|event| event.event.kind == "swarmie.task-transition")
        .collect::<Vec<_>>();
    ensure(
        facts.len() as u64 == terminal.revision,
        "published facts do not cover each committed task revision",
    )?;
    let mut ids = BTreeSet::new();
    for (index, fact) in facts.iter().enumerate() {
        ensure(
            ids.insert(&fact.event.event_id),
            "duplicate source fact identity",
        )?;
        ensure(
            fact.session_id == request.session_id
                && fact.event.correlation.task_id.as_deref() == Some(key.id.as_str())
                && fact.event.correlation.conversation_id.as_deref()
                    == Some(request.conversation_id.as_str())
                && fact.event.correlation.turn_id.as_deref() == Some(request.turn_id.as_str()),
            "published fact has incorrect destination correlation",
        )?;
        ensure(
            fact.event.payload["source"]["id"]["store_id"] == h.task_store_id
                && fact.event.payload["source"]["revision"] == (index + 1) as u64
                && fact.event.payload["origin"] == "task_storage_outbox",
            "source fact identity/order changed",
        )?;
    }

    let tool_calls = h.tool.calls.borrow();
    let calls = tool_calls
        .iter()
        .filter(|call| call.turn.task_id == key.id)
        .collect::<Vec<_>>();
    let requests = h.model.requests.borrow();
    let requests = requests
        .iter()
        .filter(|model| model.model == request.model)
        .collect::<Vec<_>>();
    let policies = h.policy.calls.borrow();
    let policies = policies
        .iter()
        .filter(|call| call.turn.task_id == key.id)
        .collect::<Vec<_>>();
    ensure(
        requests.len() == 2 && calls.len() == 1 && policies.len() == 1,
        "model/tool invocation count changed",
    )?;
    let call = &calls[0];
    ensure(
        call.tool == "fixture.double" && call.arguments == json!({"value":21}),
        "tool invocation arguments changed",
    )?;
    ensure(
        policies.iter().any(|authorized| authorized == call),
        "tool invocation lacks matching mandatory policy check",
    )?;
    let intents = events
        .iter()
        .filter(|event| event.event.kind == "swarmie.agent.tool-intent")
        .collect::<Vec<_>>();
    ensure(
        intents.len() == 1
            && intents[0].event.payload == json!({"call":call,"replay":"safe","denial":null}),
        "durable tool intent differs from the authorized effect",
    )?;
    let model_intents = events
        .iter()
        .filter(|event| event.event.kind == "swarmie.agent.model-request")
        .collect::<Vec<_>>();
    ensure(
        model_intents.len() == 2,
        "durable model request count differs",
    )?;
    for (intent, observed) in model_intents.iter().zip(&requests) {
        ensure(
            intent.event.payload["request"] == json!(observed),
            "durable model intent differs from the actual provider request",
        )?;
    }
    let model_confirmations = events
        .iter()
        .filter(|event| event.event.kind == "swarmie.agent.model-confirmed")
        .collect::<Vec<_>>();
    ensure(
        model_confirmations.len() == 2,
        "model confirmation count differs",
    )?;
    for ((intent, confirmation), observed) in model_intents
        .iter()
        .zip(&model_confirmations)
        .zip(&requests)
    {
        ensure(
            intent.sequence < confirmation.sequence
                && confirmation.event.payload["result"]["response"]["request_id"]
                    == observed.request_id,
            "model intent/confirmation correlation or order differs",
        )?;
    }
    let tool_confirmation = events
        .iter()
        .find(|event| event.event.kind == "swarmie.agent.tool-confirmed")
        .ok_or_else(|| RuntimeError::invalid("diagnostic_missing", "tool confirmation absent"))?;
    ensure(
        intents[0].sequence < tool_confirmation.sequence,
        "tool confirmation precedes its durable intent",
    )?;
    for event in events
        .iter()
        .filter(|event| event.event.event_id != request.input_event_id)
    {
        ensure(
            event.session_id == request.session_id
                && event.event.correlation.task_id.as_deref() == Some(key.id.as_str())
                && event.event.correlation.turn_id.as_deref() == Some(request.turn_id.as_str()),
            "foreign event in turn session",
        )?;
    }
    let messages = events
        .iter()
        .filter(|event| event.event.kind == MESSAGE)
        .map(|event| event.event.payload.clone())
        .collect::<Vec<_>>();
    let confirmed = events
        .iter()
        .filter(|event| event.event.kind == "swarmie.agent.tool-confirmed")
        .map(|event| event.event.payload.clone())
        .collect::<Vec<_>>();
    let expected = expected_messages(&call.tool_call_id);
    ensure(
        messages == expected,
        "exact ordered transcript differs from user/tool/final42 fixture",
    )?;
    let model_messages = expected[..3]
        .iter()
        .map(|message| {
            json!({
                "role":message["role"],"content":message["content"],
            })
        })
        .collect::<Vec<_>>();
    ensure(
        serde_json::to_value(&requests[1].messages)
            .map_err(|error| RuntimeError::invalid("diagnostic_encode", error.to_string()))?
            == json!(model_messages),
        "second model request did not consume the exact tool-result context",
    )?;
    let final_event = events
        .iter()
        .rev()
        .find(|event| event.event.kind == "swarmie.agent.model-confirmed")
        .ok_or_else(|| {
            RuntimeError::invalid("diagnostic_missing", "final model confirmation absent")
        })?;
    ensure(
        final_event.event.payload["result"]["response"]["message"]
            == json!({"role":"assistant","content":[{"type":"text","text":"42"}]}),
        "final confirmed model response differs from exact final message",
    )?;
    ensure(
        value
            == &json!({
                "session_id":request.session_id,"conversation_id":request.conversation_id,
                "turn_id":request.turn_id,"steps":2,"tool_calls":1,"output_tokens":2,
                "response_event_id":final_event.event.event_id,
            }),
        "completed outcome differs from exact final model-confirmation identity and usage",
    )?;
    let expected_result = ToolResult {
        operation_id: call.operation_id.clone(),
        tool_call_id: call.tool_call_id.clone(),
        output: json!({"value":42}),
        is_error: false,
    };
    ensure(
        confirmed == vec![json!({"operation_id":call.operation_id,"result":expected_result})],
        "tool result is missing, duplicated or not exactly42 with matching identities",
    )?;
    Ok(json!({
        "terminal": terminal, "model_invocations": 2, "tool_invocations": 1,
        "policy_invocations": policies.len(),
        "published_facts": facts.len(),
        "messages": messages, "tool_confirmations": confirmed,
        "exact_transcript": true, "exact_tool_result": true, "exact_durable_intents":true,
        "exact_outcome": true, "tool_result_consumed_by_model": true,
        "distinct_ordered_source_facts": true, "repeated_publication_empty": true,
    }))
}

fn expected_messages(call_id: &str) -> Vec<Value> {
    vec![
        json!({"role":"user","content":[{"type":"text","text":"Compute the result."}],"reply_to":null,"tool_call_id":null}),
        json!({"role":"assistant","content":[{"type":"tool_call","call":{"id":call_id,"name":"fixture.double","arguments":{"value":21},"extensions":[]}}],"reply_to":null,"tool_call_id":null}),
        json!({"role":"tool","content":[{"type":"tool_result","call_id":call_id,"value":{"value":42},"is_error":false}],"reply_to":null,"tool_call_id":call_id}),
        json!({"role":"assistant","content":[{"type":"text","text":"42"}],"reply_to":null,"tool_call_id":null}),
    ]
}

/// Drains the real shared outbox in a separate, bounded window.
pub async fn drain(h: &Harness) -> Result<Value> {
    let lag_before = h.publisher.diagnostics().await?.lag;
    ensure(
        lag_before > 0,
        "expected pending outbox before manual publication",
    )?;
    let mut published = 0_u64;
    let mut drained = false;
    for _ in 0..4096 {
        let receipt = h.publisher.publish(PublishRequest { limit: 256 }).await?;
        ensure(receipt.excluded == 0, "turn source facts were excluded")?;
        published += u64::from(receipt.published);
        if receipt.published == 0 {
            drained = true;
            break;
        }
    }
    ensure(drained, "bounded publication did not drain")?;
    let repeated = h.publisher.publish(PublishRequest { limit: 256 }).await?;
    ensure(
        repeated.published == 0
            && repeated.excluded == 0
            && h.publisher.diagnostics().await?.lag == 0,
        "publisher repeated facts or retained lag",
    )?;
    Ok(json!({"published":published,"lag_before":lag_before,"lag_after":0}))
}
