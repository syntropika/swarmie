mod authority;
pub use authority::AccountLifetime;
mod capture_management;
mod captures;
pub use capture_management::*;
mod lifecycle;
#[cfg(feature = "cloud-client")]
mod management;
mod mutations;
mod recovery;
pub use lifecycle::*;
#[cfg(feature = "cloud-client")]
pub use management::*;
pub use recovery::*;

pub use captures::*;

use crate::{
    Approval, Authority, DeviceEnvelope, DeviceKeys, EncryptedRecord, Error, ExecutionBinding,
    HumanDecision, HumanSession, OwnerBinding, Permission, PluginGrant, RecordHeader, RecordKind,
    Result, SecretMetadata, SecretReference, SecretValue, SignedEnrollment, TrustPins, UseGrant,
    UseIntent, VaultEligibility, VaultIdentity, VaultKey, VaultSnapshot, VaultStorage, canonical,
    storage::advance,
};
use futures::future::LocalBoxFuture;
use serde::{Deserialize, Serialize};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::{Rc, Weak},
};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultMode {
    Current,
    ArchivedReadable,
    Exposed,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseRequest {
    pub identity: VaultIdentity,
    pub device: Uuid,
    pub client: Vec<u8>,
    pub family: Uuid,
    pub shell_installation: Uuid,
    pub shell_generation: Uuid,
    pub signing_key: Vec<u8>,
    pub nonce: Uuid,
    pub signature: Vec<u8>,
}

pub struct ReleasedEnvelope {
    pub enrollment: SignedEnrollment,
    pub envelope: DeviceEnvelope,
    pub admission: crate::ClientAdmission,
}

/// The concrete transport authenticates each account/session and performs the qualified
/// client-ticket/channel exchange. No offline fallback can manufacture a returned envelope.
pub trait VaultCloud {
    fn current_authority(
        &self,
        identity: VaultIdentity,
    ) -> LocalBoxFuture<'_, Result<crate::AuthorityCheck>>;
    fn release<'a>(
        &'a self,
        request: ReleaseRequest,
    ) -> LocalBoxFuture<'a, Result<ReleasedEnvelope>>;
    fn mode<'a>(&'a self, identity: VaultIdentity) -> LocalBoxFuture<'a, Result<VaultMode>>;
    fn commit<'a>(
        &'a self,
        command: crate::RecordCommand,
    ) -> LocalBoxFuture<'a, Result<crate::CommitOutcome>>;
    fn recognize<'a>(
        &'a self,
        command: crate::RecordCommand,
    ) -> LocalBoxFuture<'a, Result<crate::CommitOutcome>>;
    fn fence(
        &self,
        _command: crate::RecordCommand,
    ) -> LocalBoxFuture<'_, Result<crate::CommitOutcome>> {
        Box::pin(async { Err(Error::Unsupported) })
    }
    fn changes<'a>(
        &'a self,
        identity: VaultIdentity,
        after: u64,
    ) -> LocalBoxFuture<'a, Result<crate::CloudRecordPage>>;
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MutationIntent {
    pub reference: SecretReference,
    pub expected: Option<Uuid>,
    pub metadata: SecretMetadata,
    pub kind: RecordKind,
    pub prepared_entry: Uuid,
}

/// Private prepared entry binds the value to one owner-issued intent; it is never serialized.
pub struct PreparedMutation {
    owner: OwnerBinding,
    intent: MutationIntent,
    candidate: EncryptedRecord,
}

impl PreparedMutation {
    pub fn intent(&self) -> &MutationIntent {
        &self.intent
    }
}

#[derive(Clone)]
pub struct VaultNode(Rc<Node>);

struct Node {
    authority: Authority,
    identity: VaultIdentity,
    device: Uuid,
    keys: Rc<DeviceKeys>,
    pins: TrustPins,
    storage: Rc<dyn VaultStorage>,
    cloud: RefCell<Rc<dyn VaultCloud>>,
    state: Rc<RefCell<NodeState>>,
}

