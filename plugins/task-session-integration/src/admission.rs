use swarmie_session_contracts::{SessionLookup, SessionStoreRef};
use swarmie_task_contracts::{
    DurableTasksRef, TaskLookup, TaskRecord, TaskStorageRef, validate_identity,
    validate_task_payload_size, validate_task_record_size,
};
use swarmie_task_session_contracts::*;
use tembra::{Context, Resource, Result, RuntimeError};

use crate::{
    Configuration,
    store::{Store, canonical},
};

pub(crate) struct Bindings {
    pub configuration: Configuration,
    pub resource: Resource<Store>,
    pub storage: TaskStorageRef,
    pub sessions: SessionStoreRef,
    pub tasks: DurableTasksRef,
}

pub(crate) fn invalid(reason: &str, message: &str) -> RuntimeError {
    RuntimeError::invalid(reason, message)
}

pub(crate) fn validate_key(key: &BindingKey) -> Result<()> {
    validate_identity(&key.task_store_id)?;
    validate_identity(&key.task_id)
}

/// Asks the live selected TaskStorage provider for its store identity on every call. A label,
/// State hint or earlier answer never substitutes for this read.
pub(crate) async fn source_identity(
    storage: &TaskStorageRef,
    context: &Context,
    expected: &str,
) -> Result<()> {
    if storage.with_context(context).identity().await?.store_id != expected {
        return Err(invalid(
            "binding_store_mismatch",
            "selected TaskStorage identity differs from the admitted source",
        ));
    }
    Ok(())
}

/// Asks the live selected SessionStore provider for its store identity on every call. A label,
/// State hint or earlier answer never substitutes for this read.
pub(crate) async fn destination_identity(
    sessions: &SessionStoreRef,
    context: &Context,
    expected: &str,
) -> Result<()> {
    if sessions.with_context(context).identity().await?.store_id != expected {
        return Err(invalid(
            "binding_store_mismatch",
            "selected SessionStore identity differs from the admitted destination",
        ));
    }
    Ok(())
}

impl Bindings {
    pub fn lookup(&self, key: &BindingKey) -> Result<Option<BindingDisposition>> {
        validate_key(key)?;
        self.resource.value()?.lookup(key)
    }

    pub fn route(&self, key: &BindingKey) -> Result<TaskSessionBinding> {
        match self.lookup(key)? {
            Some(BindingDisposition::Routed { binding }) => Ok(*binding),
            Some(BindingDisposition::Excluded { .. }) => Err(invalid(
                "binding_excluded",
                "excluded work cannot obtain effect admission",
            )),
            None => Err(invalid(
                "binding_missing",
                "no durable routing intent exists for this store/task identity",
            )),
        }
    }

    async fn selected(&self, key: &BindingKey, context: &Context) -> Result<()> {
        validate_key(key)?;
        if key.task_store_id != self.configuration.task_store_id {
            return Err(invalid(
                "binding_store_mismatch",
                "binding belongs to another configured source",
            ));
        }
        source_identity(&self.storage, context, &key.task_store_id).await
    }

    async fn destination(&self, destination: &SessionDestination, context: &Context) -> Result<()> {
        for id in [
            &destination.session_store_id,
            &destination.session_id,
            &destination.conversation_id,
            &destination.turn_id,
        ] {
            swarmie_session_contracts::identity(id)?;
        }
        if destination.session_store_id != self.configuration.session_store_id {
            return Err(invalid(
                "binding_store_mismatch",
                "binding belongs to another configured destination",
            ));
        }
        destination_identity(&self.sessions, context, &destination.session_store_id).await?;
        let session = self
            .sessions
            .with_context(context)
            .open(SessionLookup {
                session_id: destination.session_id.clone(),
            })
            .await?
            .ok_or_else(|| {
                invalid(
                    "binding_session_missing",
                    "destination session must already exist",
                )
            })?;
        if session.store_id != destination.session_store_id
            || session.session_id != destination.session_id
        {
            return Err(invalid(
                "binding_store_mismatch",
                "session receipt does not identify the admitted destination",
            ));
        }
        Ok(())
    }

    async fn prepare_route(
        &self,
        binding: TaskSessionBinding,
        context: &Context,
    ) -> Result<TaskSessionBinding> {
        self.selected(&binding.key, context).await?;
        self.destination(&binding.destination, context).await?;
        validate_identity(&binding.task.kind)?;
        validate_identity(&binding.task.phase)?;
        if binding.routing_version != 1 || binding.task.definition_version == 0 {
            return Err(invalid(
                "binding_version",
                "only routing v1 and positive task definitions are supported",
            ));
        }
        validate_task_record_size(&TaskRecord::new(
            binding.task.clone(),
            binding.parent_task_id.clone(),
        ))?;
        validate_task_payload_size(&BindingDisposition::Routed {
            binding: Box::new(binding.clone()),
        })?;
        if self.lookup(&binding.key)?.is_none()
            && self
                .storage
                .with_context(context)
                .get(TaskLookup {
                    id: binding.key.task_id.clone(),
                })
                .await?
                .is_some()
        {
            return Err(invalid(
                "binding_admission_order",
                "an initial routing intent must precede task creation; restore lost intent from authoritative evidence",
            ));
        }
        let key = binding.key.clone();
        match self.resource.value()?.insert(
            &key,
            BindingDisposition::Routed {
                binding: Box::new(binding),
            },
        )? {
            BindingDisposition::Routed { binding } => Ok(*binding),
            BindingDisposition::Excluded { .. } => Err(crate::store::conflict()),
        }
    }

