//! Private native ceremonies. Only safe receipt digests and trust bundles survive process exit.
use super::*;
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FoundIntent {
    pub identity: VaultIdentity,
    pub device: Uuid,
    pub expected_current: Option<Uuid>,
}

/// Contains no envelope or usable vault key. Persist before issuing the private command.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CeremonyReceipt {
    pub identity: VaultIdentity,
    pub operation: Uuid,
    pub device: Uuid,
    pub payload: Vec<u8>,
}

/// Inner envelopes must never enter disk journals, telemetry or inspectable application State.
#[derive(Serialize)]
pub struct LifecycleCommand {
    operation: Uuid,
    nonce: Uuid,
    device: Uuid,
    action: LifecycleAction,
    signature: Vec<u8>,
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum LifecycleAction {
    Found {
        expected_current: Option<Uuid>,
        founder: Box<SignedEnrollment>,
        envelope: DeviceEnvelope,
        management: Option<Box<SignedEnrollment>>,
    },
    Enroll {
        certificate: Box<SignedEnrollment>,
        envelope: Option<DeviceEnvelope>,
    },
    Rebind {
        identity: VaultIdentity,
        family: Uuid,
    },
    Remove {
        identity: VaultIdentity,
        target: Uuid,
        compromised: bool,
        affected: BTreeSet<Uuid>,
        authority_base: u64,
    },
    Restore {
        identity: VaultIdentity,
        expected_current: Uuid,
    },
}
impl LifecycleCommand {
    pub fn is_security_maintenance(&self) -> bool {
        matches!(
            self.action,
            LifecycleAction::Remove { .. }
                | LifecycleAction::Restore { .. }
                | LifecycleAction::Rebind { .. }
        )
    }
    pub fn receipt(&self) -> Result<CeremonyReceipt> {
        let identity = match &self.action {
            LifecycleAction::Found { founder, .. } => founder.statement.identity,
            LifecycleAction::Enroll { certificate, .. } => certificate.statement.identity,
            LifecycleAction::Rebind { identity, .. }
            | LifecycleAction::Remove { identity, .. }
            | LifecycleAction::Restore { identity, .. } => *identity,
        };
        Ok(CeremonyReceipt {
            identity,
            operation: self.operation,
            device: self.device,
            payload: digest(&canonical(&(
                "vault-command/v1",
                self.operation,
                self.nonce,
                self.device,
                &self.action,
            ))?),
        })
    }

    fn prepare(
        keys: &DeviceKeys,
        device: Uuid,
        identity: VaultIdentity,
        action: LifecycleAction,
    ) -> Result<(Self, CeremonyReceipt)> {
        let mut command = Self {
            operation: Uuid::new_v4(),
            nonce: Uuid::new_v4(),
            device,
            action,
            signature: vec![],
        };
        let bytes = canonical(&(
            "vault-command/v1",
            command.operation,
            command.nonce,
            command.device,
            &command.action,
        ))?;
        command.signature = keys.sign(&bytes);
        let receipt = CeremonyReceipt {
            identity,
            operation: command.operation,
            device: command.device,
            payload: digest(&bytes),
        };
        Ok((command, receipt))
    }
}

/// One explicit first/new-empty founding. The returned bundle is saved to protected storage
/// before submission; a process restart recognizes the receipt rather than creating a new V.
pub fn found(
    shell: &HumanSession,
    decision: HumanDecision,
    intent: FoundIntent,
    keys: DeviceKeys,
) -> Result<(TrustBundle, LifecycleCommand, CeremonyReceipt)> {
    if shell.identity().account != Some(intent.identity.account) {
        return Err(Error::Forbidden);
    }
    decision.consume(shell, &("vault-found/v1", &intent))?;
    let (bundle, command, receipt, _) = build_found(intent, keys, None)?;
    Ok((bundle, command, receipt))
}

/// Purpose-limited first controller for a separately provisioned decryption node. Its public key
/// must be independently compared by the native operator; account login cannot supply that trust.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagementBootstrap {
    pub device: Uuid,
    pub signing: Vec<u8>,
}

