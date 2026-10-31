//! Qualified native HTTPS adapter. Actual channel proofs come from the private management owner.
mod broker;
pub use broker::NativeUnlockBroker;

use crate::*;
use futures::{FutureExt, future::LocalBoxFuture};
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    rc::Rc,
    time::{Duration, Instant},
};
use swarmie_cloud_account::AccountSession;
use swarmie_permissions::transport::ChannelBinding;
use uuid::Uuid;

/// The embedding exchanges ticket control only through its actual mutually pinned byte carrier.
/// There is no implementation that accepts JSON exporter claims or a reachable socket as proof.
pub trait UnlockBroker {
    fn authorize<'a>(
        &'a self,
        request: &'a ReleaseRequest,
    ) -> LocalBoxFuture<'a, Result<AuthorizedUnlock>>;
}

/// Independent enrollment/operator pins supplied by the native trust custodian.
pub struct UnlockChallenges {
    identity: VaultIdentity,
    node: Uuid,
    signing: [u8; 32],
    managers: BTreeSet<[u8; 32]>,
}

impl UnlockChallenges {
    pub fn new(
        identity: VaultIdentity,
        node: Uuid,
        signing: [u8; 32],
        managers: BTreeSet<[u8; 32]>,
    ) -> Self {
        Self {
            identity,
            node,
            signing,
            managers,
        }
    }
    pub fn issue(
        &self,
        request: &ReleaseRequest,
        channel: ChannelBinding,
    ) -> Result<UnlockChallenge> {
        if request.identity != self.identity
            || request.device != self.node
            || request.signing_key != self.signing
            || channel.local() != &self.signing
            || !channel.is_live()
            || !self.managers.contains(channel.peer())
            || channel.peer_fingerprint() != request.client
        {
            return Err(Error::Forbidden);
        }
        verify_signature(
            &self.signing,
            &canonical(&(
                "vault-release/v1",
                request.identity,
                request.device,
                &request.client,
                request.family,
                request.shell_installation,
                request.shell_generation,
                &request.signing_key,
                request.nonce,
            ))?,
            &request.signature,
        )?;
        Ok(UnlockChallenge {
            request: request.clone(),
            channel,
            challenge: Uuid::new_v4(),
            expires: Instant::now() + Duration::from_secs(30),
        })
    }
}

/// One-use native challenge, never reconstructed from public identifiers or serialized State.
pub struct UnlockChallenge {
    request: ReleaseRequest,
    channel: ChannelBinding,
    challenge: Uuid,
    expires: Instant,
}
impl UnlockChallenge {
    pub fn intent(&self, resume: Vec<u8>) -> Result<TicketIntent> {
        if !self.channel.is_live() || Instant::now() > self.expires || resume.len() != 32 {
            return Err(Error::Revoked);
        }
        Ok(TicketIntent {
            identity: self.request.identity,
            node: self.request.device,
            client: self.request.client.clone(),
            shell: self.request.shell_generation,
            resume,
            exporter: self.channel.exporter().to_vec(),
            challenge: self.challenge,
            nonce: self.request.nonce,
        })
    }
    pub fn confirm(self, ticket: Ticket) -> Result<AuthorizedUnlock> {
        if ticket.intent != self.intent(ticket.intent.resume.clone())? {
            return Err(Error::Integrity);
        }
        Ok(AuthorizedUnlock {
            ticket,
            channel: self.channel,
            request: self.request,
        })
    }
}

/// Cannot be constructed by a model, generic RPC payload or claimed exporter byte string.
pub struct AuthorizedUnlock {
    ticket: Ticket,
    channel: ChannelBinding,
    request: ReleaseRequest,
}

/// One root/node's account transport. Independent of the Cloud implementation's source tree.
pub struct CloudVault {
    account: AccountSession,
    identity: VaultIdentity,
    node: Uuid,
    keys: Rc<DeviceKeys>,
    broker: Rc<dyn UnlockBroker>,
}
impl CloudVault {
    pub fn new(
        account: AccountSession,
        identity: VaultIdentity,
        node: Uuid,
        keys: Rc<DeviceKeys>,
        broker: Rc<dyn UnlockBroker>,
    ) -> Result<Self> {
        if account.identity().account != identity.account {
            return Err(Error::Forbidden);
        }
        Ok(Self {
            account,
            identity,
            node,
            keys,
            broker,
        })
    }
    /// Persist the safe receipt first; the adapter makes exactly one physical command attempt.
    pub async fn submit_ceremony(
        &self,
        command: &LifecycleCommand,
        expected: &CeremonyReceipt,
    ) -> Result<CommitOutcome> {
        submit_ceremony(&self.account, command, expected).await
    }
    pub async fn recognize_ceremony(&self, expected: &CeremonyReceipt) -> Result<CommitOutcome> {
        recognize_ceremony(&self.account, expected).await
    }
    async fn page(&self, after: u64) -> Result<CloudRecordPage> {
        self.account
            .post(
                "/api/vault/records",
                &serde_json::json!({"identity":self.identity,"device":self.node,"after":after}),
            )
            .await
            .map_err(account_error)
    }
    pub async fn fence(&self, command: &RecordCommand) -> Result<CommitOutcome> {
        self.command("/api/vault/fence", command).await
    }
    async fn command(&self, path: &str, command: &RecordCommand) -> Result<CommitOutcome> {
        if command.device != self.node || command.record().header.identity != self.identity {
            return Err(Error::Forbidden);
        }
        let bytes = command.signing_bytes()?;
        verify_signature(&self.keys.signing_public(), &bytes, &command.signature)?;
        let receipt: Receipt = self
            .account
            .post(path, command)
            .await
            .map_err(account_error)?;
        if receipt.operation != command.operation || receipt.payload != digest(&bytes) {
            return Err(Error::Integrity);
        }
        Ok(receipt.outcome.into())
    }
}