    pub async fn prepare(
        &self,
        request: PrepareBinding,
        context: &Context,
    ) -> Result<TaskSessionBinding> {
        self.prepare_route(
            TaskSessionBinding {
                key: BindingKey {
                    task_store_id: request.task_store_id,
                    task_id: request.task.id.clone(),
                },
                destination: request.destination,
                root_task_id: request.task.id.clone(),
                parent_task_id: None,
                routing_version: request.routing_version,
                task: request.task,
                admission: BindingAdmission::Pending,
            },
            context,
        )
        .await
    }

    pub async fn prepare_child(
        &self,
        request: PrepareChildBinding,
        context: &Context,
    ) -> Result<TaskSessionBinding> {
        let parent = self.confirm(request.parent, context).await?;
        if request.task.id == parent.key.task_id || request.task.id == parent.root_task_id {
            return Err(invalid(
                "binding_lineage",
                "a child must have a distinct stable identity",
            ));
        }
        let mut destination = parent.destination;
        if let Some(conversation) = request.conversation_id {
            destination.conversation_id = conversation;
        }
        self.prepare_route(
            TaskSessionBinding {
                key: BindingKey {
                    task_store_id: parent.key.task_store_id,
                    task_id: request.task.id.clone(),
                },
                destination,
                root_task_id: parent.root_task_id,
                parent_task_id: Some(parent.key.task_id),
                routing_version: parent.routing_version,
                task: request.task,
                admission: BindingAdmission::Pending,
            },
            context,
        )
        .await
    }

    pub async fn admit(&self, key: BindingKey, context: &Context) -> Result<TaskSessionBinding> {
        let binding = self.route(&key)?;
        self.selected(&key, context).await?;
        self.destination(&binding.destination, context).await?;
        let existing = self
            .storage
            .with_context(context)
            .get(TaskLookup {
                id: key.task_id.clone(),
            })
            .await?;
        if existing.is_none() {
            if binding.parent_task_id.is_some() {
                return Err(invalid(
                    "binding_child_admission",
                    "children are created by their owner's Wait transition, never root create",
                ));
            }
            if let Err(error) = self
                .tasks
                .with_context(context)
                .create(binding.task.clone())
                .await
            {
                // A failed reply can follow a committed create. Never delete or exclude intent.
                if self
                    .storage
                    .with_context(context)
                    .get(TaskLookup {
                        id: key.task_id.clone(),
                    })
                    .await?
                    .is_none()
                {
                    return Err(error);
                }
            }
        }
        self.confirm(key, context).await
    }

    pub async fn confirm(&self, key: BindingKey, context: &Context) -> Result<TaskSessionBinding> {
        let binding = self.route(&key)?;
        self.selected(&key, context).await?;
        self.destination(&binding.destination, context).await?;
        let task = self
            .storage
            .with_context(context)
            .get(TaskLookup {
                id: key.task_id.clone(),
            })
            .await?
            .ok_or_else(|| {
                invalid(
                    "binding_task_missing",
                    "routing is pending; no matching task commit is confirmed",
                )
            })?;
        check_task(&binding, &task)?;
        if let Some(parent) = &binding.parent_task_id {
            let parent = self.route(&BindingKey {
                task_store_id: key.task_store_id.clone(),
                task_id: parent.clone(),
            })?;
            if parent.root_task_id != binding.root_task_id
                || parent.destination.session_store_id != binding.destination.session_store_id
                || parent.destination.session_id != binding.destination.session_id
                || parent.destination.turn_id != binding.destination.turn_id
            {
                return Err(invalid(
                    "binding_lineage",
                    "child lineage differs from its immutable parent route",
                ));
            }
        } else if binding.root_task_id != key.task_id {
            return Err(invalid(
                "binding_lineage",
                "root identity differs from its source task",
            ));
        }
        if binding.admission == BindingAdmission::Active {
            Ok(binding)
        } else {
            self.resource.value()?.activate(binding)
        }
    }

    pub async fn exclude(
        &self,
        request: ExcludedBinding,
        context: &Context,
    ) -> Result<ExcludedBinding> {
        self.selected(&request.key, context).await?;
        if request.reason.is_empty()
            || request.reason.len() > 512
            || request.reason.chars().any(char::is_control)
        {
            return Err(invalid(
                "binding_exclusion",
                "exclusion requires 1..=512 bytes of explicit policy reason without controls",
            ));
        }
        match self.resource.value()?.insert(
            &request.key,
            BindingDisposition::Excluded {
                exclusion: request.clone(),
            },
        )? {
            BindingDisposition::Excluded { exclusion } => Ok(exclusion),
            BindingDisposition::Routed { .. } => Err(crate::store::conflict()),
        }
    }
}

fn check_task(binding: &TaskSessionBinding, task: &TaskRecord) -> Result<()> {
    let spec = &binding.task;
    if binding.routing_version != 1 {
        return Err(invalid(
            "binding_version",
            "unsupported immutable routing version",
        ));
    }
    if task.id != spec.id
        || binding.key.task_id != spec.id
        || task.kind != spec.kind
        || task.definition_version != spec.definition_version
        || canonical(&task.input)? != canonical(&spec.input)?
        || task.parent != binding.parent_task_id
        || task.revision == 0
        || (task.revision == 1
            && (task.phase != spec.phase
                || canonical(&task.checkpoint)? != canonical(&spec.checkpoint)?))
    {
        return Err(invalid(
            "binding_task_mismatch",
            "committed task does not match the immutable admitted identity and submission",
        ));
    }
    Ok(())
}
