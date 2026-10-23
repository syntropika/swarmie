//! Required store-scoped association checks over the selected admission authority.
//!
//! A root is one turn-handler phase invocation, one model-terminal invocation, one cleanup root
//! or one AgentLoop control call. Each root that performs Session I/O confirms its routed
//! association once through [`Admission::confirm`], before its first Session read or append,
//! and lends the resulting [`Admitted`] proof to its Session reads and appends. Source
//! cancellation performs no Session I/O: it uses [`Admission::require_source`], which neither
//! confirms the association nor yields a proof. Within a root nothing that confirmation proves
//! can change: native providers and stores are fixed for the Host lifetime and the root keeps
//! its admitted implementation version, store identities are immutable, bindings only move from
//! Pending to Active without changing their route, and sessions are never deleted. Every
//! Session page and receipt is still compared with the proof's destination. Effects confirm
//! again immediately before they start.

use swarmie_agent_contracts::{StartTurn, TurnContext};
use swarmie_session_contracts::{SessionLookup, SessionStoreRef};
use swarmie_task_contracts::{TaskLookup, TaskRecord, TaskStorageRef};
use swarmie_task_session_contracts::{
    BindingAdmission, BindingDisposition, BindingKey, SessionDestination, TaskSessionBinding,
    TaskSessionBindingsRef,
};
use tembra::{Context, Result, RuntimeError};

use crate::{
    TURN_KIND,
    state::{Checkpoint, TurnIdentity, task_id},
};

/// Holds ordinary Tembra References; there is no local binding cache or admission store.
#[derive(Clone)]
pub(crate) struct Admission {
    pub(crate) bindings: TaskSessionBindingsRef,
    storage: TaskStorageRef,
    sessions: SessionStoreRef,
}

impl Admission {
    pub(crate) fn new(
        bindings: TaskSessionBindingsRef,
        storage: TaskStorageRef,
        sessions: SessionStoreRef,
    ) -> Self {
        Self {
            bindings,
            storage,
            sessions,
        }
    }

    /// Derives identity from the current durable source, never a provider label or State hint.
    pub(crate) async fn key(&self, task: &str, context: &Context) -> Result<BindingKey> {
        let source = self.storage.with_context(context).identity().await?;
        Ok(BindingKey {
            task_store_id: source.store_id,
            task_id: task.into(),
        })
    }

    /// Requires the authoritative task and both selected stores to agree with immutable routing.
    /// A pending, excluded, absent, foreign or unavailable association never grants admission.
    /// The returned proof covers only the calling root; another root confirms again.
    pub(crate) async fn confirm(&self, turn: &TurnContext, context: &Context) -> Result<Admitted> {
        let key = self.key(&turn.task_id, context).await?;
        let binding = self
            .bindings
            .with_context(context)
            .require_confirmed_binding(key.clone())
            .await?;
        validate_route(&binding, &key, turn)?;
        if binding.admission != BindingAdmission::Active {
            return Err(refused("association is not active for effect admission"));
        }
        let session = self
            .sessions
            .with_context(context)
            .open(SessionLookup {
                session_id: turn.session_id.clone(),
            })
            .await?
            .ok_or_else(|| {
                refused("the routed session is absent from the selected session store")
            })?;
        if session.store_id != binding.destination.session_store_id
            || session.session_id != binding.destination.session_id
            || self.key(&turn.task_id, context).await? != binding.key
        {
            return Err(refused(
                "selected durable stores differ from the confirmed association",
            ));
        }
        context.check(context.host_id())?;
        Ok(Admitted {
            binding,
            turn: turn.clone(),
        })
    }