impl VaultCloud for CloudVault {
    fn current_authority(
        &self,
        identity: VaultIdentity,
    ) -> LocalBoxFuture<'_, Result<AuthorityCheck>> {
        async move {
            if identity != self.identity {
                return Err(Error::Forbidden);
            }
            let check: AuthorityCheck = self
                .account
                .post(
                    "/api/vault/check",
                    &serde_json::json!({"identity":identity}),
                )
                .await
                .map_err(account_error)?;
            if check.actor_family != self.account.identity().family {
                return Err(Error::Integrity);
            }
            Ok(check)
        }
        .boxed_local()
    }

    fn release<'a>(
        &'a self,
        request: ReleaseRequest,
    ) -> LocalBoxFuture<'a, Result<ReleasedEnvelope>> {
        async move{
            let authorized=self.broker.authorize(&request).await?;
            if !authorized.channel.is_live() || authorized.request.nonce!=request.nonce || authorized.ticket.intent.exporter!=authorized.channel.exporter()
                || authorized.ticket.intent.client!=request.client || authorized.ticket.intent.identity!=self.identity || authorized.ticket.intent.node!=self.node
                || request.signing_key!=self.keys.signing_public() { return Err(Error::Integrity); }
            let family=self.account.identity().family;
            let signature=self.keys.sign(&canonical(&("vault-redeem/v1",authorized.ticket.id,&authorized.ticket.intent,family))?);
            let release:CloudRelease=self.account.post("/api/vault/redeem",&serde_json::json!({"ticket":authorized.ticket.id,"exporter":authorized.channel.exporter(),"signature":signature})).await.map_err(account_error)?;
            if !authorized.channel.is_live() || release.admission.ticket!=authorized.ticket.id || release.admission.intent!=authorized.ticket.intent
                || release.admission.family!=request.family { return Err(Error::Integrity); }
            Ok(ReleasedEnvelope { enrollment:release.certificate,envelope:release.envelope,admission:release.admission })
        }.boxed_local()
    }
    fn mode<'a>(&'a self, identity: VaultIdentity) -> LocalBoxFuture<'a, Result<VaultMode>> {
        Box::pin(async move {
            let check = self.current_authority(identity).await?;
            Ok(if check.exposed {
                VaultMode::Exposed
            } else if check.current == Some(identity.vault) {
                VaultMode::Current
            } else {
                VaultMode::ArchivedReadable
            })
        })
    }
    fn commit<'a>(&'a self, command: RecordCommand) -> LocalBoxFuture<'a, Result<CommitOutcome>> {
        async move { self.command("/api/vault/commands", &command).await }.boxed_local()
    }
    fn fence(&self, command: RecordCommand) -> LocalBoxFuture<'_, Result<CommitOutcome>> {
        Box::pin(async move { CloudVault::fence(self, &command).await })
    }
    fn recognize<'a>(
        &'a self,
        command: RecordCommand,
    ) -> LocalBoxFuture<'a, Result<CommitOutcome>> {
        Box::pin(async move {
            if command.device != self.node || command.record().header.identity != self.identity {
                return Err(Error::Forbidden);
            }
            let bytes = command.signing_bytes()?;
            verify_signature(&self.keys.signing_public(), &bytes, &command.signature)?;
            let receipt: Option<Receipt> = self
                .account
                .post(
                    &format!("/api/vault/recognize/{}", command.operation),
                    &serde_json::json!({}),
                )
                .await
                .map_err(account_error)?;
            match receipt {
                Some(receipt)
                    if receipt.operation == command.operation
                        && receipt.payload == digest(&bytes) =>
                {
                    Ok(receipt.outcome.into())
                }
                Some(_) => Err(Error::Integrity),
                None => Ok(CommitOutcome::Unknown),
            }
        })
    }
    fn changes<'a>(
        &'a self,
        identity: VaultIdentity,
        after: u64,
    ) -> LocalBoxFuture<'a, Result<CloudRecordPage>> {
        async move {
            if identity != self.identity {
                return Err(Error::Forbidden);
            }
            self.page(after).await
        }
        .boxed_local()
    }
}

