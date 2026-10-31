//! Durable checkpoints. Storage values remain separate from backend authority.

use super::recovery::{
    CredentialRead, CredentialReference, CredentialSlot, ProviderState, RecoveryAccount,
    RecoveryKind, RecoveryPhase, RecoveryRecord, RecoveryReferences, StateLoad,
};
use super::{Accounts, StoreWriteError, failure, uncertain};
use crate::{AccountRecord, CredentialRequirement};
use std::collections::BTreeMap;
use uuid::Uuid;
use zeroize::Zeroizing;

impl Accounts {
    pub(super) async fn state(&self) -> tembra::Result<ProviderState> {
        let state = match self.store.load().await? {
            StateLoad::Initialized(state) => state,
            StateLoad::Provisioning(plan) => {
                if plan.owner() != &self.owner {
                    return Err(failure("provider provisioning belongs to another owner"));
                }
                let initial = ProviderState::new(
                    self.owner.clone(),
                    plan.initial_target(),
                    vec![],
                    BTreeMap::new(),
                    None,
                    self.limits,
                )?;
                self.store
                    .initialize(plan, initial.clone())
                    .await
                    .map_err(StoreWriteError::into_error)?;
                initial
            }
            StateLoad::Absent => {
                return Err(failure("provider initialization evidence is unavailable"));
            }
            StateLoad::Legacy(_) => {
                return Err(failure(
                    "provider legacy adoption requires private recovery",
                ));
            }
        };
        if state.owner() != &self.owner {
            return Err(failure("provider state belongs to another owner"));
        }
        state.encode(self.limits)?;
        Ok(state)
    }

    pub(super) async fn ready_state(&self) -> tembra::Result<ProviderState> {
        let state = self.state().await?;
        if state.workflow().is_some() {
            return Err(uncertain());
        }
        Ok(state)
    }

