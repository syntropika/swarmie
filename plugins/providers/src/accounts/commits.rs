//! Exact account/reference publication and bounded cleanup. There is no mutable
//! credential selector and no automatic replay of an uncertain publication.

use super::recovery::{
    CredentialRead, ProviderState, RecoveryAccount, RecoveryKind, RecoveryPhase, ReplacementKind,
    StateReplacement,
};
use super::{Accounts, failure, uncertain};
use crate::settings::Account;
use std::{cell::RefCell, rc::Rc};
use uuid::Uuid;

/// The original live authorization session's safe receipt. This contains no
/// secret and is not a workflow registry or authority to override storage.
pub(super) struct Publication {
    pub(super) operation: Uuid,
    account: RefCell<Option<Account>>,
    clearing: RefCell<Option<(uuid::Uuid, Vec<u8>, Account)>>,
}

impl Publication {
    pub(super) fn new(operation: Uuid) -> Rc<Self> {
        Rc::new(Self {
            operation,
            account: RefCell::new(None),
            clearing: RefCell::new(None),
        })
    }

    pub(super) fn account(&self) -> Option<Account> {
        self.account.borrow().clone()
    }
}

impl Accounts {
    pub(super) async fn publish(&self, state: ProviderState) -> tembra::Result<Account> {
        let record = state.workflow().ok_or_else(uncertain)?.clone();
        self.active(&state, record.operation())?;
        if record.phase() != RecoveryPhase::CandidateReady
            && !(record.kind() == RecoveryKind::Defaults && record.phase() == RecoveryPhase::Intent)
        {
            return Err(uncertain());
        }
        let RecoveryAccount::Captured { candidate, .. } = record.account() else {
            return Err(uncertain());
        };
        if let Some(reference) = record.references().candidate() {
            let value = self.read_complete(reference).await?;
            self.driver.validate_credential(candidate, &value)?;
        } else if record.kind() != RecoveryKind::Defaults
            && self.driver.credential_requirement(candidate)?
                == crate::CredentialRequirement::Account
        {
            return Err(uncertain());
        }
        let checkpoint_revision = Uuid::new_v4();
        let plan = StateReplacement::new(
            ReplacementKind::Publish,
            checkpoint_revision,
            Uuid::new_v4(),
        )?;
        let checkpoint = ProviderState::new(
            self.owner.clone(),
            checkpoint_revision,
            state.accounts().to_vec(),
            state.credentials().clone(),
            Some(
                record
                    .clone()
                    .with_phase(RecoveryPhase::AccountPublication, Some(plan))?,
            ),
            self.limits,
        )?;
        self.store
            .save(state.revision(), checkpoint)
            .await
            .map_err(super::StoreWriteError::into_error)?;
        let mut accounts = state.accounts().to_vec();
        accounts.retain(|record| record.account.id != candidate.account.id);
        accounts.push(candidate.as_ref().clone());
        let mut credentials = state.credentials().clone();
        if record.kind() != RecoveryKind::Defaults {
            credentials.remove(&candidate.account.id);
            if let Some(reference) = record.references().candidate() {
                credentials.insert(candidate.account.id.clone(), reference.clone());
            }
        }
        let target = ProviderState::new(
            self.owner.clone(),
            plan.target_revision(),
            accounts,
            credentials,
            Some(record.with_phase(RecoveryPhase::Committed, Some(plan))?),
            self.limits,
        )?;
        self.store
            .save(plan.expected_revision(), target.clone())
            .await
            .map_err(super::StoreWriteError::into_error)?;
        self.finish(target).await
    }