pub fn found_with_management(
    shell: &HumanSession,
    decision: HumanDecision,
    intent: FoundIntent,
    keys: DeviceKeys,
    management: ManagementBootstrap,
) -> Result<(TrustBundle, LifecycleCommand, CeremonyReceipt, JoinProposal)> {
    if shell.identity().account != Some(intent.identity.account)
        || management.device == intent.device
        || management.signing.len() != 32
        || management.signing == keys.signing_public()
    {
        return Err(Error::Forbidden);
    }
    decision.consume(
        shell,
        &("vault-found-with-management/v1", &intent, &management),
    )?;
    let (bundle, command, receipt, proposal) = build_found(intent, keys, Some(management))?;
    Ok((bundle, command, receipt, proposal.ok_or(Error::Integrity)?))
}

fn build_found(
    intent: FoundIntent,
    keys: DeviceKeys,
    management: Option<ManagementBootstrap>,
) -> Result<(
    TrustBundle,
    LifecycleCommand,
    CeremonyReceipt,
    Option<JoinProposal>,
)> {
    let key = VaultKey::generate();
    let envelope = DeviceKeys::envelope(
        &key,
        &keys.encryption_public()?,
        intent.identity,
        intent.device,
        &keys.signing_public(),
    )?;
    let founder = keys.enroll(EnrollmentStatement {
        version: 1,
        identity: intent.identity,
        recipient: intent.device,
        signing_key: keys.signing_public(),
        encryption_key: Some(keys.encryption_public()?),
        roles: BTreeSet::from([Role::Decrypt, Role::Manage, Role::EnrollDevices]),
        approver: keys.signing_public(),
        nonce: Uuid::new_v4(),
        envelope_digest: Some(digest(&canonical(&envelope)?)),
        commitment: key.commitment(intent.identity)?,
    })?;
    let pins = TrustPins {
        identity: intent.identity,
        root: keys.signing_public(),
        chain: vec![],
        approver: keys.signing_public(),
        envelope_digest: digest(&canonical(&envelope)?),
        statement_digest: digest(&canonical(&founder)?),
        commitment: key.commitment(intent.identity)?,
    };
    let management_certificate = management
        .as_ref()
        .map(|manager| {
            keys.enroll(EnrollmentStatement {
                version: 1,
                identity: intent.identity,
                recipient: manager.device,
                signing_key: manager.signing.clone(),
                encryption_key: None,
                roles: BTreeSet::from([Role::Manage]),
                approver: keys.signing_public(),
                nonce: Uuid::new_v4(),
                envelope_digest: None,
                commitment: key.commitment(intent.identity)?,
            })
        })
        .transpose()?;
    let management_proposal = management_certificate
        .as_ref()
        .map(|certificate| JoinProposal {
            certificate: certificate.clone(),
            chain: vec![founder.clone()],
            root: keys.signing_public(),
        });
    let (command, receipt) = LifecycleCommand::prepare(
        &keys,
        intent.device,
        intent.identity,
        LifecycleAction::Found {
            expected_current: intent.expected_current,
            founder: Box::new(founder),
            envelope,
            management: management_certificate.clone().map(Box::new),
        },
    )?;
    Ok((
        TrustBundle { keys, pins },
        command,
        receipt,
        management_proposal,
    ))
}

/// Independent out-of-band verification covers the exact root, both keys, roles and envelope.
/// Both the recipient and approver must compare this digest through an independent channel.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JoinProposal {
    pub certificate: SignedEnrollment,
    pub chain: Vec<SignedEnrollment>,
    pub root: Vec<u8>,
}
impl JoinProposal {
    pub fn verification_code(&self) -> Result<String> {
        let bytes = digest(&canonical(&("vault-enrollment-verification/v1", self))?);
        Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
    }
    /// Recipient confirmation is separate from the approver's native confirmation.
    pub fn accept(
        self,
        shell: &HumanSession,
        decision: HumanDecision,
        keys: DeviceKeys,
    ) -> Result<TrustBundle> {
        let statement = &self.certificate.statement;
        if shell.identity().account != Some(statement.identity.account)
            || statement.signing_key != keys.signing_public()
            || (statement.roles.contains(&Role::Decrypt)
                && statement.encryption_key != Some(keys.encryption_public()?))
        {
            return Err(Error::Forbidden);
        }
        decision.consume(
            shell,
            &("vault-accept-enrollment/v1", self.verification_code()?),
        )?;
        let pins = TrustPins {
            identity: statement.identity,
            root: self.root,
            chain: self.chain,
            approver: statement.approver.clone(),
            envelope_digest: statement.envelope_digest.clone().unwrap_or_default(),
            statement_digest: digest(&canonical(&self.certificate)?),
            commitment: statement.commitment.clone(),
        };
        crate::crypto::verify_enrollment_chain(&pins, &self.certificate)?;
        Ok(TrustBundle { keys, pins })
    }
}

