//! Native owner-issued human, execution and plugin permission capabilities.
//! Serializable identifiers are metadata only; no public wire call may mint these handles.
use serde::Serialize as SerializeValue;
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum Error {
    #[error("private owner authority is required")]
    Forbidden,
    #[error("owner authority ended or changed")]
    Revoked,
    #[error("invalid authority intent")]
    Invalid,
}

pub type Result<T> = std::result::Result<T, Error>;

fn canonical(value: &impl SerializeValue) -> Result<Vec<u8>> {
    serde_jcs::to_vec(value).map_err(|_| Error::Invalid)
}
fn digest(value: &[u8]) -> Vec<u8> {
    Sha256::digest(value).to_vec()
}

use serde::{Deserialize, Serialize};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    rc::{Rc, Weak},
};
use uuid::Uuid;

/// Account authority obtained by the native session owner, never a caller installation label.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CloudOwnerIdentity {
    pub client: Vec<u8>,
    pub family: Uuid,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ShellIdentity {
    pub cloud: Option<CloudOwnerIdentity>,
    pub account: Option<Uuid>,
    pub installation: Uuid,
    pub generation: Uuid,
}

/// Constructed only by the embedding after authenticating its private shell channel.
/// No RPC method exposes this issuer, or accepts a serialized proof in its place.
#[derive(Clone, Default)]
pub struct Authority(Rc<AuthorityState>);

type ShellKey = (Uuid, Option<Uuid>, Option<Vec<u8>>);

#[derive(Default)]
struct AuthorityState {
    shells: RefCell<BTreeMap<ShellKey, Weak<Shell>>>,
}

struct Shell {
    issuer: Weak<AuthorityState>,
    identity: ShellIdentity,
    live: Cell<bool>,
    closed: RefCell<Vec<Box<dyn Fn()>>>,
}

#[derive(Clone)]
pub struct HumanSession(Rc<Shell>);

impl Authority {
    pub fn new() -> Self {
        Self::default()
    }

    /// Native embedding entry, not an agent tool or public service method.
    pub fn authenticate_shell(&self, identity: ShellIdentity) -> HumanSession {
        // Independent canonical management identities can coexist in one application installation.
        // A fresh generation supersedes only that actual client/account, never another root's client.
        let key = (
            identity.installation,
            identity.account,
            identity.cloud.as_ref().map(|cloud| cloud.client.clone()),
        );
        let prior = self.0.shells.borrow().get(&key).and_then(Weak::upgrade);
        if let Some(prior) = prior {
            HumanSession(prior).close();
        }
        let shell = Rc::new(Shell {
            issuer: Rc::downgrade(&self.0),
            identity,
            live: Cell::new(true),
            closed: RefCell::new(vec![]),
        });
        self.0
            .shells
            .borrow_mut()
            .insert(key, Rc::downgrade(&shell));
        HumanSession(shell)
    }

    pub fn check(&self, session: &HumanSession) -> Result<()> {
        if !session.0.live.get()
            || !session
                .0
                .issuer
                .upgrade()
                .is_some_and(|s| Rc::ptr_eq(&s, &self.0))
        {
            return Err(Error::Revoked);
        }
        Ok(())
    }

    pub fn check_binding(&self, binding: &OwnerBinding) -> Result<()> {
        if !binding.is_live()
            || !binding
                .0
                .issuer
                .upgrade()
                .is_some_and(|a| Rc::ptr_eq(&a, &self.0))
        {
            return Err(Error::Revoked);
        }
        Ok(())
    }

    /// End every owner retained by this native application, including independent archives and
    /// vault-independent detector owners. Callbacks run after releasing the issuer map borrow.
    pub fn end_all(&self) {
        let owners = self
            .0
            .shells
            .borrow()
            .values()
            .filter_map(Weak::upgrade)
            .collect::<Vec<_>>();
        self.0.shells.borrow_mut().clear();
        for owner in owners {
            HumanSession(owner).close();
        }
    }