    /// Verifies an existing route against only its selected durable source. This read-only
    /// proof permits logical cancellation while Session is unavailable; it neither confirms
    /// admission nor authorizes Session publication or another external effect.
    pub(crate) async fn require_source(
        &self,
        turn: &TurnContext,
        task: &TaskRecord,
        context: &Context,
    ) -> Result<TaskSessionBinding> {
        let key = self.key(&turn.task_id, context).await?;
        let binding = match self
            .bindings
            .with_context(context)
            .lookup(key.clone())
            .await?
        {
            Some(BindingDisposition::Routed { binding }) => *binding,
            _ => {
                return Err(refused(
                    "source cancellation requires an existing routed association",
                ));
            }
        };
        validate_route(&binding, &key, turn)?;
        validate_source_task(&binding, task, turn)?;
        let actual = self
            .storage
            .with_context(context)
            .get(TaskLookup {
                id: turn.task_id.clone(),
            })
            .await?
            .ok_or_else(|| refused("routed task is absent from the selected durable source"))?;
        // Cancellation targets immutable turn identity, so ordinary phase/revision progress
        // does not invalidate the request. Both snapshots must retain the same admitted input.
        validate_source_task(&binding, &actual, turn)?;
        if self.key(&turn.task_id, context).await? != key {
            return Err(refused(
                "selected durable source differs from the historical association",
            ));
        }
        context.check(context.host_id())?;
        Ok(binding)
    }
}

/// Confirmed routed association of one root for one turn.
///
/// Only [`Admission::confirm`] constructs it, and it is neither `Clone` nor `Default`: a root
/// lends it to the Session I/O it performs and never hands it to another root or keeps it
/// beyond its own call. It carries no grant for an external effect, which confirms again.
pub(crate) struct Admitted {
    binding: TaskSessionBinding,
    turn: TurnContext,
}

impl Admitted {
    /// Destination that every Session page and receipt of this root must match.
    pub(crate) fn destination(&self) -> &SessionDestination {
        &self.binding.destination
    }

    /// Refuses Session I/O for any turn other than the one this root confirmed.
    pub(crate) fn check_turn(&self, turn: &TurnContext) -> Result<()> {
        if turn.session_id != self.turn.session_id
            || turn.conversation_id != self.turn.conversation_id
            || turn.turn_id != self.turn.turn_id
            || turn.task_id != self.turn.task_id
        {
            return Err(refused("admission proof belongs to another turn"));
        }
        Ok(())
    }
}

impl TurnIdentity for Admitted {
    fn session_id(&self) -> &str {
        &self.turn.session_id
    }

    fn conversation_id(&self) -> &str {
        &self.turn.conversation_id
    }

    fn turn_id(&self) -> &str {
        &self.turn.turn_id
    }
}

fn refused(message: &'static str) -> RuntimeError {
    RuntimeError::invalid("agent_binding", message)
}

fn valid_lineage(binding: &TaskSessionBinding) -> bool {
    match &binding.parent_task_id {
        None => binding.root_task_id == binding.key.task_id,
        Some(parent) => {
            !parent.is_empty()
                && parent != &binding.key.task_id
                && !binding.root_task_id.is_empty()
                && binding.root_task_id != binding.key.task_id
        }
    }
}

fn validate_route(
    binding: &TaskSessionBinding,
    key: &BindingKey,
    turn: &TurnContext,
) -> Result<()> {
    if &binding.key != key
        || binding.routing_version != 1
        || !valid_lineage(binding)
        || binding.task.id != turn.task_id
        || binding.task.kind != TURN_KIND
        || binding.task.definition_version != 2
    {
        return Err(refused(
            "routed association does not describe this agent task",
        ));
    }
    let request: StartTurn = serde_json::from_value(binding.task.input.clone())
        .map_err(|_| refused("retained binding submission is not a supported agent turn"))?;
    let checkpoint: Checkpoint = serde_json::from_value(binding.task.checkpoint.clone())
        .map_err(|_| refused("retained binding checkpoint is not a supported agent checkpoint"))?;
    checkpoint
        .validate_identity(&request)
        .map_err(|_| refused("retained binding checkpoint identity differs from its submission"))?;
    if (binding.parent_task_id.is_none() && task_id(&request) != turn.task_id)
        || request.session_id != turn.session_id
        || request.conversation_id != turn.conversation_id
        || request.turn_id != turn.turn_id
        || binding.destination.session_id != turn.session_id
        || binding.destination.conversation_id != turn.conversation_id
        || binding.destination.turn_id != turn.turn_id
    {
        return Err(refused(
            "routed association differs from the persisted turn identity",
        ));
    }
    Ok(())
}