struct NodeState {
    key: Option<Rc<VaultKey>>,
    snapshot: VaultSnapshot,
    sessions: BTreeMap<Uuid, Weak<Session>>,
    grants: Vec<(Uuid, UseGrant)>,
    capture_owners: BTreeMap<Uuid, Weak<Session>>,
    capture_lifetimes: BTreeMap<Uuid, Weak<captures::CaptureLifetime>>,
    authority_failed: bool,
    records_failed: bool,
    ended: bool,
    account_generation: Uuid,
}

struct Session {
    admission: crate::ClientAdmission,
    enrollment: SignedEnrollment,
    #[cfg(feature = "cloud-client")]
    attachment: Cell<u64>,
    id: Uuid,
    owner: OwnerBinding,
    state: Weak<RefCell<NodeState>>,
    live: Cell<bool>,
}

impl Session {
    fn close(&self) {
        self.live.set(false);
        if let Some(state) = self.state.upgrade() {
            let mut state = state.borrow_mut();
            state.sessions.remove(&self.id);
            state.grants.retain(|(id, _)| *id != self.id);
            if state.sessions.is_empty() {
                state.key = None;
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.close();
    }
}

/// Private owner capability. A public session ID never reconstructs it.
#[derive(Clone)]
pub struct VaultSession(Rc<Session>);

impl VaultSession {
    pub fn close(&self) {
        self.0.close();
    }
    pub fn admission(&self) -> &crate::ClientAdmission {
        &self.0.admission
    }
    pub fn owner(&self) -> &OwnerBinding {
        &self.0.owner
    }
}

impl VaultNode {
    pub fn open(
        identity: VaultIdentity,
        device: Uuid,
        keys: impl Into<Rc<DeviceKeys>>,
        pins: TrustPins,
        storage: Rc<dyn VaultStorage>,
        cloud: Rc<dyn VaultCloud>,
        authority: Authority,
    ) -> Result<Self> {
        if pins.identity != identity {
            return Err(Error::Integrity);
        }
        let keys = keys.into();
        let mut snapshot = storage.load(identity)?;
        snapshot.verify(identity, device, &keys)?;
        let mut recovered = snapshot.clone();
        for capture in &mut recovered.captures {
            capture.recovered()?;
        }
        if recovered
            .captures
            .iter()
            .zip(&snapshot.captures)
            .any(|(a, b)| a.state != b.state)
        {
            let expected = advance(&mut recovered)?;
            recovered.sign(identity, device, &keys)?;
            storage.compare_exchange(identity, expected, &recovered)?;
            snapshot = recovered;
        }
        if snapshot.cloud_cursor != snapshot.records.len() as u64 {
            return Err(Error::Integrity);
        }
        Ok(Self(Rc::new(Node {
            authority,
            identity,
            device,
            keys,
            pins,
            storage,
            cloud: RefCell::new(cloud),
            state: Rc::new(RefCell::new(NodeState {
                key: None,
                snapshot,
                sessions: BTreeMap::new(),
                grants: vec![],
                capture_owners: BTreeMap::new(),
                capture_lifetimes: BTreeMap::new(),
                authority_failed: false,
                records_failed: false,
                ended: false,
                account_generation: Uuid::new_v4(),
            })),
        })))
    }

    fn cloud(&self) -> Rc<dyn VaultCloud> {
        self.0.cloud.borrow().clone()
    }
    /// Native custodian starts a freshly authenticated account generation on this same storage
    /// owner. Existing physical capture sinks retain their encrypted journal owner; old releases
    /// and sessions cannot revive authority. No decryption occurs before the new Cloud admission.
    pub fn begin_account(&self, cloud: Rc<dyn VaultCloud>) {
        self.close_all();
        *self.0.cloud.borrow_mut() = cloud;
        let mut state = self.0.state.borrow_mut();
        state.account_generation = Uuid::new_v4();
        state.ended = false;
        state.authority_failed = false;
    }
    pub fn identity(&self) -> VaultIdentity {
        self.0.identity
    }

    fn check(&self, session: &VaultSession) -> Result<Rc<VaultKey>> {
        let key = self.check_admission(session)?;
        if self.0.state.borrow().records_failed {
            return Err(Error::Unavailable);
        }
        Ok(key)
    }

    /// Recovery may inspect encrypted pages after a record refusal, but cannot release a value.
    fn check_admission(&self, session: &VaultSession) -> Result<Rc<VaultKey>> {
        self.0.storage.health()?;
        let state = self.0.state.borrow();
        if state.ended
            || state.authority_failed
            || !session.0.live.get()
            || !session.owner().is_live()
            || session.owner().identity().account != Some(self.0.identity.account)
            || !state
                .sessions
                .get(&session.0.id)
                .and_then(Weak::upgrade)
                .is_some_and(|s| Rc::ptr_eq(&s, &session.0))
        {
            return Err(Error::Revoked);
        }
        state.key.clone().ok_or(Error::Locked)
    }

    pub async fn unlock(&self, owner: &HumanSession) -> Result<VaultSession> {
        self.0.authority.check(owner)?;
        if self.0.state.borrow().ended {
            return Err(Error::Revoked);
        }
        if !owner.is_live() || owner.identity().account != Some(self.0.identity.account) {
            return Err(Error::Forbidden);
        }
        let account_generation = self.0.state.borrow().account_generation;
        let client = owner.identity().cloud.as_ref().ok_or(Error::Forbidden)?;
        let mut request = ReleaseRequest {
            identity: self.0.identity,
            device: self.0.device,
            client: client.client.clone(),
            family: client.family,
            shell_installation: owner.identity().installation,
            shell_generation: owner.identity().generation,
            signing_key: self.0.keys.signing_public(),
            nonce: Uuid::new_v4(),
            signature: vec![],
        };
        request.signature = self.0.keys.sign(&canonical(&(
            "vault-release/v1",
            request.identity,
            request.device,
            &request.client,
            request.family,
            request.shell_installation,
            request.shell_generation,
            &request.signing_key,
            request.nonce,
        ))?);
        let nonce = request.nonce;
        let released = self.cloud().release(request).await?;
        let admission = &released.admission;
        if admission.intent.identity != self.0.identity
            || admission.intent.node != self.0.device
            || admission.intent.client != client.client
            || admission.family != client.family
            || admission.intent.shell != owner.identity().generation
            || admission.intent.nonce != nonce
            || admission.intent.resume.len() != 32
            || admission.intent.exporter.len() != 32
            || admission.generation == 0
        {
            return Err(Error::Integrity);
        }
        if !owner.is_live() || self.0.state.borrow().account_generation != account_generation {
            return Err(Error::Revoked);
        }
        if released.enrollment.statement.recipient != self.0.device {
            return Err(Error::Integrity);
        }
        self.reconcile(Some(&released.admission)).await?;
        if self.0.state.borrow().ended
            || !owner.is_live()
            || self.0.state.borrow().account_generation != account_generation
        {
            return Err(Error::Revoked);
        }
        let key = self
            .0
            .keys
            .unlock(&self.0.pins, &released.enrollment, &released.envelope)?;
        {
            let state = self.0.state.borrow();
            let mut verified = VaultSnapshot::default();
            for record in &state.snapshot.records {
                apply_record(&key, self.0.identity, &mut verified, record.clone())?;
            }
            for command in &state.snapshot.pending {
                key.verify(command.record())?;
                crate::verify_signature(
                    &self.0.keys.signing_public(),
                    &command.signing_bytes()?,
                    &command.signature,
                )?;
            }
            for capture in &state.snapshot.captures {
                if let Some(candidate) = &capture.candidate {
                    key.verify(candidate)?;
                }
            }
        }
        let session = Rc::new(Session {
            #[cfg(feature = "cloud-client")]
            attachment: Cell::new(1),
            enrollment: released.enrollment.clone(),
            admission: released.admission,
            id: Uuid::new_v4(),
            owner: owner.binding(),
            state: Rc::downgrade(&self.0.state),
            live: Cell::new(true),
        });
        let weak = Rc::downgrade(&session);
        owner.on_close(Box::new(move || {
            if let Some(session) = weak.upgrade() {
                session.close();
            }
        }));
        {
            let mut state = self.0.state.borrow_mut();
            state.key = Some(Rc::new(key));
            state.sessions.insert(session.id, Rc::downgrade(&session));
        }
        let session = VaultSession(session);
        self.synchronize(&session).await?;
        Ok(session)
    }

    pub fn is_locked(&self) -> bool {
        self.0.state.borrow().key.is_none()
    }

    fn latest(&self, reference: SecretReference) -> Result<EncryptedRecord> {
        if reference.account != self.0.identity.account || reference.vault != self.0.identity.vault
        {
            return Err(Error::Forbidden);
        }
        self.0
            .state
            .borrow()
            .snapshot
            .records
            .iter()
            .rev()
            .find(|r| r.header.secret == reference.secret)
            .cloned()
            .ok_or(Error::NotFound)
    }

    pub fn discover(
        &self,
        session: &VaultSession,
        binding: &ExecutionBinding,
        plugin: &PluginGrant,
    ) -> Result<Vec<(SecretReference, SecretMetadata)>> {
        self.check(session)?;
        plugin.check(plugin.provider(), Permission::Discover)?;
        if !session.owner().same_owner(binding.shell()) {
            return Err(Error::Forbidden);
        }
        let mut latest = BTreeMap::new();
        for r in &self.0.state.borrow().snapshot.records {
            latest.insert(r.header.secret, r.clone());
        }
        Ok(latest
            .into_values()
            .filter_map(|r| {
                let reference = SecretReference {
                    account: r.header.identity.account,
                    vault: r.header.identity.vault,
                    secret: r.header.secret,
                };
                (r.header.kind == RecordKind::Value
                    && binding.eligible(reference, &r.header.metadata).is_ok())
                .then_some((reference, r.header.metadata))
            })
            .collect())
    }

    /// Management projection is scoped to the authenticated account/root, not an agent allowlist.
    pub fn list(&self, session: &VaultSession, plugin: &PluginGrant) -> Result<Vec<RecordHeader>> {
        self.check(session)?;
        plugin.check(plugin.provider(), Permission::Manage)?;
        let mut latest = BTreeMap::new();
        for record in &self.0.state.borrow().snapshot.records {
            latest.insert(record.header.secret, record.header.clone());
        }
        Ok(latest
            .into_values()
            .filter(|record| record.kind == RecordKind::Value)
            .collect())
    }
    pub fn pending_mutations(
        &self,
        session: &VaultSession,
        plugin: &PluginGrant,
    ) -> Result<Vec<MutationIntent>> {
        self.check(session)?;
        plugin.check(plugin.provider(), Permission::Manage)?;
        Ok(self
            .0
            .state
            .borrow()
            .snapshot
            .pending
            .iter()
            .map(|command| {
                let header = &command.record().header;
                MutationIntent {
                    reference: SecretReference {
                        account: header.identity.account,
                        vault: header.identity.vault,
                        secret: header.secret,
                    },
                    expected: header.previous,
                    metadata: header.metadata.clone(),
                    kind: header.kind,
                    prepared_entry: command.operation,
                }
            })
            .collect())
    }
    fn persist(&self, mut snapshot: VaultSnapshot) -> Result<()> {
        let old = advance(&mut snapshot)?;
        snapshot.sign(self.identity(), self.0.device, &self.0.keys)?;
        self.0
            .storage
            .compare_exchange(self.0.identity, old, &snapshot)?;
        self.0.state.borrow_mut().snapshot = snapshot;
        Ok(())
    }

    pub async fn synchronize(&self, session: &VaultSession) -> Result<()> {
        let result = self.synchronize_records(session).await;
        if let Err(error) = result
            && !matches!(error, Error::Offline | Error::SignInRequired)
        {
            self.0.state.borrow_mut().records_failed = true;
        }
        result
    }

    async fn synchronize_records(&self, session: &VaultSession) -> Result<()> {
        self.refresh_authority(session).await?;
        self.check_admission(session)?;
        loop {
            let after = self.0.state.borrow().snapshot.cloud_cursor;
            let page = self.cloud().changes(self.0.identity, after).await?;
            let key = self.check_admission(session)?;
            if page.identity != self.0.identity
                || page.after != after
                || page.next
                    != after
                        .checked_add(page.records.len() as u64)
                        .ok_or(Error::Exhausted)?
                || (page.more && page.records.is_empty())
                || page.records.len() > 128
            {
                return Err(Error::Integrity);
            }
            let mut snapshot = self.0.state.borrow().snapshot.clone();
            for record in page.records {
                apply_record(&key, self.0.identity, &mut snapshot, record)?;
            }
            snapshot.cloud_cursor = page.next;
            if page.next != after {
                self.persist(snapshot)?;
            }
            if !page.more {
                self.0.state.borrow_mut().records_failed = false;
                return Ok(());
            }
        }
    }

    /// The actual executor derives this from its frozen operation and current secret policy.
    /// Model-supplied provider labels and cached policy generations are not admitted here.
    pub fn prepare_use(
        &self,
        session: &VaultSession,
        binding: &ExecutionBinding,
        plugin: &PluginGrant,
        request: SecretUseRequest,
    ) -> Result<UseIntent> {
        self.check(session)?;
        plugin.check(plugin.provider(), Permission::RequestUse)?;
        if !session.owner().same_owner(binding.shell()) {
            return Err(Error::Forbidden);
        }
        let record = self.latest(request.reference)?;
        if record.header.kind != RecordKind::Value {
            return Err(Error::NotFound);
        }
        binding.eligible(request.reference, &record.header.metadata)?;
        Ok(UseIntent {
            reference: request.reference,
            metadata: record.header.metadata.clone(),
            operation: request.operation,
            attempt: request.attempt,
            provider: plugin.provider().into(),
            destination: request.destination,
            execution: binding.identity().clone(),
            policy_generation: record.header.metadata.policy_generation,
            permission_generation: plugin.generation(),
        })
    }

    pub fn requires_approval(
        &self,
        session: &VaultSession,
        binding: &ExecutionBinding,
        plugin: &PluginGrant,
        intent: &UseIntent,
    ) -> Result<bool> {
        self.check(session)?;
        plugin.check(&intent.provider, Permission::ReceiveForExecution)?;
        if plugin.generation() != intent.permission_generation
            || !session.owner().same_owner(binding.shell())
            || intent.execution != *binding.identity()
        {
            return Err(Error::Forbidden);
        }
        let record = self.latest(intent.reference)?;
        binding.eligible(intent.reference, &record.header.metadata)?;
        if record.header.kind != RecordKind::Value {
            return Err(Error::NotFound);
        }
        if record.header.metadata.policy_generation != intent.policy_generation
            || record.header.metadata != intent.metadata
        {
            return Err(Error::ApprovalRequired);
        }
        Ok(record.header.metadata.approval != Approval::None
            && !self.0.state.borrow().grants.iter().any(|(id, grant)| {
                *id == session.0.id
                    && grant.permits(binding, intent, record.header.metadata.approval)
            }))
    }

    pub fn approve_use(
        &self,
        session: &VaultSession,
        binding: &ExecutionBinding,
        plugin: &PluginGrant,
        intent: UseIntent,
        conversation: bool,
        decision: HumanDecision,
    ) -> Result<()> {
        self.check(session)?;
        plugin.check(&intent.provider, Permission::RequestUse)?;
        if plugin.generation() != intent.permission_generation {
            return Err(Error::Forbidden);
        }
        let record = self.latest(intent.reference)?;
        binding.eligible(intent.reference, &record.header.metadata)?;
        if !session.owner().same_owner(binding.shell())
            || intent.execution != *binding.identity()
            || intent.metadata != record.header.metadata
            || intent.policy_generation != record.header.metadata.policy_generation
            || (conversation
                && (!binding.identity().main_agent
                    || binding.identity().conversation.is_none()
                    || record.header.metadata.approval != Approval::Conversation))
        {
            return Err(Error::Forbidden);
        }
        decision.consume_owner(session.owner(), &("vault-use/v1", &intent, conversation))?;
        self.0.state.borrow_mut().grants.push((
            session.0.id,
            UseGrant {
                shell: session.owner().clone(),
                execution: binding.identity().clone(),
                reference: intent.reference,
                policy: intent.policy_generation,
                permission: intent.permission_generation,
                provider: intent.provider.clone(),
                conversation,
                intent: (!conversation).then_some(intent),
            },
        ));
        Ok(())
    }

    /// Actual dispatch acquires the latest confirmed value, never a prepared plaintext argument.
    pub fn lease(
        &self,
        session: &VaultSession,
        binding: &ExecutionBinding,
        plugin: &PluginGrant,
        intent: &UseIntent,
    ) -> Result<SecretLease> {
        let key = self.check(session)?;
        if self
            .0
            .state
            .borrow()
            .snapshot
            .pending
            .iter()
            .any(|command| {
                command.record().header.secret == intent.reference.secret
                    && self
                        .0
                        .state
                        .borrow()
                        .snapshot
                        .accepted
                        .contains(&command.operation)
            })
        {
            return Err(Error::Unavailable);
        }
        plugin.check(&intent.provider, Permission::ReceiveForExecution)?;
        if plugin.generation() != intent.permission_generation {
            return Err(Error::Forbidden);
        }
        if intent.execution != *binding.identity() || !session.owner().same_owner(binding.shell()) {
            return Err(Error::Forbidden);
        }
        let record = self.latest(intent.reference)?;
        binding.eligible(intent.reference, &record.header.metadata)?;
        if record.header.metadata.policy_generation != intent.policy_generation
            || record.header.metadata != intent.metadata
        {
            return Err(Error::ApprovalRequired);
        }
        if record.header.metadata.approval != Approval::None {
            let mut state = self.0.state.borrow_mut();
            let at = state
                .grants
                .iter()
                .position(|(id, g)| {
                    *id == session.0.id
                        && g.permits(binding, intent, record.header.metadata.approval)
                })
                .ok_or(Error::ApprovalRequired)?;
            if !state.grants[at].1.conversation {
                state.grants.remove(at);
            }
        }
        Ok(SecretLease {
            value: key.open(&record)?,
            reference: intent.reference,
            revision: record.header.revision,
        })
    }
}

/// Native executor data; deliberately neither Debug, Clone nor Serialize.
pub struct SecretLease {
    value: SecretValue,
    pub reference: SecretReference,
    pub revision: Uuid,
}

impl SecretLease {
    pub fn deliver<T>(&self, consumer: impl FnOnce(&[u8]) -> Result<T>) -> Result<T> {
        consumer(self.value.expose())
    }

    pub fn mask(&self, output: &str) -> String {
        let value = std::str::from_utf8(self.value.expose()).expect("validated UTF-8");
        output.replace(value, &self.reference.to_string())
    }
}

fn next_successor(prior: Option<&EncryptedRecord>) -> Result<u64> {
    prior.map_or(Ok(1), |r| {
        r.header.successor.checked_add(1).ok_or(Error::Exhausted)
    })
}

fn apply_record(
    key: &VaultKey,
    identity: VaultIdentity,
    snapshot: &mut VaultSnapshot,
    record: EncryptedRecord,
) -> Result<()> {
    if record.header.identity != identity {
        return Err(Error::Integrity);
    }
    key.verify(&record)?;
    if snapshot
        .records
        .iter()
        .any(|r| r.header.revision == record.header.revision)
    {
        return Err(Error::Integrity);
    }
    let prior = snapshot
        .records
        .iter()
        .rev()
        .find(|r| r.header.secret == record.header.secret);
    if record.header.previous != prior.map(|r| r.header.revision)
        || record.header.successor != next_successor(prior)?
    {
        return Err(Error::Stale);
    }
    snapshot
        .pending
        .retain(|c| c.operation != record.header.revision);
    snapshot
        .accepted
        .retain(|operation| *operation != record.header.revision);
    snapshot.records.push(record);
    Ok(())
}

/// Safe request metadata; only the actual executor selects a destination and immutable operation.
pub struct SecretUseRequest {
    pub reference: SecretReference,
    pub operation: Uuid,
    pub attempt: Uuid,
    pub destination: String,
}