    /// Native authority-reducing action after an authenticated owner/session denial.
    pub fn end_owner(&self, owner: &OwnerBinding) -> Result<()> {
        self.check_binding(owner)?;
        HumanSession(owner.0.clone()).close();
        Ok(())
    }

    /// Trusted private UI owner records a real human action for an existing native owner.
    /// A read-only binding cannot mint a decision without this separately retained issuer.
    pub fn decide_for(
        &self,
        owner: &OwnerBinding,
        intent: &impl SerializeValue,
    ) -> Result<HumanDecision> {
        self.check_binding(owner)?;
        HumanSession(owner.0.clone()).decide(intent)
    }

    pub fn execution(
        &self,
        shell: &HumanSession,
        identity: ExecutionIdentity,
    ) -> Result<ExecutionBinding> {
        self.check(shell)?;
        if identity.account != shell.0.identity.account {
            return Err(Error::Forbidden);
        }
        Ok(ExecutionBinding {
            shell: shell.binding(),
            identity,
            lifetime: Rc::new(ExecutionLifetime {
                live: Cell::new(true),
                parent: None,
            }),
        })
    }
}

/// Read-only owner proof available to execution providers. It cannot mint a HumanDecision.
///
/// ```compile_fail
/// fn cannot_approve(binding: &swarmie_permissions::ExecutionBinding) {
///     binding.shell().decide(&"agent-authored approval");
/// }
/// ```
#[derive(Clone)]
pub struct OwnerBinding(Rc<Shell>);