    pub(super) async fn finish(&self, state: ProviderState) -> tembra::Result<Account> {
        let record = state.workflow().ok_or_else(uncertain)?.clone();
        self.active(&state, record.operation())?;
        let RecoveryAccount::Captured { candidate, .. } = record.account() else {
            return Err(uncertain());
        };
        let account = candidate.account.clone();
        // A qualified exact state observation is required even after publication.
        // The checked Committed image fixes the account and immutable reference.
        let current = state
            .accounts()
            .iter()
            .find(|value| value.account.id == account.id)
            .ok_or_else(uncertain)?;
        if serde_json::to_vec(current).map_err(|_| uncertain())?
            != serde_json::to_vec(candidate.as_ref()).map_err(|_| uncertain())?
            || (record.kind() != RecoveryKind::Defaults
                && state.credentials().get(&account.id) != record.references().candidate())
        {
            return Err(uncertain());
        }
        if record.kind() != RecoveryKind::Defaults
            && let Some(reference) = record.references().candidate()
        {
            self.driver
                .validate_credential(candidate, &self.read_complete(reference).await?)?;
        }
        let state = if record.phase() == RecoveryPhase::Committed {
            if record
                .replacement()
                .is_none_or(|plan| plan.kind() != ReplacementKind::Publish)
            {
                return Err(uncertain());
            }
            self.checkpoint(
                &state,
                record.clone().with_phase(RecoveryPhase::Retirement, None)?,
            )
            .await?
        } else if record.phase() == RecoveryPhase::Retirement && record.replacement().is_none() {
            state
        } else {
            // A planned clear may already have been dispatched. Its predecessor
            // image cannot authorize replay, even when all cleanup is absent.
            return Err(uncertain());
        };
        let mut cleanup = vec![];
        for reference in [record.references().previous(), record.references().issued()]
            .into_iter()
            .flatten()
        {
            if !state.credentials().values().any(|bound| bound == reference)
                && !cleanup.contains(reference)
            {
                cleanup.push(reference.clone());
            }
        }
        for reference in cleanup {
            self.credentials.check_reference(&reference)?;
            if !matches!(
                self.credentials.read(reference.clone()).await?,
                CredentialRead::Missing
            ) {
                // The native adapter must independently establish ownership and
                // exclusion, including safe refusal of unowned orphan parts.
                self.credentials
                    .retire(reference.clone())
                    .await
                    .map_err(super::StoreWriteError::into_error)?;
                if !matches!(
                    self.credentials.read(reference).await?,
                    CredentialRead::Missing
                ) {
                    return Err(uncertain());
                }
            }
        }
        let revision = Uuid::new_v4();
        let plan = StateReplacement::new(ReplacementKind::Clear, revision, Uuid::new_v4())?;
        let checkpoint = ProviderState::new(
            self.owner.clone(),
            revision,
            state.accounts().to_vec(),
            state.credentials().clone(),
            Some(
                record
                    .clone()
                    .with_phase(RecoveryPhase::Retirement, Some(plan))?,
            ),
            self.limits,
        )?;
        self.store
            .save(state.revision(), checkpoint)
            .await
            .map_err(super::StoreWriteError::into_error)?;
        let clear = ProviderState::new(
            self.owner.clone(),
            plan.target_revision(),
            state.accounts().to_vec(),
            state.credentials().clone(),
            None,
            self.limits,
        )?;
        if let Some(receipt) = self
            .publication
            .borrow()
            .as_ref()
            .and_then(std::rc::Weak::upgrade)
            && receipt.operation == record.operation()
        {
            *receipt.clearing.borrow_mut() = Some((
                clear.revision(),
                clear.encode(self.limits)?,
                account.clone(),
            ));
        }
        self.store
            .save(plan.expected_revision(), clear)
            .await
            .map_err(super::StoreWriteError::into_error)?;
        if let Some(receipt) = self
            .publication
            .borrow()
            .as_ref()
            .and_then(std::rc::Weak::upgrade)
            && receipt.operation == record.operation()
        {
            *receipt.account.borrow_mut() = Some(super::settings::first_chunk(account.clone()));
        }
        Ok(account)
    }

    /// Inspect the durable workflow through qualified backend observations.
    /// Exchanges, unknown publications and pre-verified-stage collisions are
    /// never replayed. Missing material or native guarantees keep Active.
    pub async fn reconcile(&self, context: &tembra::Context) -> tembra::Result<Option<Account>> {
        context.check()?;
        self.authorize(&context.principal()?)?;
        let _mutation = self.mutation.lock().await;
        let state = self.state().await?;
        let Some(record) = state.workflow() else {
            if let Some(receipt) = self
                .publication
                .borrow()
                .as_ref()
                .and_then(std::rc::Weak::upgrade)
            {
                let planned = receipt.clearing.borrow().clone();
                if let Some((revision, bytes, account)) = planned
                    && state.revision() == revision
                    && state.encode(self.limits)? == bytes
                {
                    *receipt.account.borrow_mut() =
                        Some(super::settings::first_chunk(account.clone()));
                    receipt.clearing.take();
                    return Ok(Some(super::settings::first_chunk(account)));
                }
            }
            return Ok(None);
        };
        self.active(&state, record.operation())?;
        let result = match record.phase() {
            RecoveryPhase::IssuedReady => self.discover_authorization(state).await?,
            RecoveryPhase::CandidateReady => self.publish(state).await?,
            RecoveryPhase::Intent if record.kind() == RecoveryKind::Defaults => {
                self.publish(state).await?
            }
            RecoveryPhase::Committed | RecoveryPhase::Retirement => self.finish(state).await?,
            // A Complete credential below Ready is not provenance: a refused
            // unequal sealed collision followed by a crash can look identical.
            _ => return Err(uncertain()),
        };
        context.check()?;
        Ok(Some(super::settings::first_chunk(result)))
    }

    pub(super) async fn save_defaults(
        &self,
        principal: &str,
        input: crate::settings::SetModelDefaults,
    ) -> tembra::Result<crate::settings::ModelDefaults> {
        self.authorize(principal)?;
        let _mutation = self.mutation.lock().await;
        let state = self.ready_state().await?;
        let before = state
            .accounts()
            .iter()
            .find(|record| record.account.id == input.account_id)
            .cloned()
            .ok_or_else(|| failure("the provider account is not configured"))?;
        input.defaults.validate(&before.account.models)?;
        let mut candidate = before.clone();
        candidate.account.defaults = input.defaults.clone();
        let record = super::recovery::RecoveryRecord::new(
            Uuid::new_v4(),
            state.revision(),
            RecoveryKind::Defaults,
            None,
            RecoveryAccount::Captured {
                before: Some(before),
                candidate: Box::new(candidate),
            },
            super::recovery::RecoveryReferences::default(),
        )?;
        let state = self.checkpoint(&state, record).await?;
        self.publish(state).await?;
        Ok(input.defaults)
    }
}
