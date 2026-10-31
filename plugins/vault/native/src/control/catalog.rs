//! Installing catalog discovery never promotes an unadmitted replacement device.
use super::*;

fn admitted_replacement(root: &Root, catalog: &CatalogRoot, bundle: &TrustBundle) -> bool {
    let Some(pending) = &root.pending_replacement else {
        return false;
    };
    root.identity == catalog.identity
        && bundle.pins.identity == root.identity
        && !catalog.removed.contains(&pending.device)
        && catalog.certificates.iter().any(|certificate| {
            certificate.statement.recipient == pending.device
                && certificate.statement.signing_key == bundle.keys.signing_public()
                && certificate.statement.roles.contains(&Role::Decrypt)
                && canonical(&certificate.statement)
                    .is_ok_and(|bytes| digest(&bytes) == bundle.pins.statement_digest)
        })
}
impl Control {
    pub(super) fn install_catalog(&self, catalog: VaultCatalog) -> tembra::Result<()> {
        let candidates = self
            .configuration
            .borrow()
            .roots
            .iter()
            .filter_map(|root| {
                let pending = root.pending_replacement.as_ref()?;
                let remote = catalog
                    .roots
                    .iter()
                    .find(|remote| remote.identity == root.identity)?;
                let bundle = OsKeyring.load_bundle(root.identity, pending.device).ok()?;
                admitted_replacement(root, remote, &bundle)
                    .then_some((root.identity, pending.clone()))
            })
            .collect::<Vec<_>>();
        if !candidates.is_empty() {
            {
                let mut config = self.configuration.borrow_mut();
                for (identity, pending) in &candidates {
                    let root = config
                        .roots
                        .iter_mut()
                        .find(|root| root.identity == *identity)
                        .ok_or_else(unavailable)?;
                    root.previous_devices.push(root.device);
                    root.device = pending.device;
                    root.verified_code = Some(pending.verified_code.clone());
                    root.pending_replacement = None;
                }
            }
            self.persist()?;
            for (identity, _) in candidates {
                if let Some(active) = self
                    .attachments
                    .borrow_mut()
                    .remove(&identity)
                    .and_then(|weak| weak.upgrade())
                {
                    active.attachment.close();
                    active.human.close();
                }
            }
            if self
                .active
                .borrow()
                .as_ref()
                .is_some_and(|active| active.attachment.check().is_err())
            {
                self.active.borrow_mut().take();
            }
            self.tried_unlock.set(false);
        }
        *self.catalog.borrow_mut() = Some(catalog);
        Ok(())
    }
}
