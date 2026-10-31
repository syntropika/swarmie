use crate::{ArtifactRef, invalid};
use serde::{Deserialize, Serialize};
use swarmie_conversations::{EntryId, Head, HistoryPoint};
use swarmie_providers::{OperationId, Recovery};

/// Digest of the effective model route, configuration and projection policy.
/// The embedding computes it from the prepared configuration, without credentials.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Configuration(pub [u8; 32]);

/// Qualified execution route selected before dispatch. These are opaque adapter
/// qualification digests, not provider-name heuristics or model capabilities.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Strategy {
    Native { qualification: Configuration },
    Textual { model: Configuration },
}

/// Immutable selection. Appends after `through` do not change this plan.
/// `previous` selects a prior projection; it never contains a transcript copy.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub through: HistoryPoint,
    pub previous: Option<EntryId>,
    pub configuration: Configuration,
    pub strategy: Strategy,
}

/// A validated result stored once in immutable history by the execution owner.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Candidate {
    pub operation: OperationId,
    pub plan: Plan,
    pub artifact: ArtifactRef,
}

/// Active model projection. Canonical history is unaffected by activation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Projection {
    pub operation: OperationId,
    pub plan: Plan,
    pub artifact: ArtifactRef,
}

impl Projection {
    /// Validate imported references; actual ancestry and append-parent membership
    /// must still be checked through the history owner before reading or activation.
    pub fn validate(&self) -> tembra::Result<()> {
        validate_artifact(&self.plan, self.artifact)
    }
}

fn validate_artifact(plan: &Plan, artifact: ArtifactRef) -> tembra::Result<()> {
    if artifact.at().conversation != plan.through.conversation
        || artifact.entry() == plan.through.entry
        || plan.previous == Some(artifact.entry())
    {
        return Err(invalid("artifact does not follow its selected source"));
    }
    Ok(())
}

