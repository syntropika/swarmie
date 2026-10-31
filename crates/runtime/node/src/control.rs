mod catalog;
pub(crate) mod client;
mod composition;
mod decisions;
mod rpc;
mod unlock;
pub(super) use rpc::orb;
mod lifecycle;
mod maintenance;
mod projection;
mod reconciliation;
mod recovery;
pub use composition::{NativeAgent, NativeExecution, NativeNode};

use crate::configuration::{self, Configuration, Root};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    path::PathBuf,
    rc::Rc,
};
use swarmie_cloud::account::{
    AccountSession, AccountTransport, Endpoints, KeyringSessions, LoginFlow,
};
use swarmie_permissions::transport::ManagementStream;
use swarmie_permissions::*;
use swarmie_providers::protection::{Detector, ModelGuard};
use swarmie_storage::sqlite::SqliteVaultStorage;
use swarmie_vault::{
    cloud_http::*,
    keyring::{OsKeyring, ProtectedStorage},
    *,
};
use tembra::{Context, Orb};
use uuid::Uuid;
use zeroize::Zeroize;

pub const SERVICE: &str = "swarmie.vault.control";
pub const PRINCIPAL: &str = "desktop-shell";
pub(crate) fn failure<T>(_: T) -> tembra::Error {
    tembra::Error::new(
        tembra::Code::Unavailable,
        "swarmie.vault.control",
        "Private operation could not finish; reconcile its status before retrying.",
    )
}
fn unavailable() -> tembra::Error {
    failure("unavailable")
}

struct Active {
    node: VaultNode,
    session: VaultSession,
    human: HumanSession,
    connected: Cell<bool>,
    attachment: ManagementAttachment,
    _client: ManagementStream<tokio::io::DuplexStream>,
    _server: ManagementStream<tokio::io::DuplexStream>,
}

struct OwnerLock(std::fs::File);

impl Drop for OwnerLock {
    fn drop(&mut self) {
        // Forked descriptors share the lock until exec. The settled owner must
        // release it explicitly instead of waiting for an unrelated child.
        let _ = self.0.unlock();
    }
}

