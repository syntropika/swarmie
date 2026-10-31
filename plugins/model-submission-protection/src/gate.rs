use crate::{Coverage, Detector, Inspection, MAX_CONTENT_BYTES};
use hmac::{Hmac, Mac};
use rand::{RngCore, rngs::OsRng};
use serde::Serialize;
use sha2::Sha256;
use std::{cell::RefCell, rc::Rc};
use swarmie_permissions::{ExecutionBinding, HumanDecision, HumanSession, Permission, PluginGrant};
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum Error {
    #[error("the submission requires a human decision")]
    DecisionRequired,
    #[error("submission authority ended or changed")]
    Revoked,
    #[error("invalid or oversized submission")]
    Invalid,
}

/// Contains only safe correlation metadata and a private-keyed, opaque content seal.
#[derive(Clone, Debug, Serialize)]
pub struct DecisionIntent {
    pub operation: Uuid,
    pub decision: Uuid,
    pub execution: swarmie_permissions::ExecutionIdentity,
    pub shell: swarmie_permissions::ShellIdentity,
    pub attempt: Uuid,
    pub provider: String,
    pub destination: String,
    pub policy: Uuid,
    pub permission: Uuid,
    pub seal: Vec<u8>,
    pub inspection: Inspection,
}

/// Correlation for one physical preparation; creating this value grants no authority.
#[derive(Clone, Copy)]
pub struct ModelAttempt {
    pub operation: Uuid,
    pub attempt: Uuid,
}

impl ModelAttempt {
    pub fn new(operation: Uuid) -> Self {
        Self {
            operation,
            attempt: Uuid::new_v4(),
        }
    }
}

struct Policy {
    enabled: bool,
    generation: Uuid,
    detector: Option<Detector>,
    grant: PluginGrant,
    provider: String,
}

/// Node-owned guard. Native embedding installs it at the final provider byte boundary.
#[derive(Clone)]
pub struct ModelGuard {
    policy: Rc<RefCell<Policy>>,
    key: Rc<Zeroizing<[u8; 32]>>,
}

impl ModelGuard {
    pub fn new(provider: String, grant: PluginGrant, detector: Option<Detector>) -> Self {
        let mut key = Zeroizing::new([0; 32]);
        OsRng.fill_bytes(key.as_mut());
        Self {
            policy: Rc::new(RefCell::new(Policy {
                enabled: true,
                generation: Uuid::new_v4(),
                detector,
                grant,
                provider,
            })),
            key: Rc::new(key),
        }
    }

    /// Policy changes require the actual administration action, independent of a vault session.
    pub fn configure(
        &self,
        shell: &HumanSession,
        decision: HumanDecision,
        enabled: bool,
    ) -> Result<(), Error> {
        let mut policy = self.policy.borrow_mut();
        decision
            .consume(
                shell,
                &("model-protection-policy/v1", policy.generation, enabled),
            )
            .map_err(|_| Error::Revoked)?;
        policy.enabled = enabled;
        policy.generation = Uuid::new_v4();
        Ok(())
    }

    pub fn generation(&self) -> Uuid {
        self.policy.borrow().generation
    }

    /// Rechecks a final admission after an asynchronous credential approval.
    pub fn check_version(&self, generation: Uuid, permission: Uuid) -> Result<(), Error> {
        let policy = self.policy.borrow();
        if policy.generation != generation
            || policy.grant.generation() != permission
            || (policy.enabled
                && policy
                    .grant
                    .check(&policy.provider, Permission::InspectModelContent)
                    .is_err())
        {
            return Err(Error::Revoked);
        }
        Ok(())
    }

    /// Preparation performs no I/O. The adapter must finish all provider-owned additions first.
    /// Authentication headers are outside this content body and use separate private custody.
    pub fn prepare(
        &self,
        execution: ExecutionBinding,
        attempt: ModelAttempt,
        provider: String,
        destination: String,
        body: Vec<u8>,
        coverage: Coverage,
    ) -> Result<PreparedSubmission, Error> {
        let body = Zeroizing::new(body);
        if body.len() > MAX_CONTENT_BYTES || !execution.is_live() {
            return Err(Error::Invalid);
        }
        let policy = self.policy.borrow();
        let inspection = if !policy.enabled {
            Inspection::Unprotected
        } else if policy
            .grant
            .check(&policy.provider, Permission::InspectModelContent)
            .is_err()
        {
            Inspection::Unavailable
        } else if let Some(detector) = &policy.detector {
            detector.inspect(&body, coverage)
        } else {
            Inspection::Unavailable
        };
        let mut mac = Hmac::<Sha256>::new_from_slice(self.key.as_ref().as_ref())
            .map_err(|_| Error::Invalid)?;
        mac.update(&body);
        let intent = DecisionIntent {
            decision: Uuid::new_v4(),
            execution: execution.identity().clone(),
            shell: execution.shell().identity().clone(),
            operation: attempt.operation,
            attempt: attempt.attempt,
            provider,
            destination,
            policy: policy.generation,
            permission: policy.grant.generation(),
            seal: mac.finalize().into_bytes().to_vec(),
            inspection,
        };
        Ok(PreparedSubmission {
            guard: self.clone(),
            execution,
            intent,
            body,
            overridden: false,
        })
    }
}

/// Memory-only and non-cloneable. Dropping cancels; restart must reconstruct and rescan.
/// Neither an opaque ID nor a serialized intent can reconstruct this native capability.
pub struct PreparedSubmission {
    guard: ModelGuard,
    execution: ExecutionBinding,
    intent: DecisionIntent,
    body: Zeroizing<Vec<u8>>,
    overridden: bool,
}

impl PreparedSubmission {
    pub fn intent(&self) -> &DecisionIntent {
        &self.intent
    }

    pub fn allow_once(&mut self, decision: HumanDecision) -> Result<(), Error> {
        self.check()?;
        decision
            .consume_owner(self.execution.shell(), &("model-send/v1", &self.intent))
            .map_err(|_| Error::Revoked)?;
        self.overridden = true;
        Ok(())
    }

    fn check(&self) -> Result<(), Error> {
        let policy = self.guard.policy.borrow();
        if !self.execution.is_live()
            || policy.generation != self.intent.policy
            || policy.grant.generation() != self.intent.permission
            || (policy.enabled
                && policy
                    .grant
                    .check(&policy.provider, Permission::InspectModelContent)
                    .is_err())
        {
            return Err(Error::Revoked);
        }
        Ok(())
    }

    /// Consumes the exact preparation at physical send admission. A retry is a new preparation.
    /// Transport may add only credentials/transport headers, never change content or destination.
    pub fn dispatch<T>(self, send: impl FnOnce(&str, &[u8]) -> T) -> Result<T, Error> {
        self.check()?;
        if !self.overridden
            && !matches!(
                self.intent.inspection,
                Inspection::Clean | Inspection::Unprotected
            )
        {
            return Err(Error::DecisionRequired);
        }
        Ok(send(&self.intent.destination, &self.body))
    }
}
