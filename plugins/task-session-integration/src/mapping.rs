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

#[cfg(test)]
mod tests {
    use super::*;
    use swarmie_task_contracts::{TaskOutboxEventId, TaskRecord, TaskSpec};
    use swarmie_task_session_contracts::{BindingAdmission, BindingKey, SessionDestination};

    fn fixture() -> (TaskSessionBinding, TaskOutboxEvent) {
        let task = TaskSpec {
            id: "task".into(),
            kind: "fixture.mapping".into(),
            definition_version: 1,
            input: serde_json::json!({}),
            checkpoint: serde_json::json!({}),
            phase: "start".into(),
        };
        let binding = TaskSessionBinding {
            key: BindingKey {
                task_store_id: "source".into(),
                task_id: task.id.clone(),
            },
            destination: SessionDestination {
                session_store_id: "destination".into(),
                session_id: "session".into(),
                conversation_id: "conversation".into(),
                turn_id: "turn".into(),
            },
            root_task_id: task.id.clone(),
            parent_task_id: None,
            routing_version: 1,
            task: task.clone(),
            admission: BindingAdmission::Pending,
        };
        let fact = TaskOutboxEvent::from_records(
            TaskOutboxEventId {
                store_id: "source".into(),
                sequence: 1,
                ordinal: 0,
            },
            None,
            &TaskRecord::new(task, None),
            1,
            None,
        );
        (binding, fact)
    }

    #[test]
    fn exact_mapping_survives_admission_change_and_distinguishes_source_identity() {
        let (mut binding, mut fact) = fixture();
        let first = append(&binding, &fact).unwrap();
        binding.admission = BindingAdmission::Active;
        assert_eq!(append(&binding, &fact).unwrap(), first);
        assert_ne!(first.batch_id, first.events[0].event_id);
        assert_eq!(first.expected_head, None);
        assert_eq!(first.events[0].occurred_at_ms, None);
        binding.key.task_store_id = "replacement".into();
        fact.id.store_id = "replacement".into();
        assert_ne!(append(&binding, &fact).unwrap().batch_id, first.batch_id);
        fact.version = 3;
        assert_eq!(
            append(&binding, &fact).unwrap_err().reason,
            "binding_version"
        );
    }

    #[test]
    fn only_matching_single_event_receipts_are_confirmed() {
        let (binding, fact) = fixture();
        let request = append(&binding, &fact).unwrap();
        let valid = AppendReceipt {
            store_id: binding.destination.session_store_id.clone(),
            session_id: request.session_id.clone(),
            batch_id: request.batch_id.clone(),
            first_sequence: 7,
            last_sequence: 7,
            commit_sequence: 10,
        };
        receipt(&binding, &request, &valid).unwrap();
        let mut wrong = vec![];
        let mut candidate = valid.clone();
        candidate.store_id = "other".into();
        wrong.push(candidate);
        let mut candidate = valid.clone();
        candidate.session_id = "other".into();
        wrong.push(candidate);
        let mut candidate = valid.clone();
        candidate.batch_id = "other".into();
        wrong.push(candidate);
        let mut candidate = valid.clone();
        candidate.last_sequence = 8;
        wrong.push(candidate);
        let mut candidate = valid.clone();
        candidate.first_sequence = 0;
        wrong.push(candidate);
        let mut candidate = valid;
        candidate.commit_sequence = 0;
        wrong.push(candidate);
        for candidate in wrong {
            assert_eq!(
                receipt(&binding, &request, &candidate).unwrap_err().reason,
                "binding_session_receipt"
            );
        }
    }
}