impl OwnerBinding {
    pub fn identity(&self) -> &ShellIdentity {
        &self.0.identity
    }
    pub fn is_live(&self) -> bool {
        self.0.live.get() && self.0.issuer.strong_count() > 0
    }
    pub fn same_owner(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl HumanSession {
    pub fn binding(&self) -> OwnerBinding {
        OwnerBinding(self.0.clone())
    }

    pub fn same_owner(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    pub fn identity(&self) -> &ShellIdentity {
        &self.0.identity
    }

    pub fn close(&self) {
        if self.0.live.replace(false) {
            let callbacks = std::mem::take(&mut *self.0.closed.borrow_mut());
            for f in callbacks {
                f();
            }
        }
    }

    pub fn on_close(&self, closed: Box<dyn Fn()>) {
        if self.is_live() {
            self.0.closed.borrow_mut().push(closed);
        } else {
            closed();
        }
    }

    pub fn is_live(&self) -> bool {
        self.0.live.get() && self.0.issuer.strong_count() > 0
    }

    /// Must be called only by the qualified shell's actual user-action handler.
    pub fn decide(&self, intent: &impl Serialize) -> Result<HumanDecision> {
        if !self.is_live() {
            return Err(Error::Revoked);
        }
        Ok(HumanDecision {
            shell: self.clone(),
            seal: digest(&canonical(intent)?),
            consumed: Cell::new(false),
        })
    }
}

/// One-use authority is private native data, not a decision ID in serialized State.
pub struct HumanDecision {
    shell: HumanSession,
    seal: Vec<u8>,
    consumed: Cell<bool>,
}

impl HumanDecision {
    pub fn consume(&self, shell: &HumanSession, intent: &impl Serialize) -> Result<()> {
        self.consume_owner(&shell.binding(), intent)
    }

    pub fn consume_owner(&self, shell: &OwnerBinding, intent: &impl Serialize) -> Result<()> {
        if !shell.is_live()
            || !Rc::ptr_eq(&self.shell.0, &shell.0)
            || self.consumed.get()
            || self.seal != digest(&canonical(intent)?)
        {
            return Err(Error::Forbidden);
        }
        self.consumed.set(true);
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionIdentity {
    pub account: Option<Uuid>,
    pub execution: Uuid,
    pub generation: Uuid,
    pub project: Option<Uuid>,
    /// Stable configured definition. Ad-hoc children without it cannot match a restricted scope.
    pub agent: Option<Uuid>,
    pub conversation: Option<Uuid>,
    pub main_agent: bool,
}

#[derive(Clone)]
pub struct ExecutionBinding {
    shell: OwnerBinding,
    identity: ExecutionIdentity,
    lifetime: Rc<ExecutionLifetime>,
}

struct ExecutionLifetime {
    live: Cell<bool>,
    parent: Option<Rc<ExecutionLifetime>>,
}

impl ExecutionLifetime {
    fn is_live(&self) -> bool {
        self.live.get() && self.parent.as_ref().is_none_or(|parent| parent.is_live())
    }
}

impl ExecutionBinding {
    /// Authority-reducing physical scope. Closing it cannot end its logical parent.
    pub fn scoped(&self) -> Self {
        Self {
            shell: self.shell.clone(),
            identity: self.identity.clone(),
            lifetime: Rc::new(ExecutionLifetime {
                live: Cell::new(true),
                parent: Some(self.lifetime.clone()),
            }),
        }
    }

    pub fn is_live(&self) -> bool {
        self.shell.is_live() && self.lifetime.is_live()
    }

    /// Fence future dispatch without dropping a source that already owns its restricted sink.
    pub fn close(&self) {
        self.lifetime.live.set(false);
    }

    pub fn identity(&self) -> &ExecutionIdentity {
        &self.identity
    }

    pub fn shell(&self) -> &OwnerBinding {
        &self.shell
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Discover,
    RequestUse,
    RequestCapture,
    ReceiveForExecution,
    AcceptAndStore,
    Manage,
    InspectModelContent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CorePlugin {
    Vault,
    ModelProtection,
    ModelCredentials,
}

#[derive(Clone)]
pub struct PluginGrant(Rc<Grant>);

struct Grant {
    provider: String,
    generation: Uuid,
    permissions: BTreeSet<Permission>,
    live: Cell<bool>,
    core: bool,
}

impl PluginGrant {
    /// The distribution selects this compiled enum; a manifest string cannot assert core status.
    pub fn bundled(plugin: CorePlugin, provider: String) -> Self {
        let permissions = match plugin {
            CorePlugin::Vault => BTreeSet::from([
                Permission::Discover,
                Permission::RequestUse,
                Permission::RequestCapture,
                Permission::ReceiveForExecution,
                Permission::AcceptAndStore,
                Permission::Manage,
            ]),
            CorePlugin::ModelCredentials => {
                BTreeSet::from([Permission::RequestUse, Permission::ReceiveForExecution])
            }
            CorePlugin::ModelProtection => BTreeSet::from([Permission::InspectModelContent]),
        };
        Self(Rc::new(Grant {
            provider,
            generation: Uuid::new_v4(),
            permissions,
            live: Cell::new(true),
            core: true,
        }))
    }

    pub fn accept(
        shell: &HumanSession,
        decision: HumanDecision,
        provider: String,
        permissions: BTreeSet<Permission>,
    ) -> Result<Self> {
        decision.consume(shell, &("plugin-permissions/v1", &provider, &permissions))?;
        Ok(Self(Rc::new(Grant {
            provider,
            permissions,
            generation: Uuid::new_v4(),
            live: Cell::new(true),
            core: false,
        })))
    }

    pub fn check(&self, provider: &str, permission: Permission) -> Result<()> {
        if !self.0.live.get()
            || self.0.provider != provider
            || !self.0.permissions.contains(&permission)
        {
            return Err(Error::Forbidden);
        }
        Ok(())
    }

    pub fn revoke(&self) {
        self.0.live.set(false);
    }

    pub fn provider(&self) -> &str {
        &self.0.provider
    }

    pub fn generation(&self) -> Uuid {
        self.0.generation
    }

    pub fn permissions(&self) -> &BTreeSet<Permission> {
        &self.0.permissions
    }

    pub fn is_core(&self) -> bool {
        self.0.core
    }
}

#[cfg(feature = "tembra")]
mod context;

#[cfg(feature = "tembra")]
pub use context::*;

#[cfg(feature = "tls")]
pub mod transport;
