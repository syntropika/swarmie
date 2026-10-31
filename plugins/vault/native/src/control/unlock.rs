//! Fresh hosted admission and the retained, single local journal owner.
use super::*;

impl Control {
    fn activate(&self, active: Rc<Active>) {
        let old = self.active.replace(Some(active.clone()));
        if let Some(old) = old
            && !Rc::ptr_eq(&old, &active)
        {
            old.attachment.close();
            old.human.close();
        }
        let previous_shell = self.shell.replace(active.human.clone());
        if !previous_shell.same_owner(&active.human) {
            previous_shell.close();
        }
    }

    pub(super) async fn unlock(&self, requested: Option<VaultIdentity>) -> tembra::Result<()> {
        if let Some(active) = self.active.borrow().as_ref()
            && requested.is_none_or(|identity| identity == active.node.identity())
            && active.attachment.check().is_ok()
            && active.human.is_live()
        {
            return Ok(());
        }
        if let Some(identity) = requested {
            let existing = self
                .attachments
                .borrow()
                .get(&identity)
                .and_then(std::rc::Weak::upgrade);
            if let Some(active) = existing
                && active.attachment.check().is_ok()
                && active.human.is_live()
                && self.account.borrow().as_ref().is_some_and(|account| {
                    account.is_live()
                        && active
                            .human
                            .identity()
                            .cloud
                            .as_ref()
                            .is_some_and(|cloud| cloud.family == account.identity().family)
                })
            {
                self.activate(active);
                return Ok(());
            }
        }
        if self.unlocking.replace(true) {
            return Err(unavailable());
        }
        let _unlock = Unlocking(&self.unlocking);
        let account = self.account()?;
        let catalog: Value = account
            .post("/api/vault/catalog", &())
            .await
            .map_err(failure)?;
        let current = catalog
            .get("current")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<Uuid>().ok());
        let root = self
            .configuration
            .borrow()
            .roots
            .iter()
            .find(|root| {
                root.identity.account == account.identity().account
                    && requested.map_or(Some(root.identity.vault) == current, |identity| {
                        root.identity == identity
                    })
            })
            .cloned()
            .ok_or_else(unavailable)?;
        let TrustBundle { keys, pins } = OsKeyring
            .load_bundle(root.identity, root.device)
            .map_err(failure)?;
        let keys = Rc::new(keys);
        let transport = keys.transport_identity();
        let public = transport.public();
        let (a, b) = tokio::io::duplex(16_384);
        let (client, server) = tokio::try_join!(
            ManagementStream::connect(a, &transport, public),
            ManagementStream::accept(b, &transport, public)
        )
        .map_err(failure)?;
        let broker = Rc::new(
            NativeUnlockBroker::new(
                account.clone(),
                keys.clone(),
                UnlockChallenges::new(root.identity, root.device, public, BTreeSet::from([public])),
                client.binding(),
                server.binding(),
            )
            .map_err(failure)?,
        );
        let cloud = Rc::new(
            CloudVault::new(
                account.clone(),
                root.identity,
                root.device,
                keys.clone(),
                broker,
            )
            .map_err(failure)?,
        );
        let cached = self
            .nodes
            .borrow()
            .get(&(root.identity, root.device))
            .cloned();
        let node = if let Some(node) = cached {
            node.begin_account(cloud.clone());
            node
        } else {
            let node = VaultNode::open(
                root.identity,
                root.device,
                keys.clone(),
                pins,
                Rc::new(
                    SqliteVaultStorage::open(
                        self.root.join(format!("vault-{}.sqlite3", root.device)),
                    )
                    .map_err(failure)?,
                ),
                cloud.clone(),
                self.authority.clone(),
            )
            .map_err(failure)?;
            self.nodes
                .borrow_mut()
                .insert((root.identity, root.device), node.clone());
            node
        };
        let human = self.authority.authenticate_shell(ShellIdentity {
            cloud: Some(CloudOwnerIdentity {
                client: digest(&keys.signing_public()),
                family: account.identity().family,
            }),
            account: Some(account.identity().account),
            installation: account.identity().installation,
            generation: Uuid::new_v4(),
        });
        let session = node.unlock(&human).await.map_err(failure)?;
        let attachment = node.attach(&session, server.binding()).map_err(failure)?;
        attachment
            .confirmation()
            .map_err(failure)?
            .verify(&client.binding(), session.admission())
            .map_err(failure)?;
        let ended = node.account_lifetime();
        let owner = human.clone();
        account.on_end(
            root.identity.vault,
            Box::new(move || {
                ended.close();
                owner.close();
            }),
        );
        let active = Rc::new(Active {
            node,
            session,
            human,
            connected: Cell::new(true),
            attachment,
            _client: client,
            _server: server,
        });
        self.attachments
            .borrow_mut()
            .insert(root.identity, Rc::downgrade(&active));
        self.activate(active);
        Ok(())
    }
}
struct Unlocking<'a>(&'a Cell<bool>);
impl Drop for Unlocking<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}