#[derive(Deserialize)]
struct Receipt {
    operation: Uuid,
    payload: Vec<u8>,
    outcome: Outcome,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    Accepted,
    RejectedVaultExists,
    RejectedStale,
    RejectedNameConflict,
    RejectedAuthorization,
    VaultNotCurrent,
    RootExposed,
    NotDispatched,
}
impl From<Outcome> for CommitOutcome {
    fn from(value: Outcome) -> Self {
        match value {
            Outcome::Accepted => Self::Accepted,
            Outcome::RejectedStale => Self::RejectedStale,
            Outcome::RejectedNameConflict => Self::RejectedNameConflict,
            Outcome::RejectedVaultExists => Self::RejectedVaultExists,
            Outcome::RejectedAuthorization => Self::RejectedAuthorization,
            Outcome::VaultNotCurrent => Self::VaultNotCurrent,
            Outcome::RootExposed => Self::RootExposed,
            Outcome::NotDispatched => Self::NotDispatched,
        }
    }
}
fn account_error(error: swarmie_cloud_account::Error) -> Error {
    match error {
        swarmie_cloud_account::Error::Refused => Error::CloudRefused,
        swarmie_cloud_account::Error::Revoked => Error::Revoked,
        swarmie_cloud_account::Error::Forbidden => Error::Forbidden,
        swarmie_cloud_account::Error::Invalid | swarmie_cloud_account::Error::Integrity => {
            Error::Integrity
        }
        swarmie_cloud_account::Error::SignInRequired
        | swarmie_cloud_account::Error::RefreshUnknown => Error::SignInRequired,
        swarmie_cloud_account::Error::ProtectedStorageUnavailable => Error::Unavailable,
        swarmie_cloud_account::Error::Unavailable => Error::Offline,
    }
}

/// No returned inner envelope is persisted to recover an uncertain lifecycle operation.
pub async fn submit_ceremony(
    account: &AccountSession,
    command: &LifecycleCommand,
    expected: &CeremonyReceipt,
) -> Result<CommitOutcome> {
    if expected.identity.account != account.identity().account || command.receipt()? != *expected {
        return Err(Error::Forbidden);
    }
    let receipt: Receipt = account
        .post("/api/vault/commands", command)
        .await
        .map_err(account_error)?;
    check_ceremony_receipt(receipt, expected)
}
pub async fn recognize_ceremony(
    account: &AccountSession,
    expected: &CeremonyReceipt,
) -> Result<CommitOutcome> {
    if expected.identity.account != account.identity().account {
        return Err(Error::Forbidden);
    }
    let receipt: Option<Receipt> = account
        .post(
            &format!("/api/vault/recognize/{}", expected.operation),
            &serde_json::json!({}),
        )
        .await
        .map_err(account_error)?;
    receipt.map_or(Ok(CommitOutcome::Unknown), |receipt| {
        check_ceremony_receipt(receipt, expected)
    })
}
fn check_ceremony_receipt(receipt: Receipt, expected: &CeremonyReceipt) -> Result<CommitOutcome> {
    if receipt.operation != expected.operation || receipt.payload != expected.payload {
        return Err(Error::Integrity);
    }
    Ok(receipt.outcome.into())
}
/// Human-initiated resolution can fence the exact provisional founding key after a restart,
/// without storing E or V locally. This endpoint cannot fence ordinary record/lifecycle commands.
pub async fn fence_founding(
    account: &AccountSession,
    expected: &CeremonyReceipt,
    device: Uuid,
    keys: &DeviceKeys,
) -> Result<CommitOutcome> {
    if expected.identity.account != account.identity().account || expected.payload.len() != 32 {
        return Err(Error::Forbidden);
    }
    let public = keys.signing_public();
    let signature = keys.sign(&canonical(&(
        "vault-fence-found/v1",
        expected.identity,
        expected.operation,
        device,
        &public,
        &expected.payload,
    ))?);
    let receipt:Receipt=account.post("/api/vault/fence-found",&serde_json::json!({"identity":expected.identity,"operation":expected.operation,"device":device,"public":public,"payload":expected.payload,"signature":signature})).await.map_err(account_error)?;
    check_ceremony_receipt(receipt, expected)
}

/// Settle an original enrollment/rebind/removal/restore using a retained device key and digest.
/// No inner HPKE envelope is needed or persisted for this recovery action.
pub async fn fence_ceremony(
    account: &AccountSession,
    expected: &CeremonyReceipt,
    keys: &DeviceKeys,
) -> Result<CommitOutcome> {
    if expected.identity.account != account.identity().account || expected.payload.len() != 32 {
        return Err(Error::Forbidden);
    }
    let signature = keys.sign(&canonical(&(
        "vault-fence-lifecycle/v1",
        expected.identity,
        expected.operation,
        expected.device,
        &expected.payload,
    ))?);
    let receipt:Receipt=account.post("/api/vault/fence-lifecycle",&serde_json::json!({"identity":expected.identity,"operation":expected.operation,"device":expected.device,"payload":expected.payload,"signature":signature})).await.map_err(account_error)?;
    check_ceremony_receipt(receipt, expected)
}
