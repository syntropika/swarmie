use super::*;
use swarmie_permissions::transport::TransportIdentity;

/// Native client side of one actually authenticated channel. TLS pins are supplied by enrollment,
/// never discovered from the peer. The embedding retains both byte carriers for this attachment.
pub struct NativeUnlockBroker {
    account: AccountSession,
    management: Rc<DeviceKeys>,
    node: UnlockChallenges,
    client_channel: ChannelBinding,
    node_channel: ChannelBinding,
    resume: Rc<TransportIdentity>,
}
impl NativeUnlockBroker {
    pub fn new(
        account: AccountSession,
        management: Rc<DeviceKeys>,
        node: UnlockChallenges,
        client_channel: ChannelBinding,
        node_channel: ChannelBinding,
    ) -> Result<Self> {
        if !client_channel.is_live()
            || !node_channel.is_live()
            || client_channel.exporter() != node_channel.exporter()
            || client_channel.local() != node_channel.peer()
            || client_channel.peer() != node_channel.local()
            || client_channel.local().as_slice() != management.signing_public()
        {
            return Err(Error::Forbidden);
        }
        Ok(Self {
            account,
            management,
            node,
            client_channel,
            node_channel,
            resume: Rc::new(TransportIdentity::generate()),
        })
    }
    pub fn resume_identity(&self) -> Rc<TransportIdentity> {
        self.resume.clone()
    }
}
impl UnlockBroker for NativeUnlockBroker {
    fn authorize<'a>(
        &'a self,
        request: &'a ReleaseRequest,
    ) -> LocalBoxFuture<'a, Result<AuthorizedUnlock>> {
        Box::pin(async move {
            if !self.client_channel.is_live()
                || request.family != self.account.identity().family
                || request.identity.account != self.account.identity().account
                || request.client != digest(&self.management.signing_public())
            {
                return Err(Error::Forbidden);
            }
            let challenge = self.node.issue(request, self.node_channel.clone())?;
            let intent = challenge.intent(self.resume.public().to_vec())?;
            if intent.exporter != self.client_channel.exporter()
                || intent.client != self.client_channel.local_fingerprint()
            {
                return Err(Error::Integrity);
            }
            let signature = self.management.sign(&canonical(&(
                "vault-client-ticket/v1",
                &intent,
                self.account.identity().family,
            ))?);
            let ticket: Ticket = self
                .account
                .post(
                    "/api/vault/tickets",
                    &serde_json::json!({"intent":intent,"signature":signature}),
                )
                .await
                .map_err(account_error)?;
            if !self.client_channel.is_live() {
                return Err(Error::Revoked);
            }
            if ticket.intent != intent {
                return Err(Error::Integrity);
            }
            challenge.confirm(ticket)
        })
    }
}