impl VaultNode {
    /// Prepare privately after both endpoints' public keys have been obtained independently.
    pub fn propose_enrollment(
        &self,
        session: &VaultSession,
        recipient: Uuid,
        signing: Vec<u8>,
        encryption: Option<Vec<u8>>,
        roles: BTreeSet<Role>,
    ) -> Result<(JoinProposal, Option<DeviceEnvelope>)> {
        let key = self.check(session)?;
        self.require_enrollment_authority()?;
        let envelope = if roles.contains(&Role::Decrypt) {
            Some(DeviceKeys::envelope(
                &key,
                encryption.as_deref().ok_or(Error::Invalid)?,
                self.identity(),
                recipient,
                &signing,
            )?)
        } else {
            None
        };
        let certificate = self.0.keys.enroll(EnrollmentStatement {
            version: 1,
            identity: self.identity(),
            recipient,
            signing_key: signing,
            encryption_key: encryption,
            roles,
            approver: self.0.keys.signing_public(),
            nonce: Uuid::new_v4(),
            envelope_digest: envelope
                .as_ref()
                .map(|e| canonical(e).map(|b| digest(&b)))
                .transpose()?,
            commitment: self.0.pins.commitment.clone(),
        })?;
        let mut chain = self.0.pins.chain.clone();
        // The own immutable certificate is supplied by the authenticated release, not inferred from IDs.
        chain.push(session.0.enrollment.clone());
        let proposal = JoinProposal {
            certificate,
            chain,
            root: self.0.pins.root.clone(),
        };
        crate::crypto::verify_enrollment_chain(
            &TrustPins {
                identity: self.identity(),
                root: proposal.root.clone(),
                chain: proposal.chain.clone(),
                approver: proposal.certificate.statement.approver.clone(),
                envelope_digest: proposal
                    .certificate
                    .statement
                    .envelope_digest
                    .clone()
                    .unwrap_or_default(),
                statement_digest: digest(&canonical(&proposal.certificate)?),
                commitment: self.0.pins.commitment.clone(),
            },
            &proposal.certificate,
        )?;
        Ok((proposal, envelope))
    }
    fn require_enrollment_authority(&self) -> Result<()> {
        let state = self.0.state.borrow();
        let authority = state
            .snapshot
            .authority
            .as_ref()
            .ok_or(Error::Unavailable)?;
        if !authority.devices.iter().any(|d| {
            d.device == self.0.device && !d.removed && d.roles.contains(&Role::EnrollDevices)
        }) {
            return Err(Error::Forbidden);
        }
        Ok(())
    }
    pub fn approve_enrollment(
        &self,
        session: &VaultSession,
        decision: HumanDecision,
        proposal: JoinProposal,
        envelope: Option<DeviceEnvelope>,
    ) -> Result<(LifecycleCommand, CeremonyReceipt)> {
        self.check(session)?;
        self.require_enrollment_authority()?;
        let mut expected_chain = self.0.pins.chain.clone();
        expected_chain.push(session.0.enrollment.clone());
        if proposal.root != self.0.pins.root
            || proposal.chain != expected_chain
            || proposal.certificate.statement.commitment != self.0.pins.commitment
            || proposal.certificate.statement.identity != self.identity()
            || proposal.certificate.statement.approver != self.0.keys.signing_public()
            || proposal.certificate.statement.envelope_digest
                != envelope
                    .as_ref()
                    .map(|e| canonical(e).map(|b| digest(&b)))
                    .transpose()?
        {
            return Err(Error::Integrity);
        }
        decision.consume_owner(
            session.owner(),
            &("vault-approve-enrollment/v1", proposal.verification_code()?),
        )?;
        LifecycleCommand::prepare(
            &self.0.keys,
            self.0.device,
            self.identity(),
            LifecycleAction::Enroll {
                certificate: Box::new(proposal.certificate),
                envelope,
            },
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemovalIntent {
    pub identity: VaultIdentity,
    pub target: Uuid,
    pub compromised: bool,
    pub affected: BTreeSet<Uuid>,
    pub authority_base: u64,
}

/// An existing enrollment proves possession independently of a new account session.
pub fn rebind(
    shell: &HumanSession,
    decision: HumanDecision,
    identity: VaultIdentity,
    device: Uuid,
    keys: &DeviceKeys,
    family: Uuid,
) -> Result<(LifecycleCommand, CeremonyReceipt)> {
    if shell.identity().account != Some(identity.account)
        || shell
            .identity()
            .cloud
            .as_ref()
            .is_none_or(|cloud| cloud.family != family)
    {
        return Err(Error::Forbidden);
    }
    decision.consume(shell, &("vault-rebind/v1", identity, device, family))?;
    LifecycleCommand::prepare(
        keys,
        device,
        identity,
        LifecycleAction::Rebind { identity, family },
    )
}

impl VaultNode {
    /// The exact affected set and authority sequence are displayed before this security action.
    /// Cloud independently recomputes the cascade and refuses stale or incomplete sets.
    pub fn remove_enrollment(
        &self,
        session: &VaultSession,
        decision: HumanDecision,
        intent: RemovalIntent,
    ) -> Result<(LifecycleCommand, CeremonyReceipt)> {
        self.check(session)?;
        self.require_enrollment_authority()?;
        if intent.identity != self.identity()
            || !intent.affected.contains(&intent.target)
            || (!intent.compromised && intent.affected != BTreeSet::from([intent.target]))
            || self.0.state.borrow().snapshot.authority_cursor != intent.authority_base
        {
            return Err(Error::Stale);
        }
        decision.consume_owner(session.owner(), &("vault-remove-enrollment/v1", &intent))?;
        LifecycleCommand::prepare(
            &self.0.keys,
            self.0.device,
            self.identity(),
            LifecycleAction::Remove {
                identity: intent.identity,
                target: intent.target,
                compromised: intent.compromised,
                affected: intent.affected,
                authority_base: intent.authority_base,
            },
        )
    }
    pub fn restore_archived(
        &self,
        session: &VaultSession,
        decision: HumanDecision,
        expected_current: Uuid,
    ) -> Result<(LifecycleCommand, CeremonyReceipt)> {
        self.check(session)?;
        self.require_enrollment_authority()?;
        decision.consume_owner(
            session.owner(),
            &("vault-restore/v1", self.identity(), expected_current),
        )?;
        LifecycleCommand::prepare(
            &self.0.keys,
            self.0.device,
            self.identity(),
            LifecycleAction::Restore {
                identity: self.identity(),
                expected_current,
            },
        )
    }
}

/// Signing-only security maintenance remains available while decryption admission is unavailable.
/// The native operator supplies the exact human confirmation and existing protected enrollment key;
/// Cloud independently checks the current family, role, affected set and authority base.
pub fn remove_enrollment_with_key(
    owner: &HumanSession,
    decision: HumanDecision,
    intent: RemovalIntent,
    device: Uuid,
    keys: &DeviceKeys,
) -> Result<(LifecycleCommand, CeremonyReceipt)> {
    if owner.identity().account != Some(intent.identity.account)
        || !intent.affected.contains(&intent.target)
        || (!intent.compromised && intent.affected != BTreeSet::from([intent.target]))
    {
        return Err(Error::Forbidden);
    }
    decision.consume(owner, &("vault-remove-enrollment/v1", &intent))?;
    LifecycleCommand::prepare(
        keys,
        device,
        intent.identity,
        LifecycleAction::Remove {
            identity: intent.identity,
            target: intent.target,
            compromised: intent.compromised,
            affected: intent.affected,
            authority_base: intent.authority_base,
        },
    )
}

/// Restore uses the old root's signing authority, never a plaintext value or an unlocked vault key.
pub fn restore_archived_with_key(
    owner: &HumanSession,
    decision: HumanDecision,
    identity: VaultIdentity,
    expected_current: Uuid,
    device: Uuid,
    keys: &DeviceKeys,
) -> Result<(LifecycleCommand, CeremonyReceipt)> {
    if owner.identity().account != Some(identity.account) {
        return Err(Error::Forbidden);
    }
    decision.consume(owner, &("vault-restore/v1", identity, expected_current))?;
    LifecycleCommand::prepare(
        keys,
        device,
        identity,
        LifecycleAction::Restore {
            identity,
            expected_current,
        },
    )
}
