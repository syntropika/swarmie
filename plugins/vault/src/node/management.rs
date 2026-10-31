//! Actual native channel attachment and proof-based resume; no unlock expiry timers.
use super::*;
use crate::verify_signature;
use swarmie_permissions::transport::{ChannelBinding, TransportIdentity};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionConfirmation {
    pub identity: VaultIdentity,
    pub ticket: Uuid,
    pub session: Uuid,
    pub shell: Uuid,
    pub client: Vec<u8>,
    pub node: Vec<u8>,
    pub hosted_generation: u64,
    pub attachment: u64,
    pub exporter: Vec<u8>,
    pub signature: Vec<u8>,
}
impl AdmissionConfirmation {
    fn signing_bytes(&self) -> Result<Vec<u8>> {
        canonical(&(
            "vault-node-admission/v1",
            self.identity,
            self.ticket,
            self.session,
            self.shell,
            &self.client,
            &self.node,
            self.hosted_generation,
            self.attachment,
            &self.exporter,
        ))
    }
    /// Client verifies before sending any private entry/decision over this attachment.
    pub fn verify(
        &self,
        channel: &ChannelBinding,
        admission: &crate::ClientAdmission,
    ) -> Result<()> {
        if !channel.is_live()
            || self.identity != admission.intent.identity
            || self.ticket != admission.ticket
            || self.shell != admission.intent.shell
            || self.client != admission.intent.client
            || self.node.as_slice() != channel.peer()
            || self.client != channel.local_fingerprint()
            || self.hosted_generation != admission.generation
            || self.exporter != channel.exporter()
        {
            return Err(Error::Integrity);
        }
        verify_signature(&self.node, &self.signing_bytes()?, &self.signature)
    }
}

/// Private native attachment capability; never deserialized from IDs or client exporter claims.
#[derive(Clone)]
pub struct ManagementAttachment {
    node: VaultNode,
    session: VaultSession,
    channel: ChannelBinding,
    generation: u64,
}
impl ManagementAttachment {
    pub fn check(&self) -> Result<&VaultSession> {
        if !self.channel.is_live() || self.session.0.attachment.get() != self.generation {
            return Err(Error::Revoked);
        }
        self.node.check(&self.session)?;
        Ok(&self.session)
    }
    /// Only reduces authority; an observed application close does not require Cloud connectivity.
    pub fn close(&self) {
        self.session.close();
    }
    pub fn confirmation(&self) -> Result<AdmissionConfirmation> {
        self.check()?;
        let mut confirmation = AdmissionConfirmation {
            identity: self.node.identity(),
            ticket: self.session.0.admission.ticket,
            session: self.session.0.id,
            shell: self.session.0.admission.intent.shell,
            client: self.session.0.admission.intent.client.clone(),
            node: self.node.0.keys.signing_public(),
            hosted_generation: self.session.0.admission.generation,
            attachment: self.generation,
            exporter: self.channel.exporter().to_vec(),
            signature: vec![],
        };
        confirmation.signature = self.node.0.keys.sign(&confirmation.signing_bytes()?);
        Ok(confirmation)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResumeIntent {
    pub identity: VaultIdentity,
    pub session: Uuid,
    pub ticket: Uuid,
    pub generation: u64,
    pub attachment: u64,
    pub client: Vec<u8>,
    pub node: Vec<u8>,
    pub exporter: Vec<u8>,
    pub challenge: Uuid,
}
pub struct ResumeChallenge {
    node: VaultNode,
    session: VaultSession,
    channel: ChannelBinding,
    intent: ResumeIntent,
}
impl ResumeChallenge {
    pub fn intent(&self) -> &ResumeIntent {
        &self.intent
    }
    /// One challenge is consumed even if a stale attachment races its admission.
    pub fn accept(self, signature: &[u8]) -> Result<ManagementAttachment> {
        self.node.check(&self.session)?;
        if !self.channel.is_live()
            || self.session.0.attachment.get().checked_add(1) != Some(self.intent.attachment)
        {
            return Err(Error::Revoked);
        }
        verify_signature(
            &self.session.0.admission.intent.resume,
            &canonical(&("vault-resume/v1", &self.intent))?,
            signature,
        )?;
        self.session.0.attachment.set(self.intent.attachment);
        Ok(ManagementAttachment {
            node: self.node,
            session: self.session,
            channel: self.channel,
            generation: self.intent.attachment,
        })
    }
}

/// Memory-only client resume authority. An app restart cannot reconstruct this from its keyring.
pub struct ResumeController {
    key: Rc<TransportIdentity>,
    admission: crate::ClientAdmission,
    session: Uuid,
    node: Vec<u8>,
}
impl ResumeController {
    pub fn new(
        key: Rc<TransportIdentity>,
        confirmation: &AdmissionConfirmation,
        channel: &ChannelBinding,
        admission: crate::ClientAdmission,
    ) -> Result<Self> {
        confirmation.verify(channel, &admission)?;
        if key.public().as_slice() != admission.intent.resume {
            return Err(Error::Forbidden);
        }
        Ok(Self {
            key,
            admission,
            session: confirmation.session,
            node: confirmation.node.clone(),
        })
    }
    pub fn sign(&self, intent: &ResumeIntent, channel: &ChannelBinding) -> Result<Vec<u8>> {
        if !channel.is_live()
            || intent.identity != self.admission.intent.identity
            || intent.ticket != self.admission.ticket
            || intent.session != self.session
            || intent.generation != self.admission.generation
            || intent.client != channel.local_fingerprint()
            || intent.client != self.admission.intent.client
            || intent.node.as_slice() != channel.peer()
            || intent.node != self.node
            || intent.exporter != channel.exporter()
        {
            return Err(Error::Forbidden);
        }
        Ok(self.key.sign(&canonical(&("vault-resume/v1", intent))?))
    }
}
impl VaultNode {
    pub fn attach(
        &self,
        session: &VaultSession,
        channel: ChannelBinding,
    ) -> Result<ManagementAttachment> {
        self.check(session)?;
        if !channel.is_live()
            || channel.local().as_slice() != self.0.keys.signing_public()
            || channel.peer_fingerprint() != session.0.admission.intent.client
            || channel.exporter().as_slice() != session.0.admission.intent.exporter
            || session.0.attachment.get() != 1
        {
            return Err(Error::Forbidden);
        }
        Ok(ManagementAttachment {
            node: self.clone(),
            session: session.clone(),
            channel,
            generation: 1,
        })
    }
    pub async fn resume_challenge(
        &self,
        session: &VaultSession,
        channel: ChannelBinding,
    ) -> Result<ResumeChallenge> {
        self.refresh_authority(session).await?;
        if !channel.is_live()
            || channel.local().as_slice() != self.0.keys.signing_public()
            || channel.peer_fingerprint() != session.0.admission.intent.client
        {
            return Err(Error::Forbidden);
        }
        let intent = ResumeIntent {
            identity: self.identity(),
            session: session.0.id,
            ticket: session.0.admission.ticket,
            generation: session.0.admission.generation,
            attachment: session
                .0
                .attachment
                .get()
                .checked_add(1)
                .ok_or(Error::Exhausted)?,
            client: session.0.admission.intent.client.clone(),
            node: self.0.keys.signing_public(),
            exporter: channel.exporter().to_vec(),
            challenge: Uuid::new_v4(),
        };
        Ok(ResumeChallenge {
            node: self.clone(),
            session: session.clone(),
            channel,
            intent,
        })
    }
}
