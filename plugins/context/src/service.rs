use crate::{Budget, BudgetDecision, Checkpoint, CompactArguments, CompactionRequest, WorkFence};
use futures::future::LocalBoxFuture;
use serde::{Deserialize, Serialize};
use std::rc::Rc;
use swarmie_commands::{Command, CommandId, Invocation, Target};
use swarmie_providers::OperationId;
use tembra::{Context, Declaration, Orb, Service, State};

pub const SERVICE: &str = "swarmie.context";

/// Register in the command catalog selected for this validated session key.
/// The key identifies an object; it does not establish conversation authority.
pub fn compact_command(key: Vec<String>) -> tembra::Result<Command> {
    let command = Command {
        id: CommandId::new(swarmie_commands::COMPACT)?,
        label: "Compact context".into(),
        description: "Compact the conversation context while retaining its full history".into(),
        aliases: vec!["/compact".into()],
        target: Target {
            service: SERVICE.into(),
            key,
            method: "command".into(),
        },
    };
    command.validate()?;
    Ok(command)
}

/// Safe durable progress projection. Detailed request measurement is unavailable
/// until the adapter has retained and measured the actual prepared request.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub checkpoint: Checkpoint,
    pub fence: Option<WorkFence>,
    pub inspection: Option<Inspection>,
    pub can_compact: bool,
    pub unavailable: Option<String>,
}

/// Measurement of one complete final preparation at a fixed history head. A stale
/// inspection remains useful evidence but does not describe a newer request.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inspection {
    pub at: swarmie_conversations::Head,
    pub prepared: swarmie_providers::PreparedDescription,
    pub decision: BudgetDecision,
    pub stale: bool,
}

impl Inspection {
    pub fn validate(&self) -> tembra::Result<()> {
        let budget = self.decision.budget();
        if &Budget::calculate(&self.prepared, budget.headroom)? != budget {
            return Err(crate::invalid(
                "inspection budget differs from its preparation",
            ));
        }
        match &self.decision {
            BudgetDecision::Ready { budget } if budget.requires_compaction() => {
                return Err(crate::invalid("ready inspection exceeds its budget"));
            }
            BudgetDecision::CompactionRequired { budget, fixed }
            | BudgetDecision::InputTooLarge { budget, fixed }
                if !budget.requires_compaction()
                    || fixed.context != budget.context
                    || fixed.input_limit != budget.input_limit
                    || fixed.output_reserve != budget.output_reserve
                    || fixed.headroom != budget.headroom
                    || fixed.precision != budget.precision
                    || fixed.available != fixed.input_limit.saturating_sub(fixed.used)
                    || fixed.excess != fixed.used.saturating_sub(fixed.input_limit)
                    || matches!(self.decision, BudgetDecision::InputTooLarge { .. })
                        != fixed.requires_compaction() =>
            {
                return Err(crate::invalid("inspection fixed budget is inconsistent"));
            }
            _ => {}
        }
        Ok(())
    }
}

impl Snapshot {
    /// Validate safe imported domain state before publishing it. Provider payloads
    /// and unbounded failure strings do not belong in this projection.
    pub fn validate(&self) -> tembra::Result<()> {
        self.checkpoint.validate()?;
        if self.can_compact && self.fence.is_none() {
            return Err(crate::invalid(
                "available compaction has no admission fence",
            ));
        }
        if let Some(inspection) = &self.inspection {
            inspection.validate()?;
        }
        if let Some(reason) = &self.unavailable {
            crate::validate_reason(reason)?;
        }
        if serde_json::to_vec(self)
            .map_err(|_| crate::invalid("context snapshot could not be encoded"))?
            .len()
            > crate::MAX_SNAPSHOT_BYTES
        {
            return Err(crate::invalid(
                "context snapshot exceeds its observation budget",
            ));
        }
        Ok(())
    }
}

/// Receipt semantics are separate from the operation phase. Admitted and Joined
/// acknowledge a durable association, not completion of a physical model call.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Admission {
    Admitted,
    Joined,
    Recognized,
    Unknown,
    Refused { reason: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub requested: OperationId,
    pub operation: OperationId,
    pub admission: Admission,
    pub snapshot: Snapshot,
}