/// Recognized progress for one logical operation. A dispatched operation always
/// needs recognition after interruption, even if the transport reports cancellation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Phase {
    Planned,
    Dispatched,
    Unknown,
    Candidate {
        artifact: ArtifactRef,
    },
    Activated,
    Superseded {
        artifact: Option<ArtifactRef>,
    },
    Stopped {
        artifact: Option<ArtifactRef>,
    },
    Rejected {
        artifact: ArtifactRef,
        reason: String,
    },
    Failed {
        reason: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Compaction {
    pub operation: OperationId,
    pub plan: Plan,
    pub required: bool,
    pub stop_requested: bool,
    pub phase: Phase,
}

impl Compaction {
    pub fn is_pending(&self) -> bool {
        matches!(
            self.phase,
            Phase::Planned | Phase::Dispatched | Phase::Unknown | Phase::Candidate { .. }
        )
    }
}

/// Serializable bounded progress. Persist through the existing durable owner;
/// assigning this value in memory does not acknowledge a storage commit.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    active: Option<Projection>,
    compaction: Option<Compaction>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Request {
    pub operation: OperationId,
    pub plan: Plan,
    pub required: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Requested {
    Started(OperationId),
    Joined(OperationId),
}

/// Evidence from the history/execution adapter at the activation transaction.
/// The adapter must check membership against the actual current head and fence
/// model/tool dispatch. It must not manufacture this from a client-supplied head.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Activation {
    pub head: Head,
    pub configuration: Configuration,
    pub prefix_is_visible: bool,
    pub execution_is_safe: bool,
    pub request_fits: bool,
}

impl Checkpoint {
    /// Validate imported durable values before admission, recovery or publication.
    /// This checks progress consistency; history membership still belongs to storage.
    pub fn validate(&self) -> tembra::Result<()> {
        if let Some(active) = &self.active {
            active.validate()?;
        }
        if let Some(current) = &self.compaction {
            if let Phase::Failed { reason } | Phase::Rejected { reason, .. } = &current.phase {
                crate::validate_reason(reason)?;
            }
            let artifact = match current.phase {
                Phase::Candidate { artifact } | Phase::Rejected { artifact, .. } => Some(artifact),
                Phase::Stopped { artifact } | Phase::Superseded { artifact } => artifact,
                _ => None,
            };
            if let Some(artifact) = artifact {
                validate_artifact(&current.plan, artifact)?;
            }
            if matches!(current.phase, Phase::Stopped { .. }) && !current.stop_requested {
                return Err(invalid("stopped checkpoint has no Stop intent"));
            }
            if current.phase == Phase::Activated {
                if !self.active.as_ref().is_some_and(|active| {
                    active.operation == current.operation && active.plan == current.plan
                }) {
                    return Err(invalid("activated checkpoint has no matching projection"));
                }
            } else if current.is_pending()
                && self
                    .active_for(current.plan.configuration)
                    .map(|p| p.artifact.entry())
                    != current.plan.previous
            {
                return Err(invalid(
                    "pending checkpoint does not select the current projection",
                ));
            }
        }
        Ok(())
    }

    pub fn active(&self) -> Option<&Projection> {
        self.active.as_ref()
    }

    /// Only return a projection for the configuration that qualified it. A model
    /// change must prepare from canonical history unless compatibility is proven.
    pub fn active_for(&self, configuration: Configuration) -> Option<&Projection> {
        self.active
            .as_ref()
            .filter(|p| p.plan.configuration == configuration)
    }

    pub fn compaction(&self) -> Option<&Compaction> {
        self.compaction.as_ref()
    }

    /// Coalesce manual and automatic requests, including unknown paid work.
    /// A different plan cannot replace an unresolved operation. The caller must
    /// reconcile it first. A required request promotes the existing obligation.
    pub fn request(&mut self, request: Request) -> tembra::Result<Requested> {
        self.validate()?;
        if let Some(current) = &mut self.compaction {
            if current.is_pending() {
                if current.operation == request.operation && current.plan != request.plan {
                    return Err(invalid("operation identity was reused with another plan"));
                }
                if current.plan.configuration != request.plan.configuration
                    || current.plan.strategy != request.plan.strategy
                    || current.plan.previous != request.plan.previous
                    || current.plan.through.conversation != request.plan.through.conversation
                {
                    return Err(invalid("another compaction plan requires recognition"));
                }
                current.required |= request.required;
                return Ok(Requested::Joined(current.operation));
            }
            if current.operation == request.operation {
                return Err(invalid("a terminal operation identity cannot be reused"));
            }
        }
        if self
            .active_for(request.plan.configuration)
            .map(|p| p.artifact.entry())
            != request.plan.previous
        {
            return Err(invalid(
                "compaction plan does not select the current projection",
            ));
        }
        self.compaction = Some(Compaction {
            operation: request.operation,
            plan: request.plan,
            required: request.required,
            stop_requested: false,
            phase: Phase::Planned,
        });
        Ok(Requested::Started(request.operation))
    }

    /// Confirm this transition durably before invoking the adapter. Dispatch and
    /// recovery happen outside retryable hooks and storage transaction closures.
    pub fn dispatch(&mut self, operation: OperationId) -> tembra::Result<()> {
        let current = self.operation_mut(operation)?;
        if current.phase != Phase::Planned || current.stop_requested {
            return Err(invalid("compaction is not admitted for dispatch"));
        }
        current.phase = Phase::Dispatched;
        Ok(())
    }

    /// Lost replies and process interruption never grant another model call.
    pub fn unknown(&mut self, operation: OperationId) -> tembra::Result<()> {
        let current = self.operation_mut(operation)?;
        if !matches!(current.phase, Phase::Dispatched | Phase::Unknown) {
            return Err(invalid("compaction has no uncertain dispatch"));
        }
        current.phase = Phase::Unknown;
        Ok(())
    }

    /// Use the existing provider recognition vocabulary. Only SafeToRetry grants
    /// a new dispatch; Stop fences it even when the provider allows a retry.
    pub fn recognize(
        &mut self,
        operation: OperationId,
        outcome: Recovery<Candidate>,
    ) -> tembra::Result<()> {
        let current = self.operation_mut(operation)?;
        if !matches!(current.phase, Phase::Dispatched | Phase::Unknown) {
            return Err(invalid("compaction is not awaiting recognition"));
        }
        let phase = match outcome {
            Recovery::Unknown => Phase::Unknown,
            Recovery::SafeToRetry if current.stop_requested => Phase::Stopped { artifact: None },
            Recovery::SafeToRetry => Phase::Planned,
            Recovery::Completed(candidate) => {
                if candidate.operation != operation || candidate.plan != current.plan {
                    return Err(invalid("candidate does not match the frozen operation"));
                }
                validate_artifact(&candidate.plan, candidate.artifact)?;
                if current.stop_requested {
                    Phase::Stopped {
                        artifact: Some(candidate.artifact),
                    }
                } else {
                    Phase::Candidate {
                        artifact: candidate.artifact,
                    }
                }
            }
        };
        current.phase = phase;
        Ok(())
    }

    /// Stop preserves recognized and uncertain effects. Observer detach has no
    /// transition here; its lifetime belongs to Tembra's observation resources.
    pub fn stop(&mut self, operation: OperationId) -> tembra::Result<()> {
        let current = self.operation_mut(operation)?;
        current.stop_requested = true;
        current.phase = match current.phase {
            Phase::Planned => Phase::Stopped { artifact: None },
            Phase::Dispatched | Phase::Unknown => Phase::Unknown,
            Phase::Candidate { artifact } => Phase::Stopped {
                artifact: Some(artifact),
            },
            ref terminal => terminal.clone(),
        };
        Ok(())
    }

    /// A confirmed pre-dispatch refusal must not turn uncertain work into failure.
    pub fn refuse(&mut self, operation: OperationId, reason: String) -> tembra::Result<()> {
        crate::validate_reason(&reason)?;
        let current = self.operation_mut(operation)?;
        if current.phase != Phase::Planned {
            return Err(invalid("only planned work can be refused with a reason"));
        }
        current.phase = Phase::Failed { reason };
        Ok(())
    }

    /// Recognize a paid result that cannot be used, retaining its one history
    /// artifact and required obligation. This never grants another dispatch. A
    /// host must recognize the unchanged automatic intent rather than create an
    /// unbounded sequence of replacement operations after rejection.
    pub fn reject(
        &mut self,
        operation: OperationId,
        candidate: Candidate,
        reason: String,
    ) -> tembra::Result<()> {
        crate::validate_reason(&reason)?;
        validate_artifact(&candidate.plan, candidate.artifact)?;
        let current = self.operation_mut(operation)?;
        if candidate.operation != operation
            || candidate.plan != current.plan
            || !matches!(current.phase, Phase::Dispatched | Phase::Unknown)
                && current.phase
                    != (Phase::Candidate {
                        artifact: candidate.artifact,
                    })
        {
            return Err(invalid("rejected result does not match recognized work"));
        }
        current.phase = if current.stop_requested {
            Phase::Stopped {
                artifact: Some(candidate.artifact),
            }
        } else {
            Phase::Rejected {
                artifact: candidate.artifact,
                reason,
            }
        };
        Ok(())
    }

    /// Retire a known, unused plan/result when configuration changes. Recognition
    /// is mandatory first for dispatched work. Retain any paid result reference.
    pub fn supersede(&mut self, operation: OperationId) -> tembra::Result<()> {
        let current = self.operation_mut(operation)?;
        current.phase = match current.phase {
            Phase::Planned => Phase::Superseded { artifact: None },
            Phase::Candidate { artifact } => Phase::Superseded {
                artifact: Some(artifact),
            },
            _ => {
                return Err(invalid(
                    "only a known unused plan or candidate can be superseded",
                ));
            }
        };
        Ok(())
    }

    /// Activate only the frozen prefix. The caller reads the full appended suffix
    /// after this prefix when it prepares the next request. No history is removed.
    pub fn activate(
        &mut self,
        operation: OperationId,
        at: Activation,
    ) -> tembra::Result<Projection> {
        let current = self.operation_mut(operation)?;
        let Phase::Candidate { artifact } = current.phase else {
            return Err(invalid("compaction has no candidate to activate"));
        };
        if current.stop_requested
            || !at.execution_is_safe
            || !at.request_fits
            || !at.prefix_is_visible
            || at.head.conversation != current.plan.through.conversation
            || at.configuration != current.plan.configuration
        {
            return Err(invalid(
                "compaction activation boundary changed or is unsafe",
            ));
        }
        let projection = Projection {
            operation,
            plan: current.plan.clone(),
            artifact,
        };
        current.phase = Phase::Activated;
        self.active = Some(projection.clone());
        Ok(projection)
    }

    fn operation_mut(&mut self, operation: OperationId) -> tembra::Result<&mut Compaction> {
        self.validate()?;
        self.compaction
            .as_mut()
            .filter(|c| c.operation == operation)
            .ok_or_else(|| invalid("compaction operation is not current"))
    }
}
