//! Security maintenance uses protected signing keys and fresh Cloud authority, without V or tickets.
use super::*;

impl Control {
    pub(super) fn maintenance_custody(
        &self,
        identity: VaultIdentity,
    ) -> tembra::Result<(Root, TrustBundle)> {
        let account = self.account()?;
        let root = self
            .configuration
            .borrow()
            .roots
            .iter()
            .find(|root| {
                root.identity == identity
                    && identity.account == account.identity().account
                    && root.pending.is_none()
            })
            .cloned()
            .ok_or_else(unavailable)?;
        let bundle = OsKeyring
            .load_bundle(identity, root.device)
            .map_err(failure)?;
        Ok((root, bundle))
    }
    async fn maintenance_check(
        &self,
        root: &Root,
        bundle: &TrustBundle,
    ) -> tembra::Result<AuthorityCheck> {
        let account = self.account()?;
        let check: AuthorityCheck = account
            .post("/api/vault/check", &json!({"identity":root.identity}))
            .await
            .map_err(failure)?;
        if check.identity != root.identity
            || check.actor_family != account.identity().family
            || !check.devices.iter().any(|device| {
                device.device == root.device
                    && !device.removed
                    && device.family == Some(account.identity().family)
                    && device.fingerprint == digest(&bundle.keys.signing_public())
                    && device.roles.contains(&Role::EnrollDevices)
            })
        {
            return Err(unavailable());
        }
        Ok(check)
    }
    fn maintenance_human(
        &self,
        identity: VaultIdentity,
        keys: &DeviceKeys,
    ) -> tembra::Result<(HumanSession, bool)> {
        let account = self.account()?;
        let active = self.active.borrow().clone();
        if let Some(active) = active
            && active.node.identity() == identity
            && active.human.is_live()
            && active.human.identity().cloud.as_ref().is_some_and(|cloud| {
                cloud.family == account.identity().family
                    && cloud.client == digest(&keys.signing_public())
            })
        {
            return Ok((active.human.clone(), false));
        }
        Ok((
            self.lifecycle_human(&account, digest(&keys.signing_public())),
            true,
        ))
    }
    pub(super) async fn preview_removal(
        &self,
        identity: VaultIdentity,
        target: Uuid,
        compromised: bool,
    ) -> tembra::Result<Value> {
        let (local, bundle) = self.maintenance_custody(identity)?;
        let check = self.maintenance_check(&local, &bundle).await?;
        self.refresh_catalog(&self.account()?).await;
        let catalog = self.catalog.borrow();
        let root = catalog
            .as_ref()
            .and_then(|catalog| catalog.roots.iter().find(|root| root.identity == identity))
            .ok_or_else(unavailable)?;
        if !check
            .devices
            .iter()
            .any(|device| device.device == target && !device.removed)
        {
            return Err(unavailable());
        }
        let mut affected = BTreeSet::from([target]);
        if compromised {
            for _ in 0..root.certificates.len() {
                let keys = root
                    .certificates
                    .iter()
                    .filter(|certificate| affected.contains(&certificate.statement.recipient))
                    .map(|certificate| certificate.statement.signing_key.clone())
                    .collect::<BTreeSet<_>>();
                let before = affected.len();
                for certificate in &root.certificates {
                    if keys.contains(&certificate.statement.approver) {
                        affected.insert(certificate.statement.recipient);
                    }
                }
                if before == affected.len() {
                    break;
                }
            }
        }
        *self.removal_preview.borrow_mut() = Some(RemovalIntent {
            identity,
            target,
            compromised,
            affected,
            authority_base: check.sequence,
        });
        Ok(Value::Null)
    }
    pub(super) async fn confirm_removal(&self, intent: RemovalIntent) -> tembra::Result<Value> {
        if self.removal_preview.borrow().as_ref() != Some(&intent) {
            return Err(unavailable());
        }
        let (root, bundle) = self.maintenance_custody(intent.identity)?;
        let check = self.maintenance_check(&root, &bundle).await?;
        if check.sequence != intent.authority_base {
            return Err(unavailable());
        }
        let (human, temporary) = self.maintenance_human(intent.identity, &bundle.keys)?;
        let decision = human
            .decide(&("vault-remove-enrollment/v1", &intent))
            .map_err(failure)?;
        let (command, receipt) =
            remove_enrollment_with_key(&human, decision, intent, root.device, &bundle.keys)
                .map_err(failure)?;
        let result = self.submit_lifecycle(command, receipt).await;
        if temporary {
            human.close();
        }
        result
    }
    pub(super) async fn confirm_restore(
        &self,
        identity: VaultIdentity,
        expected_current: Uuid,
    ) -> tembra::Result<Value> {
        let (root, bundle) = self.maintenance_custody(identity)?;
        let check = self.maintenance_check(&root, &bundle).await?;
        if check.current != Some(expected_current) || identity.vault == expected_current {
            return Err(unavailable());
        }
        let (human, temporary) = self.maintenance_human(identity, &bundle.keys)?;
        let decision = human
            .decide(&("vault-restore/v1", identity, expected_current))
            .map_err(failure)?;
        let (command, receipt) = restore_archived_with_key(
            &human,
            decision,
            identity,
            expected_current,
            root.device,
            &bundle.keys,
        )
        .map_err(failure)?;
        let result = self.submit_lifecycle(command, receipt).await;
        if temporary {
            human.close();
        }
        result
    }
}