impl Receipt {
    pub fn validate(&self) -> tembra::Result<()> {
        if let Admission::Refused { reason } = &self.admission {
            crate::validate_reason(reason)?;
        }
        self.snapshot.validate()
    }
}

/// The host supplies one backend bound to the authenticated principal and validated
/// conversation address. Reads must not enter protected execution or resume work.
/// Implementations share the existing admission/Stop guard and storage owner with
/// conversation execution; this interface owns no queue, scheduler or SQLite handle.
pub trait Backend {
    /// Reconstruct the safe projection from domain state, including terminal tasks.
    fn snapshot(&self) -> tembra::Result<Snapshot>;

    /// Acknowledge persisted admission, joining or recognition. Repeated identities
    /// must match their original request/configuration. No paid work is repeated to
    /// obtain a receipt after an unknown reply or storage commit.
    fn compact<'a>(
        &'a self,
        context: &'a Context,
        request: CompactionRequest,
    ) -> LocalBoxFuture<'a, tembra::Result<Receipt>>;

    /// Verify the exact current operation, persist Stop intent, fence admission and
    /// signal its physical context. A stale Stop must not cancel another operation.
    fn stop<'a>(
        &'a self,
        context: &'a Context,
        operation: OperationId,
    ) -> LocalBoxFuture<'a, tembra::Result<Receipt>>;
}

/// Tembra-owned observation of a bound domain backend. Dropping this observation
/// does not stop the backend's admitted work or own its private execution context.
pub struct Session {
    backend: Rc<dyn Backend>,
    state: State<Snapshot>,
}

impl Session {
    pub fn bind(declaration: &Declaration, backend: Rc<dyn Backend>) -> tembra::Result<Self> {
        let initial = backend.snapshot()?;
        initial.validate()?;
        let state = declaration.state(initial, 1, Snapshot::validate)?;
        Ok(Self { backend, state })
    }

    /// The embedding can subscribe to its storage changes and publish with the
    /// physically owning context. Publications are projections, never commit receipts.
    pub fn state(&self) -> State<Snapshot> {
        self.state.clone()
    }

    fn refresh(&self, context: &Context) -> tembra::Result<Snapshot> {
        context.check()?;
        let snapshot = self.backend.snapshot()?;
        snapshot.validate()?;
        self.state.update(context, |_| Ok(snapshot.clone()))?;
        Ok(snapshot)
    }
}

/// The factory validates the actual Declaration address before binding a Session.
/// Neither wire arguments nor command aliases can select another principal.
pub fn orb(create: impl Fn(Declaration) -> tembra::Result<Session> + 'static) -> Orb {
    Orb::new(SERVICE, 1).service(
        Service::new(SERVICE, create)
            .method(
                "command",
                |session, context, (invocation,): (Invocation,)| async move {
                    context.check()?;
                    if invocation.command.as_str() != swarmie_commands::COMPACT {
                        return Err(crate::invalid(
                            "invalid context command or unexpected arguments",
                        ));
                    }
                    let arguments: CompactArguments = serde_json::from_value(invocation.arguments)
                        .map_err(|_| crate::invalid("invalid compaction fence arguments"))?;
                    let receipt = session
                        .backend
                        .compact(
                            &context,
                            CompactionRequest {
                                operation: invocation.operation,
                                fence: arguments.fence,
                            },
                        )
                        .await?;
                    receipt.validate()?;
                    serde_json::to_value(receipt)
                        .map_err(|_| crate::invalid("context receipt could not be encoded"))
                },
            )
            .method("inspect", |session, context, (): ()| async move {
                session.refresh(&context)
            })
            .method(
                "compact",
                |session, context, (request,): (CompactionRequest,)| async move {
                    context.check()?;
                    let receipt = session.backend.compact(&context, request).await?;
                    receipt.validate()?;
                    Ok(receipt)
                },
            )
            .method(
                "stop",
                |session, context, (operation,): (OperationId,)| async move {
                    context.check()?;
                    let receipt = session.backend.stop(&context, operation).await?;
                    receipt.validate()?;
                    Ok(receipt)
                },
            ),
    )
}
