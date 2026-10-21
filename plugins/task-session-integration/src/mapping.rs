use sha2::{Digest, Sha256};
use swarmie_session_contracts::{AppendEvents, AppendReceipt, Correlation, NewEvent};
use swarmie_task_contracts::TaskOutboxEvent;
use swarmie_task_session_contracts::TaskSessionBinding;
use tembra::Result;

use crate::{admission::invalid, store::canonical};

/// Mapping v1 is frozen: no clock, current task snapshot, head, random ID or admission state.
pub(crate) fn append(binding: &TaskSessionBinding, fact: &TaskOutboxEvent) -> Result<AppendEvents> {
    if binding.routing_version != 1 || !(1..=2).contains(&fact.version) {
        return Err(invalid(
            "binding_version",
            "unsupported fact or routing mapping version",
        ));
    }
    if fact.id.store_id != binding.key.task_store_id
        || fact.task_id != binding.key.task_id
        || fact.parent != binding.parent_task_id
        || fact.task_kind != binding.task.kind
        || fact.definition_version != binding.task.definition_version
    {
        return Err(invalid(
            "binding_fact_mismatch",
            "outbox fact differs from immutable routing identity",
        ));
    }
    let source = canonical(&fact.id)?;
    let identity = |domain: &str| {
        let mut hash = Sha256::new();
        hash.update(domain.as_bytes());
        hash.update([0]);
        hash.update(source.as_bytes());
        format!("task-{}", hex::encode(hash.finalize()))
    };
    Ok(AppendEvents {
        session_id: binding.destination.session_id.clone(),
        batch_id: identity("swarmie.task-session.batch.v1"),
        expected_head: None,
        events: vec![NewEvent {
            event_id: identity("swarmie.task-session.event.v1"),
            kind: "swarmie.task-transition".into(),
            version: 1,
            correlation: Correlation {
                conversation_id: Some(binding.destination.conversation_id.clone()),
                turn_id: Some(binding.destination.turn_id.clone()),
                task_id: Some(fact.task_id.clone()),
                ..Default::default()
            },
            occurred_at_ms: None,
            payload: serde_json::json!({"mapping_version":1,"routing_version":binding.routing_version,"origin":"task_storage_outbox","source":fact,"root_task_id":binding.root_task_id,"parent_task_id":binding.parent_task_id}),
        }],
    })
}

pub(crate) fn receipt(
    binding: &TaskSessionBinding,
    append: &AppendEvents,
    receipt: &AppendReceipt,
) -> Result<()> {
    if receipt.store_id != binding.destination.session_store_id
        || receipt.session_id != append.session_id
        || receipt.batch_id != append.batch_id
        || receipt.first_sequence == 0
        || receipt.first_sequence != receipt.last_sequence
        || receipt.commit_sequence == 0
    {
        return Err(invalid(
            "binding_session_receipt",
            "SessionStore did not confirm the matching single-fact batch",
        ));
    }
    Ok(())
}
