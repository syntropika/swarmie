//! One native review over the authenticated setup channel. No Tembra object is served here.
use crate::device_linking::*;
use futures::lock::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{rc::Rc, time::Duration};
use swarmie_permissions::transport::BootstrapStream;
use swarmie_permissions::{HumanDecision, HumanSession};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use uuid::Uuid;

/// The embedding durably records uncertainty before any admission signature can escape.
/// Store only public immutable intent/proofs, never the private offer or TLS exporter.
pub trait LinkJournal {
    /// Record the exact operation before releasing the first admission signature.
    fn prepare(&self, intent: &LinkIntent) -> Result<()>;
    /// Retain both endpoint proofs before Cloud submission or peer delivery.
    fn signed(&self, request: &SignedLink) -> Result<()>;
    /// Record an independently recognized result before enabling normal management RPC.
    fn settle(&self, receipt: &LinkReceipt) -> Result<()>;
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Frame {
    Refused {
        reason: Refusal,
    },
    Claim {
        proof: Vec<u8>,
    },
    Review {
        intent: Box<LinkIntent>,
        signature: Vec<u8>,
    },
    Confirm {
        signature: Vec<u8>,
    },
    Countersign {
        signature: Vec<u8>,
    },
    Receipt {
        receipt: LinkReceipt,
    },
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Refusal {
    Claimed,
    AccountMismatch,
    Expired,
    Cancelled,
    HumanUnavailable,
    Invalid,
    Unavailable,
}

async fn refuse<S: AsyncWrite + Unpin>(stream: &mut S, reason: Refusal) {
    let _ = tokio::time::timeout(Duration::from_secs(5), async {
        send(stream, &Frame::Refused { reason }).await?;
        stream.shutdown().await.map_err(|_| Error::Unavailable)
    })
    .await;
}

async fn send<S: AsyncWrite + Unpin>(stream: &mut S, frame: &Frame) -> Result<()> {
    let bytes = zeroize::Zeroizing::new(canonical(frame)?);
    let length: u32 = bytes.len().try_into().map_err(|_| Error::Invalid)?;
    if length > 64 * 1024 {
        return Err(Error::Invalid);
    }
    stream
        .write_u32(length)
        .await
        .map_err(|_| Error::Unavailable)?;
    stream
        .write_all(&bytes)
        .await
        .map_err(|_| Error::Unavailable)?;
    stream.flush().await.map_err(|_| Error::Unavailable)
}
async fn receive<S: AsyncRead + Unpin>(stream: &mut S) -> Result<Frame> {
    tokio::time::timeout(Duration::from_secs(15), read_frame(stream))
        .await
        .map_err(|_| Error::Unavailable)?
}

async fn read_frame<S: AsyncRead + Unpin>(stream: &mut S) -> Result<Frame> {
    let length = stream.read_u32().await.map_err(|_| Error::Unavailable)? as usize;
    if length == 0 || length > 64 * 1024 {
        return Err(Error::Invalid);
    }
    let mut bytes = zeroize::Zeroizing::new(vec![0; length]);
    stream
        .read_exact(&mut bytes)
        .await
        .map_err(|_| Error::Unavailable)?;
    let frame: Frame = serde_json::from_slice(&bytes).map_err(|_| Error::Invalid)?;
    if let Frame::Refused { reason } = frame {
        return Err(match reason {
            Refusal::Claimed => Error::Claimed,
            Refusal::Expired => Error::Expired,
            Refusal::AccountMismatch | Refusal::HumanUnavailable => Error::Forbidden,
            Refusal::Cancelled => Error::Cancelled,
            Refusal::Invalid => Error::Invalid,
            Refusal::Unavailable => Error::Unavailable,
        });
    }
    Ok(frame)
}

fn human_owner(human: &HumanSession, device: &CloudDevice) -> Result<()> {
    let identity = human.identity();
    if !human.is_live()
        || identity.account != Some(device.account().identity().account)
        || identity.cloud.as_ref().is_none_or(|cloud| {
            cloud.family != device.account().identity().family
                || cloud.client != Sha256::digest(device.keys().public()).to_vec()
        })
    {
        return Err(Error::Forbidden);
    }
    Ok(())
}
fn recognized(receipt: &LinkReceipt, intent: &LinkIntent) -> Result<()> {
    if receipt.operation != intent.operation || receipt.intent_digest != digest(intent)? {
        return Err(Error::Integrity);
    }
    Ok(())
}

/// Target owns the one-use offer and fixes its node before publishing any transfer code.
pub struct LinkTarget {
    device: CloudDevice,
    owner: Mutex<OfferOwner>,
    offer: LinkOffer,
    node: String,
    human: HumanSession,
    journal: Rc<dyn LinkJournal>,
}
impl LinkTarget {
    /// Called by the actual native operator action. The decision covers account and fixed node.
    pub async fn start(
        device: CloudDevice,
        node: String,
        human: HumanSession,
        decision: HumanDecision,
        journal: Rc<dyn LinkJournal>,
    ) -> Result<Self> {
        human_owner(&human, &device)?;
        decision
            .consume(
                &human,
                &(
                    "swarmie-device-offer/v1",
                    device.account().identity().account,
                    &node,
                ),
            )
            .map_err(|_| Error::Forbidden)?;
        let registration = device.registered().await?;
        if !registration.nodes.contains(&node) {
            return Err(Error::Forbidden);
        }
        let owner = OfferOwner::new(
            device.keys(),
            registration.account,
            node.clone(),
            device
                .account()
                .endpoints()
                .api
                .trim_end_matches('/')
                .into(),
            Duration::from_secs(300),
        )?;
        let offer = owner.offer()?;
        device.publish_offer(&offer, 300).await?;
        Ok(Self {
            device,
            owner: Mutex::new(owner),
            offer,
            node,
            human,
            journal,
        })
    }
    /// Full high-entropy code for a qualified private display/clipboard only, never public State.
    pub async fn code(&self) -> Result<zeroize::Zeroizing<String>> {
        self.owner.lock().await.offer()?.code()
    }
    /// Bounded candidate discovery; invalid proofs never consume the offer.
    pub async fn pending(&self) -> Result<Vec<Uuid>> {
        self.device.pending_streams(&self.offer).await
    }
    /// Complete target TLS, authenticate the private capability and propose one exact stable intent.
    /// Vault data is public recipient commitment supplied by the target's native vault owner.
    pub async fn accept(&self, connection: Uuid, vault: Option<VaultLink>) -> Result<TargetClaim> {
        if vault.is_some() {
            return Err(Error::Unavailable);
        }
        human_owner(&self.human, &self.device)?;
        let mut stream = self
            .device
            .accept_bootstrap(&self.offer, connection)
            .await?;
        let Frame::Claim { proof } = receive(&mut stream).await? else {
            return Err(Error::Invalid);
        };
        if proof.len() != 32 {
            return Err(Error::Invalid);
        }
        let binding = stream.binding();
        let claimed = match self.owner.lock().await.claim_key(&binding, &proof) {
            Ok(claimed) => claimed,
            Err(error) => {
                let reason = match error {
                    Error::Claimed => Refusal::Claimed,
                    Error::Expired => Refusal::Expired,
                    _ => Refusal::Invalid,
                };
                refuse(&mut stream, reason).await;
                return Err(error);
            }
        };
        let claimant = match self.device.lookup(claimed.device()).await {
            Ok(claimant) => claimant,
            Err(error) => {
                refuse(
                    &mut stream,
                    if error == Error::Forbidden {
                        Refusal::AccountMismatch
                    } else {
                        Refusal::Unavailable
                    },
                )
                .await;
                return Err(error);
            }
        };
        let claim = claimed.authorize(&claimant)?;
        self.device.claim_stream(&self.offer, connection).await?;
        let local = self.device.registered().await?;
        let intent = LinkIntent {
            operation: Uuid::new_v4(),
            nonce: Uuid::new_v4(),
            account: local.account,
            controller: claimant.device,
            controller_generation: claimant.generation,
            target: local.device,
            target_generation: local.generation,
            node: self.node.clone(),
            offer: self.offer.rendezvous(),
            vault,
        };
        claim.check(&intent)?;
        let signature = self
            .device
            .keys()
            .sign(&canonical(&("swarmie-link-review/v1", &intent))?);
        send(
            &mut stream,
            &Frame::Review {
                intent: Box::new(intent.clone()),
                signature,
            },
        )
        .await?;
        Ok(TargetClaim {
            device: self.device.clone(),
            stream,
            claim,
            intent,
            human: self.human.clone(),
            journal: self.journal.clone(),
        })
    }
}

/// Claimed native target. Its labels are obtained from authenticated same-account directory data.
pub struct TargetClaim {
    device: CloudDevice,
    stream: BootstrapStream<CloudStream>,
    claim: ClaimedOffer,
    intent: LinkIntent,
    human: HumanSession,
    journal: Rc<dyn LinkJournal>,
}
impl TargetClaim {
    /// Public immutable intent; this does not grant a link or vault session.
    pub fn intent(&self) -> &LinkIntent {
        &self.intent
    }
    /// Authenticated registered claimant display data.
    pub fn claimant(&self) -> &DeviceRegistration {
        self.claim.claimant()
    }
    /// Wait for the client's single confirmation, then journal before releasing the target signature.
    pub async fn complete(mut self) -> Result<LinkReceipt> {
        let Frame::Confirm { signature } =
            tokio::time::timeout(Duration::from_secs(600), read_frame(&mut self.stream))
                .await
                .map_err(|_| Error::Expired)??
        else {
            return Err(Error::Invalid);
        };
        if let Err(error) = human_owner(&self.human, &self.device) {
            refuse(&mut self.stream, Refusal::HumanUnavailable).await;
            return Err(error);
        }
        self.claim.check(&self.intent)?;
        verify(
            &self.claim.claimant().public_key,
            &self.intent.signing_bytes()?,
            &signature,
        )?;
        self.journal.prepare(&self.intent)?;
        let request = SignedLink {
            intent: self.intent.clone(),
            controller_signature: signature,
            target_signature: self.device.keys().sign(&self.intent.signing_bytes()?),
        };
        self.journal.signed(&request)?;
        send(
            &mut self.stream,
            &Frame::Countersign {
                signature: request.target_signature,
            },
        )
        .await
        .map_err(|_| Error::Unknown)?;
        let hint = receive(&mut self.stream).await;
        let mut original = None;
        for attempt in 0..3 {
            if let Ok(Some(receipt)) = self.device.recognize(&self.intent).await {
                recognized(&receipt, &self.intent)?;
                original = Some(receipt);
                break;
            }
            if attempt < 2 {
                tokio::time::sleep(Duration::from_millis(100 * (attempt + 1))).await;
            }
        }
        let original = original.ok_or(Error::Unknown)?;
        if let Ok(Frame::Receipt { receipt }) = hint {
            recognized(&receipt, &self.intent)?;
            if original != receipt {
                return Err(Error::Integrity);
            }
        }
        self.journal.settle(&original)?;
        let _ = tokio::time::timeout(Duration::from_secs(5), self.stream.shutdown()).await;
        Ok(original)
    }
}

/// Client-side single review. It retains the actual completed channel and immutable intent.
pub struct LinkReview {
    device: CloudDevice,
    stream: BootstrapStream<CloudStream>,
    intent: LinkIntent,
    target: DeviceRegistration,
    journal: Rc<dyn LinkJournal>,
}
impl LinkReview {
    /// Parse the private code, authenticate the target, then prove the private capability.
    pub async fn connect(
        device: CloudDevice,
        code: &str,
        journal: Rc<dyn LinkJournal>,
    ) -> Result<Self> {
        let offer =
            LinkOffer::decode(code, device.account().endpoints().api.trim_end_matches('/'))?;
        let local = device.registered().await?;
        let target = device.lookup(DeviceId::of(&offer.target())).await?;
        let mut stream = device.connect_bootstrap(&offer, Uuid::new_v4()).await?;
        let proof = offer.claim_proof(&stream.binding())?.to_vec();
        send(&mut stream, &Frame::Claim { proof }).await?;
        let Frame::Review { intent, signature } = receive(&mut stream).await? else {
            return Err(Error::Invalid);
        };
        let intent = *intent;
        // Root-aware admission must be supplied by the integrated vault owner before this
        // field can be approved. A target-provided root is never consented to implicitly.
        if intent.vault.is_some() {
            return Err(Error::Unavailable);
        }
        intent.signing_bytes()?;
        if intent.account != local.account
            || intent.controller != local.device
            || intent.controller_generation != local.generation
            || intent.target != target.device
            || intent.target_generation != target.generation
            || intent.offer != offer.rendezvous()
            || !target.nodes.contains(&intent.node)
        {
            return Err(Error::Forbidden);
        }
        verify(
            &target.public_key,
            &canonical(&("swarmie-link-review/v1", &intent))?,
            &signature,
        )?;
        Ok(Self {
            device,
            stream,
            intent,
            target,
            journal,
        })
    }
    /// The exact immutable payload for the native confirmation action.
    pub fn intent(&self) -> &LinkIntent {
        &self.intent
    }
    /// Authenticated account-managed target label and fingerprint source.
    pub fn target(&self) -> &DeviceRegistration {
        &self.target
    }
    /// Cancel before an admission signature escapes. This consumes only the provisional review.
    pub async fn cancel(mut self) {
        refuse(&mut self.stream, Refusal::Cancelled).await;
    }

    /// Consume the single exact native human action. Journal before any admission signature escapes.
    pub async fn confirm(
        mut self,
        human: &HumanSession,
        decision: HumanDecision,
    ) -> Result<LinkReceipt> {
        human_owner(human, &self.device)?;
        decision
            .consume(human, &("swarmie-device-link-consent/v1", &self.intent))
            .map_err(|_| Error::Forbidden)?;
        self.journal.prepare(&self.intent)?;
        let signature = self.device.keys().sign(&self.intent.signing_bytes()?);
        send(
            &mut self.stream,
            &Frame::Confirm {
                signature: signature.clone(),
            },
        )
        .await
        .map_err(|_| Error::Unknown)?;
        let Frame::Countersign {
            signature: target_signature,
        } = receive(&mut self.stream)
            .await
            .map_err(|_| Error::Unknown)?
        else {
            return Err(Error::Unknown);
        };
        verify(
            &self.target.public_key,
            &self.intent.signing_bytes()?,
            &target_signature,
        )?;
        let request = SignedLink {
            intent: self.intent.clone(),
            controller_signature: signature,
            target_signature,
        };
        self.journal.signed(&request)?;
        let receipt = self.device.admit(&request).await?;
        self.journal.settle(&receipt)?;
        // Failure to notify the peer cannot replace this independently confirmed outcome.
        let _ = send(
            &mut self.stream,
            &Frame::Receipt {
                receipt: receipt.clone(),
            },
        )
        .await;
        let _ = tokio::time::timeout(Duration::from_secs(5), self.stream.shutdown()).await;
        Ok(receipt)
    }
}