fn validate_source_task(
    binding: &TaskSessionBinding,
    task: &TaskRecord,
    turn: &TurnContext,
) -> Result<()> {
    let spec = &binding.task;
    let request: StartTurn = serde_json::from_value(task.input.clone())
        .map_err(|_| refused("source submission is not a supported agent turn"))?;
    let checkpoint: Checkpoint = serde_json::from_value(task.checkpoint.clone())
        .map_err(|_| refused("source checkpoint is not a supported agent checkpoint"))?;
    checkpoint
        .validate_identity(&request)
        .map_err(|_| refused("source checkpoint identity differs from its submission"))?;
    if task.id != turn.task_id
        || task.id != spec.id
        || task.kind != spec.kind
        || task.definition_version != spec.definition_version
        || canonical(&task.input)? != canonical(&spec.input)?
        || task.parent != binding.parent_task_id
        || task.revision == 0
        || (task.revision == 1
            && (task.phase != spec.phase
                || canonical(&task.checkpoint)? != canonical(&spec.checkpoint)?))
    {
        return Err(refused(
            "source task differs from its retained routed submission",
        ));
    }
    Ok(())
}

fn canonical(value: &impl serde::Serialize) -> Result<String> {
    let portable = tembra::contracts::to_portable_value(value)
        .map_err(|_| refused("source proof cannot be represented as a portable value"))?;
    tembra::contracts::canonical_json(&portable)
        .map_err(|_| refused("source proof cannot be represented as canonical JSON"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarmie_task_contracts::TaskSpec;

    fn turn() -> TurnContext {
        TurnContext {
            turn_id: "turn".into(),
            session_id: "session".into(),
            conversation_id: "conversation".into(),
            task_id: "task".into(),
            step: 0,
        }
    }

    fn admitted() -> Admitted {
        let turn = turn();
        Admitted {
            binding: TaskSessionBinding {
                key: BindingKey {
                    task_store_id: "task-store".into(),
                    task_id: turn.task_id.clone(),
                },
                destination: SessionDestination {
                    session_store_id: "session-store".into(),
                    session_id: turn.session_id.clone(),
                    conversation_id: turn.conversation_id.clone(),
                    turn_id: turn.turn_id.clone(),
                },
                root_task_id: turn.task_id.clone(),
                parent_task_id: None,
                routing_version: 1,
                task: TaskSpec {
                    id: turn.task_id.clone(),
                    kind: TURN_KIND.into(),
                    definition_version: 2,
                    input: serde_json::json!({}),
                    checkpoint: serde_json::json!({}),
                    phase: "start".into(),
                },
                admission: BindingAdmission::Active,
            },
            turn,
        }
    }

    #[test]
    fn a_proof_covers_every_step_of_its_turn_and_no_other_turn() {
        let admitted = admitted();
        assert!(admitted.check_turn(&turn()).is_ok());
        let mut later_step = turn();
        later_step.step = 3;
        assert!(admitted.check_turn(&later_step).is_ok());
        let changes: [fn(&mut TurnContext); 4] = [
            |turn| turn.session_id = "other-session".into(),
            |turn| turn.conversation_id = "other-conversation".into(),
            |turn| turn.turn_id = "other-turn".into(),
            |turn| turn.task_id = "other-task".into(),
        ];
        for change in changes {
            let mut other = turn();
            change(&mut other);
            let error = admitted.check_turn(&other).unwrap_err();
            assert_eq!(error.reason, "agent_binding");
            assert_eq!(error.message, "admission proof belongs to another turn");
        }
    }
}
