use crate::*;
use futures::lock::Mutex;
use std::rc::Rc;
use swarmie_cloud_account::AccountSession;
use uuid::Uuid;

/// One account/native endpoint owner. Re-registration is shared by all root-specific consumers.
#[derive(Clone)]
pub struct CloudDevice(Rc<Owner>);

struct Owner {
    account: AccountSession,
    keys: Rc<EndpointKeys>,
    name: String,
    node: Option<String>,
    registration: Mutex<Option<DeviceRegistration>>,
}

pub(super) fn account_error(error: swarmie_cloud_account::Error) -> Error {
    use swarmie_cloud_account::Error as E;
    match error {
        E::Forbidden | E::Revoked | E::SignInRequired => Error::Forbidden,
        E::Integrity => Error::Integrity,
        E::Invalid => Error::Invalid,
        _ => Error::Unavailable,
    }
}

impl CloudDevice {
    /// Supply protected retained keys and the actual native account owner, not browser IDs.
    pub fn new(
        account: AccountSession,
        keys: Rc<EndpointKeys>,
        name: String,
        node: Option<String>,
    ) -> Result<Self> {
        if !valid_label(&name) || node.as_ref().is_some_and(|node| !valid_node(node)) {
            return Err(Error::Invalid);
        }
        Ok(Self(Rc::new(Owner {
            account,
            keys,
            name,
            node,
            registration: Mutex::new(None),
        })))
    }

    /// Authenticated native account owner.
    pub fn account(&self) -> &AccountSession {
        &self.0.account
    }

    /// Native endpoint key owner; never exposed through a browser service.
    pub(crate) fn keys(&self) -> &EndpointKeys {
        &self.0.keys
    }

    /// Stable public endpoint identity. It grants no private signing or transport authority.
    pub fn id(&self) -> DeviceId {
        self.0.keys.id()
    }

    /// Canonical public endpoint key for native identity presentation and capability bindings.
    pub fn public_key(&self) -> [u8; 32] {
        self.0.keys.public()
    }

    /// The cache belongs to this immutable native AccountSession. A rejected dependent request
    /// clears it; the hosted owner prevents reactivation of any superseded native family.
    pub(crate) async fn invalidate_registration(&self) {
        self.0.registration.lock().await.take();
    }

    /// Same-key login rebind is automatic. An ordinary call with unchanged family/identity
    /// performs no additional registration mutation or per-root ceremony.
    pub async fn registered(&self) -> Result<DeviceRegistration> {
        if !self.0.account.is_live() {
            return Err(Error::Forbidden);
        }
        let mut registration = self.0.registration.lock().await;
        if let Some(value) = registration.as_ref() {
            return Ok(value.clone());
        }
        let current: Option<DeviceRegistration> = self
            .0
            .account
            .post(
                "/api/devices/registration-state",
                &serde_json::json!({"device":self.0.keys.id()}),
            )
            .await
            .map_err(account_error)?;
        if let Some(value) = &current {
            self.check(value)?;
            if value.public_key != self.0.keys.public() || value.device != self.0.keys.id() {
                return Err(Error::Integrity);
            }
            if value.family == self.0.account.identity().family
                && self
                    .0
                    .node
                    .as_ref()
                    .is_none_or(|node| value.nodes.contains(node))
                && value.name == self.0.name
            {
                *registration = Some(value.clone());
                return Ok(value.clone());
            }
        }
        let challenge: RegistrationChallenge = self
            .0
            .account
            .post("/api/devices/challenge", &())
            .await
            .map_err(account_error)?;
        if challenge.account != self.0.account.identity().account
            || challenge.family != self.0.account.identity().family
            || challenge.issuer != self.0.account.endpoints().api.trim_end_matches('/')
        {
            return Err(Error::Integrity);
        }
        let proof = self.0.keys.registration(
            challenge,
            self.0.name.clone(),
            self.0.node.clone(),
            current.as_ref().map(|entry| entry.generation),
        )?;
        let value: DeviceRegistration = self
            .0
            .account
            .post("/api/devices/register", &proof)
            .await
            .map_err(account_error)?;
        self.check(&value)?;
        if value.public_key != proof.public_key
            || value.device != self.0.keys.id()
            || value.family != self.0.account.identity().family
            || value.name != self.0.name
            || value.generation
                != current.as_ref().map_or(1, |entry| {
                    if entry.family == self.account().identity().family {
                        entry.generation
                    } else {
                        entry.generation.saturating_add(1)
                    }
                })
            || self
                .0
                .node
                .as_ref()
                .is_some_and(|node| !value.nodes.contains(node))
        {
            return Err(Error::Integrity);
        }
        *registration = Some(value.clone());
        Ok(value)
    }

