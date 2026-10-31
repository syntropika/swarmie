//! Protocol sessions stay with Tembra. Durable account workflow and exact
//! credential escrow belong to Accounts; a client session key is not an operation.

use super::recovery::{
    CredentialRead, CredentialSlot, ProviderState, RecoveryAccount, RecoveryKind, RecoveryPhase,
    RecoveryRecord, RecoveryReferences,
};
use super::{Accounts, AuthorizationFlow, failure, uncertain};
use crate::settings::Account;
use futures::{FutureExt, future::LocalBoxFuture};
use serde::{Deserialize, Serialize};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use tembra::{Context, Orb, Service, State};
use uuid::Uuid;
use zeroize::Zeroizing;

pub const SERVICE: &str = "swarmie.providers.authorization";

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AuthorizationState {
    Waiting {
        #[serde(rename = "authorizationUrl")]
        authorization_url: String,
        #[serde(rename = "userCode")]
        user_code: Option<String>,
        #[serde(rename = "callbackUri")]
        callback_uri: Option<String>,
    },
    Connected {
        account: Account,
    },
    Failed {
        error: String,
        retryable: bool,
    },
}

struct Authorization {
    flow: Box<dyn AuthorizationFlow>,
    source: Rc<Accounts>,
    busy: Cell<bool>,
    cancelled: Cell<bool>,
    reconciliation_required: Cell<bool>,
    pending: RefCell<Option<Zeroizing<String>>>,
    publication: Rc<super::commits::Publication>,
    operation: RefCell<Option<Context>>,
}

struct Busy<'a>(&'a Cell<bool>);

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.set(false);
    }
}

struct StartedAuthorization {
    flow: Box<dyn AuthorizationFlow>,
    publication: Rc<super::commits::Publication>,
}

impl Accounts {
    fn begin_authorization<'a>(
        &'a self,
        principal: &'a str,
        provider: &'a str,
        returning: Option<&'a str>,
    ) -> LocalBoxFuture<'a, tembra::Result<StartedAuthorization>> {
        Box::pin(async move {
            self.authorize(principal)?;
            let _mutation = self.mutation.lock().await;
            let state = self.ready_state().await?;
            let returning = returning
                .map(|id| {
                    state
                        .accounts()
                        .iter()
                        .find(|record| {
                            record.account.id == id && record.account.provider_id == provider
                        })
                        .cloned()
                        .ok_or_else(|| failure("the returning account is not configured"))
                })
                .transpose()?;
            if let Some(account) = &returning
                && self.driver.association_key(account)?.is_none()
            {
                return Err(failure("the returning account is not associated"));
            }
            let previous = returning
                .as_ref()
                .and_then(|account| state.credentials().get(&account.account.id))
                .cloned();
            let credential = if let Some(reference) = &previous {
                self.credentials.check_reference(reference)?;
                match self.credentials.read(reference.clone()).await? {
                    CredentialRead::Complete(value) => {
                        self.driver.validate_credential(
                            returning.as_ref().ok_or_else(uncertain)?,
                            &value,
                        )?;
                        Some(value)
                    }
                    CredentialRead::Missing => None,
                    CredentialRead::Partial | CredentialRead::Invalid => return Err(uncertain()),
                }
            } else {
                None
            };
            let operation = Uuid::new_v4();
            let issued = self.reference(CredentialSlot::Operation { operation })?;
            let record = RecoveryRecord::new(
                operation,
                state.revision(),
                RecoveryKind::Authorize,
                Some(self.driver.protocol_binding()?),
                RecoveryAccount::Unassociated {
                    provider: provider.into(),
                    returning,
                },
                RecoveryReferences::new(previous, Some(issued), None),
            )?;
            let state = self.checkpoint(&state, record.clone()).await?;
            self.checkpoint(
                &state,
                record.with_phase(RecoveryPhase::ExchangeMayStart, None)?,
            )
            .await?;
            // BrowserLogin may exchange before poll. The checkpoint precedes begin.
            let flow = self
                .driver
                .begin_authorization(provider, credential.as_ref())
                .await?;
            let publication = super::commits::Publication::new(operation);
            *self.publication.borrow_mut() = Some(Rc::downgrade(&publication));
            Ok(StartedAuthorization { flow, publication })
        })
    }

    async fn accept_authorization(
        &self,
        context: &Context,
        operation: Uuid,
        credential: &Zeroizing<String>,
    ) -> tembra::Result<Account> {
        context.check()?;
        self.authorize(&context.principal()?)?;
        let _mutation = self.mutation.lock().await;
        let state = self.state().await?;
        let record = self.active(&state, operation)?.clone();
        if record.kind() != RecoveryKind::Authorize
            || record.phase() != RecoveryPhase::ExchangeMayStart
        {
            return Err(uncertain());
        }
        let reference = record.references().issued().ok_or_else(uncertain)?;
        self.seal(reference, credential).await?;
        let state = self
            .checkpoint(&state, record.with_phase(RecoveryPhase::IssuedReady, None)?)
            .await?;
        self.discover_authorization(state).await
    }

    pub(super) async fn discover_authorization(
        &self,
        state: ProviderState,
    ) -> tembra::Result<Account> {
        let record = state.workflow().ok_or_else(uncertain)?;
        self.active(&state, record.operation())?;
        if record.kind() != RecoveryKind::Authorize || record.phase() != RecoveryPhase::IssuedReady
        {
            return Err(uncertain());
        }
        let RecoveryAccount::Unassociated { provider, .. } = record.account() else {
            return Err(uncertain());
        };
        let credential = self
            .read_complete(record.references().issued().ok_or_else(uncertain)?)
            .await?;
        let candidate = self
            .driver
            .accept_authorization(provider, &credential)
            .await?;
        if candidate.credential.as_deref().map(String::as_str) != Some(credential.as_str()) {
            return Err(failure(
                "discovery did not retain the exact issued credential",
            ));
        }
        self.candidate(state, candidate).await
    }
}

