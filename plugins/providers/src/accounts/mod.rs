//! Provider account and model selection workflow. Native hosts inject stores,
//! custody, protocol drivers and a clock; this module owns their ordering.
//! The independent Catalog remains the only metadata resolver and cache.

pub mod authorization;
mod commits;
mod ports;
pub mod recovery;
mod selection;
mod settings;
mod workflow;

pub use ports::{
    AccountCandidate, AccountDriver, AccountStore, AuthorizationFlow, AuthorizationInstructions,
    CredentialStore, StoreWriteError,
};
pub use selection::{ModelChoice, SelectModel};

use crate::{CapturedModel, CredentialRequirement, Model, ModelFactory};
use futures::lock::Mutex;
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
    sync::{Arc, OnceLock},
};
use swarmie_model_catalog::{Catalog, ResolutionError, Snapshot, sources::ModelsDev};

pub(super) fn failure(message: &str) -> tembra::Error {
    tembra::Error::new(tembra::Code::Unavailable, "providers.settings", message)
}

pub(super) fn uncertain() -> tembra::Error {
    tembra::Error::new(
        tembra::Code::Unavailable,
        "providers.write_unknown",
        "a provider store write has an unknown outcome; reconcile it before another mutation",
    )
}

/// One provider-domain owner for a principal's account data. Its native factory
/// is injected by trusted composition, not supplied through RPC. Captured models
/// and final prepared requests retain their existing execution permissions.
pub struct Accounts {
    owner: recovery::ProviderOwner,
    limits: recovery::RecoveryLimits,
    store: Rc<dyn AccountStore>,
    credentials: Rc<dyn CredentialStore>,
    driver: Rc<dyn AccountDriver>,
    now: Rc<dyn Fn() -> u64>,
    source: OnceLock<Result<ModelsDev, ResolutionError>>,
    catalog: RefCell<Catalog>,
    mutation: Mutex<()>,
    publication: RefCell<Option<Weak<commits::Publication>>>,
}

impl Accounts {
    pub fn new(
        owner: recovery::ProviderOwner,
        store: Rc<dyn AccountStore>,
        credentials: Rc<dyn CredentialStore>,
        driver: Rc<dyn AccountDriver>,
        now: Rc<dyn Fn() -> u64>,
        metadata_capacity: usize,
        recovery_limits: recovery::RecoveryLimits,
    ) -> tembra::Result<Self> {
        Ok(Self {
            owner,
            limits: recovery_limits,
            store,
            credentials,
            driver,
            now,
            source: OnceLock::new(),
            catalog: RefCell::new(Catalog::new(metadata_capacity)),
            mutation: Mutex::new(()),
            publication: RefCell::new(None),
        })
    }

    fn authorize(&self, principal: &str) -> tembra::Result<()> {
        if principal != self.owner.principal() {
            return Err(tembra::Error::new(
                tembra::Code::Forbidden,
                "providers.principal",
                "provider access refused",
            ));
        }
        Ok(())
    }

    fn metadata(&self, capture: &CapturedModel) -> tembra::Result<Arc<Snapshot>> {
        let input = capture.metadata();
        let identity = &input.preparation.identity;
        let mut records = input.records.clone();
        if let Some(binding) = &input.embedded {
            let source = self
                .source
                .get_or_init(ModelsDev::bundled)
                .as_ref()
                .map_err(|_| failure("the bundled model metadata is invalid"))?;
            if let Some(record) = source
                .bind_provider(identity.clone(), &binding.provider)
                .map_err(|_| failure("the model metadata binding is invalid"))?
            {
                records.push(record);
            }
        }
        let mut catalog = self.catalog.borrow_mut();
        catalog
            .replace_route(identity, records)
            .map_err(|_| failure("the model metadata sources are invalid"))?;
        catalog
            .prepare(input.preparation.clone(), (self.now)())
            .map_err(|_| {
                tembra::Error::new(
                    tembra::Code::Unavailable,
                    "providers.model_metadata",
                    "required model metadata is unavailable for this exact route",
                )
            })
    }

    /// Resolve account defaults and required metadata before inference custody.
    /// A concrete factory is bound to the actual execution scope by the adapter.
    /// No remote discovery or metadata refresh occurs on this selection path.
    pub async fn select(
        &self,
        context: &tembra::Context,
        request: SelectModel,
        factory: &dyn ModelFactory,
    ) -> tembra::Result<Rc<dyn Model>> {
        context.check()?;
        self.authorize(&context.principal()?)?;
        let _mutation = self.mutation.lock().await;
        let state = self.ready_state().await?;
        let account = state
            .accounts()
            .iter()
            .find(|record| record.account.id == request.account_id)
            .cloned()
            .ok_or_else(|| failure("the provider account is not configured"))?;
        context.check()?;
        let selection = selection::resolve(&account, &request)?;
        let capture = factory.capture(&account, &selection)?;
        if capture.metadata().preparation.identity.model != selection.model_id {
            return Err(crate::invalid(
                "the captured model does not match the selected model",
            ));
        }
        let snapshot = self.metadata(&capture)?;
        context.check()?;
        let credential = match capture.credential_requirement() {
            CredentialRequirement::None => None,
            CredentialRequirement::Account => {
                let reference = state
                    .credentials()
                    .get(&account.account.id)
                    .ok_or_else(|| failure("the account has no bound credential"))?;
                let current = self.read_complete(reference).await?;
                self.driver.validate_credential(&account, &current)?;
                context.check()?;
                let now = (self.now)();
                if self.driver.needs_renewal(&account, &current, now)? {
                    Some(
                        self.renew_account(context, state, account.clone(), current, now)
                            .await?,
                    )
                } else {
                    Some(current)
                }
            }
        };
        context.check()?;
        capture.construct(snapshot, credential)
    }
}