    fn check(&self, value: &DeviceRegistration) -> Result<()> {
        if value.account != self.0.account.identity().account
            || value.device != DeviceId::of(&value.public_key)
            || value.generation == 0
            || !valid_label(&value.name)
            || value.nodes.iter().any(|node| !valid_node(node))
        {
            return Err(Error::Integrity);
        }
        Ok(())
    }

    /// Directory data comes from this owner's own authenticated account, not the peer label.
    pub async fn lookup(&self, id: DeviceId) -> Result<DeviceRegistration> {
        let value: Option<DeviceRegistration> = self
            .0
            .account
            .post("/api/devices/lookup", &serde_json::json!({"device":id}))
            .await
            .map_err(account_error)?;
        let value = value.ok_or(Error::Forbidden)?;
        self.check(&value)?;
        if value.device != id {
            return Err(Error::Integrity);
        }
        Ok(value)
    }

    /// Submit an already both-signed immutable intent. Never retries an uncertain write.
    pub async fn admit(&self, request: &SignedLink) -> Result<LinkReceipt> {
        let local = self.registered().await?;
        if request.intent.controller != local.device {
            return Err(Error::Forbidden);
        }
        let target = self.lookup(request.intent.target).await?;
        request.verify(&local, &target)?;
        let receipt: LinkReceipt = self
            .0
            .account
            .post("/api/devices/link", request)
            .await
            .map_err(|_| Error::Unknown)?;
        self.check_receipt(&receipt, &request.intent)?;
        Ok(receipt)
    }

    fn check_receipt(&self, receipt: &LinkReceipt, intent: &LinkIntent) -> Result<()> {
        if receipt.operation != intent.operation || receipt.intent_digest != digest(intent)? {
            return Err(Error::Integrity);
        }
        Ok(())
    }

    /// Read original admission state before retry, key cleanup or replacement.
    pub async fn recognize(&self, intent: &LinkIntent) -> Result<Option<LinkReceipt>> {
        let receipt: Option<LinkReceipt> = self
            .0
            .account
            .post(
                "/api/devices/recognize",
                &serde_json::json!({"operation":intent.operation}),
            )
            .await
            .map_err(account_error)?;
        if let Some(receipt) = &receipt {
            self.check_receipt(receipt, intent)?;
        }
        Ok(receipt)
    }

    /// Fence this endpoint's exact pending operation and recognize an already accepted result.
    pub async fn fence(&self, intent: LinkIntent) -> Result<LinkReceipt> {
        self.registered().await?;
        let mut request = LinkFence {
            intent,
            participant: self.0.keys.id(),
            signature: vec![],
        };
        request.signature = self.0.keys.sign(&request.signing_bytes()?);
        let receipt: LinkReceipt = self
            .0
            .account
            .post("/api/devices/fence", &request)
            .await
            .map_err(|_| Error::Unknown)?;
        self.check_receipt(&receipt, &request.intent)?;
        Ok(receipt)
    }

