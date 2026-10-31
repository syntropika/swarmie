use super::*;
use crate::{AuthorityCheck, ClientAdmission, Role, digest};

impl VaultNode {
    /// Native account owner calls this on explicit node logout or authoritative session denial.
    /// Other node capabilities continue; an ended instance cannot accept a late unlock response.
    pub fn end_account(&self) {
        self.0.state.borrow_mut().ended = true;
        self.close_all();
    }
    pub(super) fn close_all(&self) {
        let sessions: Vec<_> = self
            .0
            .state
            .borrow()
            .sessions
            .values()
            .filter_map(Weak::upgrade)
            .collect();
        for session in sessions {
            session.close();
        }
    }
    /// Connected admission barrier. Transport loss retains the accepted last-observed lifetime;
    /// a malformed/rolled-back authority response requires reconciliation instead of a clean lease.
    pub async fn refresh_authority(&self, session: &VaultSession) -> Result<bool> {
        if self.0.state.borrow().ended || !session.0.live.get() || !session.owner().is_live() {
            return Err(Error::Revoked);
        }
        match self.reconcile(None).await {
            Ok(()) => {
                self.check_admission(session)?;
                Ok(true)
            }
            Err(Error::Offline | Error::SignInRequired) => {
                self.check_admission(session)?;
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }
    /// A connected use synchronizes confirmed records; only actual connection/auth-session loss
    /// permits the accepted warm offline path. Local storage failures never masquerade as offline.
    pub async fn refresh_current(&self, session: &VaultSession) -> Result<bool> {
        match self.synchronize(session).await {
            Ok(()) => Ok(true),
            Err(Error::Offline | Error::SignInRequired) => {
                self.check(session)?;
                Ok(false)
            }
            Err(error) => {
                self.0.state.borrow_mut().authority_failed = true;
                Err(error)
            }
        }
    }
    pub(super) async fn reconcile(&self, incoming: Option<&ClientAdmission>) -> Result<()> {
        let account_generation = self.0.state.borrow().account_generation;
        let response = self.cloud().current_authority(self.0.identity).await;
        if self.0.state.borrow().account_generation != account_generation {
            return Err(Error::Revoked);
        }
        let check = match response {
            Ok(check) => check,
            Err(Error::Revoked) => {
                self.end_account();
                return Err(Error::Revoked);
            }
            Err(error) => {
                if !matches!(error, Error::Offline | Error::SignInRequired) {
                    self.0.state.borrow_mut().authority_failed = true;
                }
                return Err(error);
            }
        };
        let result = self.apply_authority(check, incoming);
        if result.is_err() && !matches!(result, Err(Error::Revoked)) {
            self.0.state.borrow_mut().authority_failed = true;
        }
        result
    }
    fn apply_authority(
        &self,
        check: AuthorityCheck,
        incoming: Option<&ClientAdmission>,
    ) -> Result<()> {
        if check.identity != self.0.identity
            || check.sequence < self.0.state.borrow().snapshot.authority_cursor
            || self
                .0
                .state
                .borrow()
                .snapshot
                .authority
                .as_ref()
                .is_some_and(|old| old.exposed && !check.exposed)
            || check.devices.len() > 128
            || check.generations.len() > 16_384
        {
            return Err(Error::Integrity);
        }
        if let Some(previous) = self.0.state.borrow().snapshot.authority.as_ref() {
            for known in &previous.generations {
                if check
                    .generations
                    .iter()
                    .find(|current| current.client == known.client && current.node == known.node)
                    .is_none_or(|current| current.generation < known.generation)
                {
                    return Err(Error::Integrity);
                }
            }
        }
        let fingerprint = digest(&self.0.keys.signing_public());
        let node = check
            .devices
            .iter()
            .find(|d| d.device == self.0.device && d.fingerprint == fingerprint)
            .ok_or(Error::Integrity)?;
        if node.removed
            || !node.roles.contains(&Role::Decrypt)
            || node.family != Some(check.actor_family)
            || !check.active_families.contains(&check.actor_family)
        {
            self.end_account();
            return Err(Error::Revoked);
        }
        if incoming.is_some_and(|a| check.sequence < a.sequence) {
            return Err(Error::Integrity);
        }
        let sessions: Vec<_> = self
            .0
            .state
            .borrow()
            .sessions
            .values()
            .filter_map(Weak::upgrade)
            .collect();
        for session in sessions {
            if !client_permitted(&check, &session.admission) {
                let _ = self.0.authority.end_owner(&session.owner);
            }
            if !permits(&check, self.0.device, &session.admission) {
                session.close();
            }
        }
        let mut snapshot = self.0.state.borrow().snapshot.clone();
        if snapshot.authority.as_ref() != Some(&check) {
            snapshot.authority_cursor = check.sequence;
            snapshot.authority = Some(check);
            self.persist(snapshot)?;
        }
        self.0.state.borrow_mut().authority_failed = false;
        if incoming.is_some_and(|a| {
            !permits(
                self.0
                    .state
                    .borrow()
                    .snapshot
                    .authority
                    .as_ref()
                    .expect("just persisted authority"),
                self.0.device,
                a,
            )
        }) {
            return Err(Error::Revoked);
        }
        Ok(())
    }
}

fn client_permitted(check: &AuthorityCheck, admission: &ClientAdmission) -> bool {
    let Some(client) = check
        .devices
        .iter()
        .find(|d| d.fingerprint == admission.intent.client)
    else {
        return false;
    };
    !client.removed
        && client.roles.contains(&Role::Manage)
        && client.family == Some(admission.family)
        && check.active_families.contains(&admission.family)
}

fn permits(check: &AuthorityCheck, node: Uuid, admission: &ClientAdmission) -> bool {
    client_permitted(check, admission)
        && check.generations.iter().any(|g| {
            g.node == node
                && g.client == admission.intent.client
                && g.generation == admission.generation
        })
}

/// Native account callback tied to the generation it authenticated. Old account responses cannot
/// end a later login's authority, and this weak handle does not pin a vault data key in memory.
#[derive(Clone)]
pub struct AccountLifetime {
    state: Weak<RefCell<NodeState>>,
    generation: Uuid,
}
impl AccountLifetime {
    pub fn close(&self) {
        let Some(state) = self.state.upgrade() else {
            return;
        };
        let sessions = {
            let mut state = state.borrow_mut();
            if state.account_generation != self.generation {
                return;
            }
            state.ended = true;
            state
                .sessions
                .values()
                .filter_map(Weak::upgrade)
                .collect::<Vec<_>>()
        };
        for session in sessions {
            session.close();
        }
    }
}
impl VaultNode {
    pub fn account_lifetime(&self) -> AccountLifetime {
        AccountLifetime {
            state: Rc::downgrade(&self.0.state),
            generation: self.0.state.borrow().account_generation,
        }
    }
}