impl Authorization {
    fn waiting(&self) -> AuthorizationState {
        let instructions = self.flow.instructions();
        AuthorizationState::Waiting {
            authorization_url: instructions.authorization_url,
            user_code: instructions.user_code,
            callback_uri: instructions.callback_uri,
        }
    }

    async fn advance(
        &self,
        context: &Context,
        state: &State<Option<AuthorizationState>>,
        callback: Option<String>,
        _retry: bool,
    ) -> tembra::Result<()> {
        context.check()?;
        self.source.authorize(&context.principal()?)?;
        if self.cancelled.get() {
            return Ok(());
        }
        if let Some(account) = self.publication.account() {
            state.update(context, |_| {
                Ok(Some(AuthorizationState::Connected { account }))
            })?;
            self.pending.take();
            return Ok(());
        }
        // A retry cannot resubmit an exchange or silently resume an unknown stage.
        // Private reconciliation reads the durable operation and updates its receipt.
        if self.reconciliation_required.get() || self.busy.replace(true) {
            return Ok(());
        }
        let _busy = Busy(&self.busy);
        *self.operation.borrow_mut() = Some(context.clone());
        let result = match callback {
            Some(callback) => self.flow.submit_redirect(&callback).await.map(Some),
            None => self.flow.poll().await,
        };
        let next = match result {
            Ok(Some(credential)) => {
                self.pending.replace(Some(credential.clone()));
                match self
                    .source
                    .accept_authorization(context, self.publication.operation, &credential)
                    .await
                {
                    Ok(account) => {
                        self.pending.take();
                        AuthorizationState::Connected {
                            account: super::settings::first_chunk(account),
                        }
                    }
                    Err(error) => {
                        self.reconciliation_required.set(true);
                        AuthorizationState::Failed {
                            error: error.message,
                            retryable: false,
                        }
                    }
                }
            }
            Ok(None) => self.waiting(),
            Err(error) => {
                self.reconciliation_required.set(true);
                AuthorizationState::Failed {
                    error: error.message,
                    retryable: false,
                }
            }
        };
        self.operation.take();
        if !self.cancelled.get() {
            state.update(context, |_| Ok(Some(next)))?;
        }
        Ok(())
    }
}

/// Expose the existing authorization methods and state through the provider
/// domain. The host supplies only the protocol and platform adapters.
pub fn orb(source: Rc<Accounts>) -> Orb {
    Orb::new(SERVICE, 1).service(
        Service::new(SERVICE, move |declaration| {
            let address = declaration.address()?;
            source.authorize(&address.principal)?;
            let (provider, id, returning) = match address.key.as_slice() {
                [provider, id] => (provider.clone(), id, None),
                [provider, id, account] => (provider.clone(), id, Some(account.clone())),
                _ => return Err(failure("invalid authorization session key")),
            };
            if uuid::Uuid::parse_str(id).is_err() {
                return Err(failure("invalid authorization session key"));
            }
            let principal = address.principal;
            let source = source.clone();
            let state = declaration.state(None::<AuthorizationState>, 1, |state| {
                if serde_json::to_vec(state)
                    .map_err(|_| failure("authorization state could not be encoded"))?
                    .len()
                    > 24 * 1024
                {
                    return Err(failure(
                        "authorization state exceeds its presentation budget",
                    ));
                }
                Ok(())
            })?;
            let resource = declaration.resource_async(
                "provider.authorization",
                1,
                &[],
                move |_| {
                    let source = source.clone();
                    let principal = principal.clone();
                    let provider = provider.clone();
                    let returning = returning.clone();
                    async move {
                        let StartedAuthorization { flow, publication } = source
                            .begin_authorization(&principal, &provider, returning.as_deref())
                            .await?;
                        Ok(Authorization {
                            flow,
                            source,
                            busy: Cell::new(false),
                            cancelled: Cell::new(false),
                            reconciliation_required: Cell::new(false),
                            pending: RefCell::new(None),
                            publication,
                            operation: RefCell::new(None),
                        })
                    }
                    .boxed_local()
                },
                |authorization, _| {
                    async move {
                        authorization.flow.close().await;
                        Ok(())
                    }
                    .boxed_local()
                },
            )?;
            Ok((resource, state))
        })
        .lifetime(tembra::core::Lifetime::Referenced, 0)
        .method("poll", |env, context, (): ()| async move {
            env.0.value()?.advance(&context, &env.1, None, false).await
        })
        .method("retry", |env, context, (): ()| async move {
            env.0.value()?.advance(&context, &env.1, None, true).await
        })
        .method(
            "complete",
            |env, context, (callback,): (String,)| async move {
                env.0
                    .value()?
                    .advance(&context, &env.1, Some(callback), false)
                    .await
            },
        )
        .method("cancel", |env, context, (): ()| async move {
            let authorization = env.0.value()?;
            if env
                .1
                .value()?
                .as_ref()
                .as_ref()
                .is_some_and(|state| matches!(state, AuthorizationState::Connected { .. }))
            {
                return Ok(());
            }
            authorization.cancelled.set(true);
            authorization.flow.cancel();
            if let Some(operation) = authorization.operation.take() {
                let _ = operation.cancel();
            }
            env.1.update(&context, |_| {
                Ok(Some(AuthorizationState::Failed {
                    error: "Provider authorization was cancelled.".into(),
                    retryable: false,
                }))
            })?;
            Ok(())
        }),
    )
}