    /// Prepare the exact signed revocation before dispatch. The embedding first durably denies
    /// this LinkId locally and journals this request; transport failure never rolls that denial back.
    pub fn prepare_revocation(&self, link: &LinkGrant, operation: Uuid) -> Result<LinkRevocation> {
        if operation.is_nil()
            || link.revoked
            || link.intent.account != self.account().identity().account
            || (link.intent.controller != self.keys().id()
                && link.intent.target != self.keys().id())
        {
            return Err(Error::Forbidden);
        }
        let mut request = LinkRevocation {
            account: self.account().identity().account,
            issuer: self.account().endpoints().api.trim_end_matches('/').into(),
            operation,
            link: link.id,
            expected_generation: link.generation,
            participant: self.keys().id(),
            signature: vec![],
        };
        request.signature = self.keys().sign(&request.signing_bytes()?);
        Ok(request)
    }

    /// Submit the already journaled exact revocation once. An uncertain result requires recognition.
    pub async fn revoke(&self, request: &LinkRevocation) -> Result<LinkReceipt> {
        self.registered().await?;
        if request.participant != self.keys().id() {
            return Err(Error::Forbidden);
        }
        verify(
            &self.keys().public(),
            &request.signing_bytes()?,
            &request.signature,
        )?;
        let receipt: LinkReceipt = self
            .account()
            .post("/api/devices/revoke-link", request)
            .await
            .map_err(|_| Error::Unknown)?;
        if receipt.operation != request.operation
            || receipt.intent_digest != request.intent_digest()?
        {
            return Err(Error::Integrity);
        }
        Ok(receipt)
    }

    /// Recognize only the original signed revocation, without replaying it or lifting local denial.
    pub async fn recognize_revocation(
        &self,
        request: &LinkRevocation,
    ) -> Result<Option<LinkReceipt>> {
        let receipt: Option<LinkReceipt> = self
            .account()
            .post(
                "/api/devices/recognize",
                &serde_json::json!({"operation":request.operation}),
            )
            .await
            .map_err(account_error)?;
        if let Some(receipt) = &receipt
            && (receipt.operation != request.operation
                || receipt.intent_digest != request.intent_digest()?)
        {
            return Err(Error::Integrity);
        }
        Ok(receipt)
    }

    /// Resolve an admitted link under this account.
    pub async fn link(&self, id: Uuid) -> Result<LinkGrant> {
        let value: LinkGrant = self
            .0
            .account
            .post("/api/devices/link-state", &serde_json::json!({"link":id}))
            .await
            .map_err(account_error)?;
        if value.id != id || value.intent.account != self.0.account.identity().account {
            return Err(Error::Integrity);
        }
        Ok(value)
    }
}

#[cfg(feature = "native-keyring")]
impl CloudDevice {
    /// Load or create endpoint keys in an issuer/account/installation-qualified OS entry.
    /// The embedding must hold the installation profile writer lock for this owner.
    /// Persist the public endpoint identity before registration and supply it on every reopen.
    /// A retained identity whose protected key is missing is unavailable, never regenerated.
    /// A missing/locked/unsupported keyring is unavailable; there is no plaintext fallback.
    pub fn open(
        account: AccountSession,
        name: String,
        node: Option<String>,
        expected: Option<DeviceId>,
    ) -> Result<Self> {
        use zeroize::Zeroizing;
        let issuer = digest(&account.endpoints().api.trim_end_matches('/'))?
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let owner = account.identity();
        let entry = keyring::Entry::new(
            "swarmie.devices.v1",
            &format!("{issuer}/{}/{}", owner.account, owner.installation),
        )
        .map_err(|_| Error::Unavailable)?;
        let keys = match entry.get_secret() {
            Ok(bytes) => serde_json::from_slice::<EndpointKeys>(&Zeroizing::new(bytes))
                .map_err(|_| Error::Integrity)?,
            Err(keyring::Error::NoEntry) if expected.is_none() => {
                let keys = EndpointKeys::generate();
                let bytes = Zeroizing::new(serde_json::to_vec(&keys).map_err(|_| Error::Invalid)?);
                entry.set_secret(&bytes).map_err(|_| Error::Unavailable)?;
                keys
            }
            Err(_) => return Err(Error::Unavailable),
        };
        if expected.is_some_and(|id| id != keys.id()) {
            return Err(Error::Integrity);
        }
        Self::new(account, Rc::new(keys), name, node)
    }
}
