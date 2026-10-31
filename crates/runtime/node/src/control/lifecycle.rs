//! Explicit private device ceremonies. Discovery never installs trust or grants access.
use super::*;
use crate::configuration::PendingJoin;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnrollmentRequest {
    identity: VaultIdentity,
    device: Uuid,
    signing: Vec<u8>,
    encryption: Vec<u8>,
}
impl EnrollmentRequest {
    fn code(&self) -> swarmie_vault::Result<String> {
        Ok(digest(&canonical(&("vault-device-request/v1", self))?)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect())
    }
}
/// The inner envelope is memory-only, including while awaiting independent human comparison.
pub(super) struct Offer {
    proposal: JoinProposal,
    envelope: Option<DeviceEnvelope>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Resolve {
        operation: Uuid,
    },
    Begin {
        identity: VaultIdentity,
    },
    Prepare {
        request: EnrollmentRequest,
        verified_request_code: String,
    },
    Accept {
        proposal: JoinProposal,
        verified_proposal_code: String,
    },
    Approve {
        offer: Uuid,
        verified_proposal_code: String,
    },
    CancelOffer {
        offer: Uuid,
    },
    CancelJoin {
        identity: VaultIdentity,
        device: Uuid,
    },
    Rebind {
        identity: VaultIdentity,
    },
    Select {
        identity: VaultIdentity,
    },
    Inspect {
        identity: VaultIdentity,
    },
    PreviewRemoval {
        identity: VaultIdentity,
        target: Uuid,
        compromised: bool,
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
impl Control {
    pub(super) fn lifecycle_human(
        &self,
        account: &AccountSession,
        client: Vec<u8>,
    ) -> HumanSession {
        self.authority.authenticate_shell(ShellIdentity {
            cloud: Some(CloudOwnerIdentity {
                client,
                family: account.identity().family,
            }),
            account: Some(account.identity().account),
            installation: self.configuration.borrow().installation,
            generation: Uuid::new_v4(),
        })
    }
    /// Only digest receipts survive restart. An uncertain ceremony is recognized, never replayed.
    pub(super) async fn submit_lifecycle(
        &self,
        command: LifecycleCommand,
        receipt: CeremonyReceipt,
    ) -> tembra::Result<Value> {
        let account = self.account()?;
        if self.configuration.borrow().ceremonies.len() >= 64 && !command.is_security_maintenance()
        {
            return Err(unavailable());
        }
        self.configuration
            .borrow_mut()
            .ceremonies
            .push(receipt.clone());
        self.persist()?;
        let outcome = match submit_ceremony(&account, &command, &receipt).await {
            Ok(outcome) => outcome,
            Err(_) => CommitOutcome::Unknown,
        };
        if outcome != CommitOutcome::Unknown {
            self.configuration
                .borrow_mut()
                .ceremonies
                .retain(|r| r.operation != receipt.operation);
            self.persist()?;
        }
        self.refresh_catalog(&account).await;
        Ok(json!({"operation":receipt.operation,"outcome":outcome}))
    }
    pub(super) async fn refresh_catalog(&self, account: &AccountSession) {
        if let Ok(catalog) = account
            .post::<VaultCatalog>("/api/vault/catalog", &())
            .await
        {
            let _ = self.install_catalog(catalog);
        }
    }
    pub(super) async fn recognize_lifecycle(&self, account: &AccountSession) {
        let receipts = self.configuration.borrow().ceremonies.clone();
        for receipt in receipts
            .into_iter()
            .filter(|r| r.identity.account == account.identity().account)
        {
            if let Ok(outcome) = recognize_ceremony(account, &receipt).await
                && outcome != CommitOutcome::Unknown
            {
                self.configuration
                    .borrow_mut()
                    .ceremonies
                    .retain(|r| r.operation != receipt.operation);
                let _ = self.persist();
            }
        }
    }
    pub(super) fn lifecycle_view(&self) -> tembra::Result<Value> {
        let config = self.configuration.borrow();
        let account = self
            .account
            .borrow()
            .as_ref()
            .map(|account| account.identity().account);
        let mut joins = vec![];
        for pending in config
            .joins
            .iter()
            .filter(|r| Some(r.identity.account) == account)
        {
            let keys = match OsKeyring.load_pending_device(pending.identity, pending.device) {
                Ok(keys) => keys,
                Err(_) => {
                    joins.push(json!({"request":{"identity":pending.identity,"device":pending.device,"signing":[],"encryption":[]},"code":"","keysUnavailable":true}));
                    continue;
                }
            };
            let request = EnrollmentRequest {
                identity: pending.identity,
                device: pending.device,
                signing: keys.signing_public(),
                encryption: keys.encryption_public().map_err(failure)?,
            };
            joins.push(json!({"request":request,"code":request.code().map_err(failure)?}));
        }
        let mut pages = json!({});
        let offers=self.page("offers",self.offers.borrow().iter().map(|(id,offer)| Ok(json!({"id":id,"proposal":offer.proposal,"code":offer.proposal.verification_code().map_err(failure)?}))).collect::<tembra::Result<_>>()?,&mut pages)?;
        let active_identity = self
            .active
            .borrow()
            .as_ref()
            .map(|active| active.node.identity());
        let active_identity = self
            .maintenance_identity
            .get()
            .or(active_identity)
            .or_else(|| {
                self.catalog.borrow().as_ref().and_then(|catalog| {
                    catalog
                        .roots
                        .iter()
                        .find(|root| Some(root.identity.vault) == catalog.current)
                        .map(|root| root.identity)
                })
            });
        let mut roots = vec![];
        if let Some(catalog) = self.catalog.borrow().as_ref() {
            for root in &catalog.roots {
                let certificates = if active_identity == Some(root.identity) {
                    self.page(
                        "devices",
                        root.certificates
                            .iter()
                            .filter(|certificate| {
                                !root.removed.contains(&certificate.statement.recipient)
                            })
                            .map(|certificate| serde_json::to_value(certificate).map_err(failure))
                            .collect::<tembra::Result<_>>()?,
                        &mut pages,
                    )?
                } else {
                    json!([])
                };
                roots.push(json!({"identity":root.identity,"exposed":root.exposed,"certificates":certificates,"removed":root.removed,"enrolled":config.roots.iter().any(|local|local.identity==root.identity),"verifiedCode":config.roots.iter().find(|local|local.identity==root.identity).and_then(|local|local.verified_code.clone()),"pendingReplacement":config.roots.iter().find(|local|local.identity==root.identity).and_then(|local|local.pending_replacement.as_ref()).map(|pending|json!({"device":pending.device,"verifiedCode":pending.verified_code}))}));
            }
        }
        Ok(
            json!({"joins":joins,"offers":offers,"roots":roots,"pages":pages,"removal":*self.removal_preview.borrow(),"pending":config.ceremonies.iter().filter(|r|Some(r.identity.account)==account).collect::<Vec<_>>()}),
        )
    }
    pub(super) async fn lifecycle(&self, request: Request) -> tembra::Result<Value> {
        let account = self.account()?;
        match request {
            Request::Resolve { operation } => {
                let receipt = self
                    .configuration
                    .borrow()
                    .ceremonies
                    .iter()
                    .find(|receipt| {
                        receipt.operation == operation
                            && receipt.identity.account == account.identity().account
                    })
                    .cloned()
                    .ok_or_else(unavailable)?;
                let bundle = OsKeyring
                    .load_bundle(receipt.identity, receipt.device)
                    .map_err(failure)?;
                let outcome = fence_ceremony(&account, &receipt, &bundle.keys)
                    .await
                    .map_err(failure)?;
                if outcome != CommitOutcome::Unknown {
                    self.configuration
                        .borrow_mut()
                        .ceremonies
                        .retain(|receipt| receipt.operation != operation);
                    self.persist()?;
                }
                self.refresh_catalog(&account).await;
                Ok(json!({"operation":operation,"outcome":outcome}))
            }
            Request::Begin { identity } => {
                if self
                    .configuration
                    .borrow()
                    .joins
                    .iter()
                    .any(|join| join.identity == identity)
                    || self
                        .configuration
                        .borrow()
                        .roots
                        .iter()
                        .any(|root| root.identity == identity && root.pending_replacement.is_some())
                    || identity.account != account.identity().account
                    || self.configuration.borrow().joins.len() >= 8
                {
                    return Err(unavailable());
                }
                if !self.catalog.borrow().as_ref().is_some_and(|catalog| {
                    catalog.roots.iter().any(|root| root.identity == identity)
                }) {
                    return Err(unavailable());
                }
                let device = Uuid::new_v4();
                let keys = DeviceKeys::generate();
                OsKeyring
                    .save_pending_device(identity, device, &keys)
                    .map_err(failure)?;
                self.configuration
                    .borrow_mut()
                    .joins
                    .push(PendingJoin { identity, device });
                self.persist()?;
                let request = EnrollmentRequest {
                    identity,
                    device,
                    signing: keys.signing_public(),
                    encryption: keys.encryption_public().map_err(failure)?,
                };
                Ok(json!({"request":request,"code":request.code().map_err(failure)?}))
            }
            Request::Prepare {
                request,
                verified_request_code,
            } => {
                let active = self.active()?;
                if request.identity != active.node.identity()
                    || request.code().map_err(failure)? != verified_request_code
                    || self.offers.borrow().len() >= 8
                {
                    return Err(unavailable());
                }
                active
                    .node
                    .refresh_current(&active.session)
                    .await
                    .map_err(failure)?;
                let (proposal, envelope) = active
                    .node
                    .propose_enrollment(
                        &active.session,
                        request.device,
                        request.signing,
                        Some(request.encryption),
                        BTreeSet::from([Role::Decrypt, Role::Manage, Role::EnrollDevices]),
                    )
                    .map_err(failure)?;
                let code = proposal.verification_code().map_err(failure)?;
                let id = Uuid::new_v4();
                let result = json!({"id":id,"proposal":proposal,"code":code});
                self.offers
                    .borrow_mut()
                    .insert(id, Offer { proposal, envelope });
                Ok(result)
            }
            Request::Accept {
                proposal,
                verified_proposal_code,
            } => {
                if proposal.verification_code().map_err(failure)? != verified_proposal_code {
                    return Err(unavailable());
                }
                let identity = proposal.certificate.statement.identity;
                let device = proposal.certificate.statement.recipient;
                if identity.account != account.identity().account
                    || !self
                        .configuration
                        .borrow()
                        .joins
                        .iter()
                        .any(|join| join.identity == identity && join.device == device)
                {
                    return Err(unavailable());
                }
                if let Some(existing) = self
                    .configuration
                    .borrow()
                    .roots
                    .iter()
                    .find(|root| root.identity == identity)
                    && let Ok(pinned) = OsKeyring.load_bundle(identity, existing.device)
                    && pinned.pins.root != proposal.root
                {
                    return Err(unavailable());
                }
                let keys = OsKeyring
                    .load_pending_device(identity, device)
                    .map_err(failure)?;
                let human = self.lifecycle_human(&account, digest(&keys.signing_public()));
                let decision = human
                    .decide(&("vault-accept-enrollment/v1", &verified_proposal_code))
                    .map_err(failure)?;
                let bundle = proposal.accept(&human, decision, keys).map_err(failure)?;
                OsKeyring
                    .save_bundle(identity, device, &bundle)
                    .map_err(failure)?;
                {
                    let mut config = self.configuration.borrow_mut();
                    if let Some(root) = config
                        .roots
                        .iter_mut()
                        .find(|root| root.identity == identity)
                    {
                        root.pending_replacement = Some(crate::configuration::AcceptedDevice {
                            device,
                            verified_code: verified_proposal_code.clone(),
                        });
                    } else {
                        config.roots.push(Root {
                            identity,
                            device,
                            previous_devices: vec![],
                            pending_replacement: None,
                            pending: None,
                            verified_code: Some(verified_proposal_code.clone()),
                        });
                    }
                    config.joins.retain(|join| join.device != device);
                }
                self.persist()?;
                let _ = OsKeyring.forget_pending_device(identity, device);
                human.close();
                self.tried_unlock.set(false);
                Ok(json!({"accepted":true,"waitingForApprover":true}))
            }
            Request::Approve {
                offer,
                verified_proposal_code,
            } => {
                let active = self.active()?;
                active
                    .node
                    .refresh_current(&active.session)
                    .await
                    .map_err(failure)?;
                {
                    let offers = self.offers.borrow();
                    let offer = offers.get(&offer).ok_or_else(unavailable)?;
                    if offer.proposal.verification_code().map_err(failure)?
                        != verified_proposal_code
                    {
                        return Err(unavailable());
                    }
                }
                let offer = self
                    .offers
                    .borrow_mut()
                    .remove(&offer)
                    .ok_or_else(unavailable)?;
                let decision = active
                    .human
                    .decide(&("vault-approve-enrollment/v1", &verified_proposal_code))
                    .map_err(failure)?;
                let (command, receipt) = active
                    .node
                    .approve_enrollment(&active.session, decision, offer.proposal, offer.envelope)
                    .map_err(failure)?;
                self.submit_lifecycle(command, receipt).await
            }
            Request::CancelOffer { offer } => {
                self.offers.borrow_mut().remove(&offer);
                Ok(Value::Null)
            }
            Request::CancelJoin { identity, device } => {
                if identity.account != account.identity().account {
                    return Err(unavailable());
                }
                self.configuration
                    .borrow_mut()
                    .joins
                    .retain(|join| join.identity != identity || join.device != device);
                {
                    let mut config = self.configuration.borrow_mut();
                    if let Some(root) = config.roots.iter_mut().find(|root| {
                        root.identity == identity
                            && root
                                .pending_replacement
                                .as_ref()
                                .is_some_and(|pending| pending.device == device)
                    }) {
                        root.previous_devices.push(device);
                        root.pending_replacement = None;
                    }
                }
                self.persist()?;
                let _ = OsKeyring.forget_pending_device(identity, device);
                Ok(Value::Null)
            }
            Request::Rebind { identity } => {
                let root = self
                    .configuration
                    .borrow()
                    .roots
                    .iter()
                    .find(|root| {
                        root.identity == identity
                            && root.identity.account == account.identity().account
                    })
                    .cloned()
                    .ok_or_else(unavailable)?;
                let bundle = OsKeyring
                    .load_bundle(identity, root.device)
                    .map_err(failure)?;
                let active = self.active.borrow().clone();
                let retained = active.as_ref().filter(|active| {
                    active.node.identity() == identity
                        && active.human.is_live()
                        && active.human.identity().cloud.as_ref().is_some_and(|cloud| {
                            cloud.family == account.identity().family
                                && cloud.client == digest(&bundle.keys.signing_public())
                        })
                });
                let human = retained.map_or_else(
                    || self.lifecycle_human(&account, digest(&bundle.keys.signing_public())),
                    |active| active.human.clone(),
                );
                let decision = human
                    .decide(&(
                        "vault-rebind/v1",
                        identity,
                        root.device,
                        account.identity().family,
                    ))
                    .map_err(failure)?;
                let (command, receipt) = rebind(
                    &human,
                    decision,
                    identity,
                    root.device,
                    &bundle.keys,
                    account.identity().family,
                )
                .map_err(failure)?;
                let result = self.submit_lifecycle(command, receipt).await;
                if retained.is_none() {
                    human.close();
                }
                self.tried_unlock.set(false);
                result
            }
            Request::Select { identity } => {
                self.unlock(Some(identity)).await?;
                Ok(Value::Null)
            }
            Request::Inspect { identity } => {
                self.maintenance_custody(identity)?;
                self.maintenance_identity.set(Some(identity));
                self.refresh_catalog(&account).await;
                Ok(Value::Null)
            }
            Request::PreviewRemoval {
                identity,
                target,
                compromised,
            } => self.preview_removal(identity, target, compromised).await,
            Request::Remove {
                identity,
                target,
                compromised,
                affected,
                authority_base,
            } => {
                self.confirm_removal(RemovalIntent {
                    identity,
                    target,
                    compromised,
                    affected,
                    authority_base,
                })
                .await
            }
            Request::Restore {
                identity,
                expected_current,
            } => self.confirm_restore(identity, expected_current).await,
        }
    }
}