    pub(super) fn active<'a>(
        &self,
        state: &'a ProviderState,
        operation: Uuid,
    ) -> tembra::Result<&'a RecoveryRecord> {
        let record = state
            .workflow()
            .filter(|record| record.operation() == operation)
            .ok_or_else(uncertain)?;
        if let Some(binding) = record.protocol_binding()
            && binding != self.driver.protocol_binding()?
        {
            return Err(uncertain());
        }
        Ok(record)
    }

    pub(super) fn reference(&self, slot: CredentialSlot) -> tembra::Result<CredentialReference> {
        let reference = CredentialReference::new(self.owner.clone(), slot, Uuid::new_v4())?;
        reference.check_size(self.limits.reference_bytes())?;
        self.credentials.check_reference(&reference)?;
        Ok(reference)
    }

    pub(super) async fn read_complete(
        &self,
        reference: &CredentialReference,
    ) -> tembra::Result<Zeroizing<String>> {
        if reference.owner() != &self.owner {
            return Err(failure("foreign provider credential"));
        }
        self.credentials.check_reference(reference)?;
        match self.credentials.read(reference.clone()).await? {
            CredentialRead::Complete(value) => Ok(value),
            CredentialRead::Missing | CredentialRead::Partial | CredentialRead::Invalid => {
                Err(failure("the exact provider credential is not complete"))
            }
        }
    }

    pub(super) async fn seal(
        &self,
        reference: &CredentialReference,
        value: &Zeroizing<String>,
    ) -> tembra::Result<()> {
        self.credentials.check_reference(reference)?;
        self.credentials
            .stage(reference.clone(), value.clone())
            .await
            .map_err(StoreWriteError::into_error)?;
        if *self.read_complete(reference).await? != **value {
            return Err(uncertain());
        }
        Ok(())
    }

    pub(super) async fn checkpoint(
        &self,
        state: &ProviderState,
        record: RecoveryRecord,
    ) -> tembra::Result<ProviderState> {
        let next = ProviderState::new(
            self.owner.clone(),
            Uuid::new_v4(),
            state.accounts().to_vec(),
            state.credentials().clone(),
            Some(record),
            self.limits,
        )?;
        self.store
            .save(state.revision(), next.clone())
            .await
            .map_err(StoreWriteError::into_error)?;
        Ok(next)
    }

    pub(super) async fn renew_account(
        &self,
        context: &tembra::Context,
        state: ProviderState,
        account: AccountRecord,
        credential: Zeroizing<String>,
        now_ms: u64,
    ) -> tembra::Result<Zeroizing<String>> {
        let previous = state
            .credentials()
            .get(&account.account.id)
            .cloned()
            .ok_or_else(uncertain)?;
        self.credentials.check_reference(&previous)?;
        let candidate = self.reference(CredentialSlot::Account {
            account_id: account.account.id.clone(),
        })?;
        let record = RecoveryRecord::new(
            Uuid::new_v4(),
            state.revision(),
            RecoveryKind::Renew,
            Some(self.driver.protocol_binding()?),
            RecoveryAccount::Captured {
                before: Some(account.clone()),
                candidate: Box::new(account.clone()),
            },
            RecoveryReferences::new(Some(previous), None, Some(candidate.clone())),
        )?;
        let state = self.checkpoint(&state, record.clone()).await?;
        let state = self
            .checkpoint(
                &state,
                record.with_phase(RecoveryPhase::ExchangeMayStart, None)?,
            )
            .await?;
        // The pure decision and dispatch share this exact capture. Recovery never
        // substitutes a new time or repeats a possibly consumed refresh.
        context.check()?;
        let replacement = self
            .driver
            .renew(&account, &credential, now_ms)
            .await?
            .ok_or_else(uncertain)?;
        self.driver.validate_credential(&account, &replacement)?;
        self.seal(&candidate, &replacement).await?;
        let record = state
            .workflow()
            .ok_or_else(uncertain)?
            .clone()
            .with_phase(RecoveryPhase::CandidateReady, None)?;
        let state = self.checkpoint(&state, record).await?;
        self.publish(state).await?;
        Ok(replacement)
    }

    pub(super) async fn candidate(
        &self,
        state: ProviderState,
        mut candidate: super::AccountCandidate,
    ) -> tembra::Result<crate::settings::Account> {
        let old = state.workflow().ok_or_else(uncertain)?.clone();
        self.active(&state, old.operation())?;
        if self.driver.association_key(&candidate.record)? != candidate.association_key
            || candidate.record.account.models.is_empty()
        {
            return Err(failure(
                "the provider candidate has invalid association or models",
            ));
        }
        let mut matches = vec![];
        if let Some(key) = &candidate.association_key {
            for record in state.accounts() {
                if record.account.provider_id == candidate.record.account.provider_id
                    && self.driver.association_key(record)?.as_ref() == Some(key)
                {
                    matches.push(record.clone());
                }
            }
        }
        if matches.len() > 1 {
            return Err(failure("the provider association is ambiguous"));
        }
        let before = matches.pop();
        if let RecoveryAccount::Unassociated {
            provider,
            returning,
        } = old.account()
            && (provider != &candidate.record.account.provider_id
                || returning.as_ref().is_some_and(|original| {
                    before
                        .as_ref()
                        .is_none_or(|before| before.account.id != original.account.id)
                }))
        {
            return Err(failure(
                "the provider candidate does not match its original capture",
            ));
        }
        candidate.record.account.id = before.as_ref().map_or_else(
            || Uuid::new_v4().to_string(),
            |record| record.account.id.clone(),
        );
        candidate.record.account.defaults = before
            .as_ref()
            .map_or_else(crate::settings::ModelDefaults::default, |record| {
                record.account.defaults.clone()
            });
        if let Some(before) = &before {
            let (account, private) = candidate.record.into_parts();
            let mut merged = before.private_configuration().clone();
            merged.extend(private);
            candidate.record = AccountRecord::new(account, merged)?;
        }
        match (
            &candidate.credential,
            self.driver.credential_requirement(&candidate.record)?,
        ) {
            (Some(value), CredentialRequirement::Account) => {
                self.driver.validate_credential(&candidate.record, value)?
            }
            (None, CredentialRequirement::None) => {}
            _ => {
                return Err(failure(
                    "the candidate credential requirement is inconsistent",
                ));
            }
        }
        let previous = before
            .as_ref()
            .and_then(|before| state.credentials().get(&before.account.id))
            .cloned();
        let reference = candidate
            .credential
            .as_ref()
            .map(|_| {
                self.reference(CredentialSlot::Account {
                    account_id: candidate.record.account.id.clone(),
                })
            })
            .transpose()?;
        let record = RecoveryRecord::new(
            old.operation(),
            old.base_revision(),
            old.kind(),
            old.protocol_binding(),
            RecoveryAccount::Captured {
                before,
                candidate: Box::new(candidate.record),
            },
            RecoveryReferences::new(
                previous,
                old.references().issued().cloned(),
                reference.clone(),
            ),
        )?
        .with_phase(
            if reference.is_some() {
                RecoveryPhase::CandidateStaging
            } else {
                RecoveryPhase::CandidateReady
            },
            None,
        )?;
        let state = self.checkpoint(&state, record).await?;
        if let (Some(reference), Some(value)) = (reference, candidate.credential) {
            self.seal(&reference, &value).await?;
        }
        let record = state
            .workflow()
            .ok_or_else(uncertain)?
            .clone()
            .with_phase(RecoveryPhase::CandidateReady, None)?;
        let state = self.checkpoint(&state, record).await?;
        self.publish(state).await
    }
}