pub struct Control {
    pub cards: crate::cards::Cards,
    pub jobs: RefCell<Vec<swarmie_vault::executors::CaptureJob>>,
    root: PathBuf,
    entries: PrivateEntries,
    private_decisions_enabled: bool,
    configuration: RefCell<Configuration>,
    authority: Authority,
    account: RefCell<Option<AccountSession>>,
    catalog: RefCell<Option<VaultCatalog>>,
    tried_unlock: Cell<bool>,
    unlocking: Cell<bool>,
    reconcile_wake: tokio::sync::Notify,
    reconcile_serial: tokio::sync::Mutex<()>,
    paging: RefCell<std::collections::BTreeMap<String, usize>>,
    maintenance_identity: Cell<Option<VaultIdentity>>,
    removal_preview: RefCell<Option<RemovalIntent>>,
    offers: RefCell<std::collections::BTreeMap<Uuid, lifecycle::Offer>>,
    login_generation: Cell<Uuid>,
    logging_in: Cell<bool>,
    needs_sign_in: Cell<bool>,
    nodes: RefCell<std::collections::BTreeMap<(VaultIdentity, Uuid), VaultNode>>,
    attachments: RefCell<std::collections::BTreeMap<VaultIdentity, std::rc::Weak<Active>>>,
    _owner: OwnerLock,
    detector_available: bool,
    active: RefCell<Option<Rc<Active>>>,
    shell: RefCell<HumanSession>,
    pub guard: ModelGuard,
    grant: PluginGrant,
    tool_grant: PluginGrant,
}
impl Control {
    pub fn open(root: PathBuf, entries: PrivateEntries) -> tembra::Result<Rc<Self>> {
        let configuration = configuration::load(&root).map_err(failure)?;
        let owner = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join("vault-owner.lock"))
            .map_err(failure)?;
        owner.try_lock().map_err(failure)?;
        let owner = OwnerLock(owner);
        configuration::save(&root, &configuration).map_err(failure)?;
        let authority = Authority::new();
        let shell = authority.authenticate_shell(ShellIdentity {
            cloud: None,
            account: None,
            installation: configuration.installation,
            generation: Uuid::new_v4(),
        });
        let detector = Detector::bundled();
        let detector_available = detector.is_some();
        let guard = ModelGuard::new(
            "core.model-protection".into(),
            PluginGrant::bundled(CorePlugin::ModelProtection, "core.model-protection".into()),
            detector,
        );
        if !configuration.detector_enabled {
            let decision = shell
                .decide(&("model-protection-policy/v1", guard.generation(), false))
                .map_err(failure)?;
            // Restoring an already accepted native preference is not a new user approval.
            guard.configure(&shell, decision, false).map_err(failure)?;
        }
        let account = match (configuration.endpoints.clone(), configuration.account) {
            (Some(endpoints), Some(account)) => AccountTransport::new(endpoints, None)
                .ok()
                .and_then(|transport| {
                    AccountSession::reopen(
                        transport,
                        Rc::new(KeyringSessions),
                        account,
                        configuration.installation,
                    )
                    .ok()
                }),
            _ => None,
        };
        // Account custody failures do not disable the independently enabled final model guard.
        let needs_sign_in = configuration.account.is_some() && account.is_none();
        Ok(Rc::new(Self {
            cards: crate::cards::Cards::default(),
            jobs: RefCell::new(vec![]),
            root,
            entries,
            private_decisions_enabled: std::env::var_os("NATIVE_SDK_SESSION_RECORD").is_none()
                && std::env::var_os("NATIVE_SDK_SESSION_REPLAY").is_none(),
            configuration: RefCell::new(configuration),
            authority,
            account: RefCell::new(account),
            catalog: RefCell::new(None),
            tried_unlock: Cell::new(false),
            unlocking: Cell::new(false),
            reconcile_wake: tokio::sync::Notify::new(),
            reconcile_serial: tokio::sync::Mutex::new(()),
            paging: RefCell::new(std::collections::BTreeMap::new()),
            maintenance_identity: Cell::new(None),
            removal_preview: RefCell::new(None),
            offers: RefCell::new(std::collections::BTreeMap::new()),
            login_generation: Cell::new(Uuid::new_v4()),
            logging_in: Cell::new(false),
            needs_sign_in: Cell::new(needs_sign_in),
            nodes: RefCell::new(std::collections::BTreeMap::new()),
            attachments: RefCell::new(std::collections::BTreeMap::new()),
            _owner: owner,
            detector_available,
            active: RefCell::new(None),
            shell: RefCell::new(shell),
            guard,
            grant: PluginGrant::bundled(CorePlugin::Vault, "core.vault".into()),
            tool_grant: PluginGrant::bundled(
                CorePlugin::Vault,
                swarmie_vault::tools::SERVICE.into(),
            ),
        }))
    }
    fn persist(&self) -> tembra::Result<()> {
        configuration::save(&self.root, &self.configuration.borrow()).map_err(failure)
    }

    pub(crate) fn private_decisions_available(&self) -> bool {
        self.private_decisions_enabled
    }
    pub(crate) fn owner_for(
        &self,
        execution: &ExecutionBinding,
    ) -> swarmie_vault::Result<OwnerBinding> {
        let shell = self.shell.borrow();
        if !execution.is_live()
            || !shell.is_live()
            || shell.identity().account != execution.identity().account
        {
            return Err(swarmie_vault::Error::Revoked);
        }
        if !execution.is_live() {
            return Err(swarmie_vault::Error::Revoked);
        }
        self.authority
            .check_binding(execution.shell())
            .map_err(swarmie_vault::Error::from)?;
        Ok(execution.shell().clone())
    }
    pub(crate) fn model_owner_for(
        &self,
        execution: &ExecutionBinding,
    ) -> swarmie_vault::Result<OwnerBinding> {
        if !execution.is_live() {
            return Err(swarmie_vault::Error::Revoked);
        }
        self.authority
            .check_binding(execution.shell())
            .map_err(swarmie_vault::Error::from)?;
        if execution.shell().identity().installation != self.configuration.borrow().installation {
            return Err(swarmie_vault::Error::Forbidden);
        }
        Ok(execution.shell().clone())
    }
    pub(crate) fn conversation_allowed(&self, intent: &UseIntent) -> bool {
        if !intent.execution.main_agent || intent.execution.conversation.is_none() {
            return false;
        }
        self.active.borrow().as_ref().is_some_and(|active| {
            active
                .node
                .list(&active.session, &self.grant)
                .ok()
                .is_some_and(|records| {
                    records.iter().any(|r| {
                        r.secret == intent.reference.secret
                            && r.metadata.approval == Approval::Conversation
                    })
                })
        })
    }
    fn account(&self) -> tembra::Result<AccountSession> {
        self.account
            .borrow()
            .clone()
            .filter(AccountSession::is_live)
            .ok_or_else(unavailable)
    }
    fn active(&self) -> tembra::Result<Rc<Active>> {
        let active = self.active.borrow().clone().ok_or_else(unavailable)?;
        active.attachment.check().map_err(failure)?;
        Ok(active)
    }
    pub fn close(&self) {
        self.login_generation.set(Uuid::new_v4());
        self.logging_in.set(false);
        self.offers.borrow_mut().clear();
        self.removal_preview.borrow_mut().take();
        self.maintenance_identity.set(None);
        self.paging.borrow_mut().clear();
        self.cards.cancel_all();
        for node in self.nodes.borrow().values() {
            node.end_account();
        }
        let active = self.active.borrow_mut().take();
        if let Some(active) = active {
            active.attachment.close();
            active.node.end_account();
            active.human.close();
        }
        self.attachments.borrow_mut().clear();
        self.shell.borrow().close();
        self.authority.end_all();
    }
    /// Only a matching provisional founding may be removed after a definitive non-admission.
    /// Existing roots, previous device locators, journals and readable archives are preserved.
    fn settle_founding(
        &self,
        receipt: &CeremonyReceipt,
        outcome: CommitOutcome,
    ) -> tembra::Result<()> {
        if outcome == CommitOutcome::Unknown {
            return Ok(());
        }
        let discarded = {
            let mut config = self.configuration.borrow_mut();
            let position = config
                .roots
                .iter()
                .position(|root| root.pending.as_ref() == Some(receipt))
                .ok_or_else(unavailable)?;
            if outcome == CommitOutcome::Accepted {
                config.roots[position].pending = None;
                None
            } else {
                Some(config.roots.remove(position))
            }
        };
        self.persist()?;
        if let Some(root) = discarded {
            OsKeyring
                .forget_bundle(root.identity, root.device)
                .map_err(failure)?;
        }
        Ok(())
    }
    async fn recognize_foundings(&self, account: &AccountSession) {
        let pending: Vec<_> = self
            .configuration
            .borrow()
            .roots
            .iter()
            .filter(|root| {
                root.identity.account == account.identity().account && root.pending.is_some()
            })
            .cloned()
            .collect();
        for root in pending {
            let Some(receipt) = root.pending else {
                continue;
            };
            if let Ok(outcome) = recognize_ceremony(account, &receipt).await
                && outcome != CommitOutcome::Unknown
            {
                let _ = self.settle_founding(&receipt, outcome);
            }
        }
    }
    async fn refresh(&self) -> bool {
        let _serial = self.reconcile_serial.lock().await;
        let generation = self.login_generation.get();
        let mut online = false;
        for job in self.jobs.borrow().iter() {
            let _ = job.retry_persistence();
        }
        let account = self.account.borrow().clone();
        if let Some(account) = account.filter(AccountSession::is_live) {
            self.needs_sign_in.set(account.needs_sign_in().await);
            self.recognize_lifecycle(&account).await;
            self.recognize_foundings(&account).await;
            if let Ok(catalog) = account
                .post::<VaultCatalog>("/api/vault/catalog", &())
                .await
            {
                if self.login_generation.get() != generation {
                    return false;
                }
                online = self.install_catalog(catalog).is_ok();
            }
            if self.login_generation.get() != generation {
                return false;
            }
            let active = self.active.borrow().clone();
            if let Some(active) = active {
                for mutation in active
                    .node
                    .pending_mutations(&active.session, &self.grant)
                    .unwrap_or_default()
                {
                    let _ = active
                        .node
                        .recognize(&active.session, mutation.prepared_entry)
                        .await;
                }
                active.connected.set(
                    active
                        .node
                        .refresh_authority(&active.session)
                        .await
                        .unwrap_or(false),
                );
            } else if !self.tried_unlock.replace(true) {
                let selected = self
                    .catalog
                    .borrow()
                    .as_ref()
                    .and_then(|catalog| catalog.current);
                if self
                    .configuration
                    .borrow()
                    .roots
                    .iter()
                    .any(|root| Some(root.identity.vault) == selected && root.pending.is_none())
                {
                    let _ = self.unlock(None).await;
                }
            }
        }
        online
    }
    async fn login(self: Rc<Self>, endpoints: Endpoints) -> tembra::Result<String> {
        if self
            .account
            .borrow()
            .as_ref()
            .is_some_and(|account| !account.is_live())
        {
            self.close();
            self.account.borrow_mut().take();
            self.active.borrow_mut().take();
            self.catalog.borrow_mut().take();
            self.tried_unlock.set(false);
            *self.shell.borrow_mut() = self.authority.authenticate_shell(ShellIdentity {
                cloud: None,
                account: None,
                installation: self.configuration.borrow().installation,
                generation: Uuid::new_v4(),
            });
        }
        if self.active.borrow().is_some()
            || self.account.borrow().is_some()
            || self.logging_in.get()
        {
            return Err(unavailable());
        }
        let transport = AccountTransport::new(endpoints.clone(), None).map_err(failure)?;
        let installation = self.configuration.borrow().installation;
        let generation = Uuid::new_v4();
        self.login_generation.set(generation);
        self.logging_in.set(true);
        let flow = LoginFlow::begin(transport, Rc::new(KeyringSessions), installation).await;
        if self.login_generation.get() != generation {
            return Err(unavailable());
        }
        let flow = match flow {
            Ok(flow) => flow,
            Err(error) => {
                self.logging_in.set(false);
                return Err(failure(error));
            }
        };
        let url = flow.authorization_url().to_owned();
        // The native owner retains the PKCE flow; a closed settings view cannot replay code redemption.
        tokio::task::spawn_local(async move {
            let permitted = || self.login_generation.get() == generation;
            let result = flow.finish_if(permitted).await;
            if self.login_generation.get() != generation {
                return;
            }
            self.logging_in.set(false);
            if let Ok(account) = result {
                {
                    let mut config = self.configuration.borrow_mut();
                    config.endpoints = Some(endpoints);
                    config.account = Some(account.identity().account);
                }
                if self.persist().is_ok() {
                    *self.account.borrow_mut() = Some(account);
                    self.needs_sign_in.set(false);
                    self.tried_unlock.set(false);
                    self.reconcile_wake.notify_one();
                } else {
                    let _ = account.logout().await;
                    self.configuration.borrow_mut().account = None;
                    self.needs_sign_in.set(true);
                }
            }
        });
        Ok(url)
    }
}

#[derive(Deserialize)]
#[serde(transparent)]
struct PrivateText(String);
impl Drop for PrivateText {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}
fn native_bash() -> PathBuf {
    #[cfg(unix)]
    {
        PathBuf::from("/bin/bash")
    }
    #[cfg(not(unix))]
    {
        PathBuf::new()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn dropping_the_last_private_owner_releases_an_inherited_file_lock() {
        let root = tempfile::tempdir().unwrap();
        let control = Control::open(root.path().into(), PrivateEntries::default()).unwrap();
        // A fork retains this same open-file description until exec closes it.
        let inherited = control._owner.0.try_clone().unwrap();
        assert!(Control::open(root.path().into(), PrivateEntries::default()).is_err());
        drop(control);
        let reopened = Control::open(root.path().into(), PrivateEntries::default());
        assert!(
            reopened.is_ok(),
            "the settled owner must release its lock before a child's exec: {:?}",
            reopened.err()
        );
        drop(inherited);
    }
}
