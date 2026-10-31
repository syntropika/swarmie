//! Account persistence, custody and protocol seams. These ports contain no
//! platform implementation. Their owners must retain started physical I/O until
//! completion; a dropped observer cannot establish that a write did not apply.

use crate::{AccountRecord, CredentialRequirement, settings::ConfigureProvider};
use futures::future::LocalBoxFuture;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

/// A failed mutation with an explicit application outcome. Unknown must retain
/// its pending candidate until recognition; it does not authorize a blind retry.
pub enum StoreWriteError {
    NotApplied(tembra::Error),
    Unknown(tembra::Error),
}

impl StoreWriteError {
    pub fn into_error(self) -> tembra::Error {
        match self {
            Self::NotApplied(error) => error,
            Self::Unknown(_) => super::uncertain(),
        }
    }
}

/// Recovery observations require the backend's qualified same-owner settlement,
/// visibility and durability barrier. A byte-only read cannot clear Unknown.
/// Unsupported guarantees remain errors. No value grants native authority.
pub trait AccountStore {
    fn load(&self) -> LocalBoxFuture<'_, tembra::Result<super::recovery::StateLoad>>;

    fn initialize(
        &self,
        plan: super::recovery::ProvisioningPlan,
        initial: super::recovery::ProviderState,
    ) -> LocalBoxFuture<'_, Result<(), StoreWriteError>>;

    fn adopt(
        &self,
        expected: super::recovery::LegacyPredecessor,
        initial: super::recovery::ProviderState,
    ) -> LocalBoxFuture<'_, Result<(), StoreWriteError>>;

    fn save(
        &self,
        expected_revision: uuid::Uuid,
        next: super::recovery::ProviderState,
    ) -> LocalBoxFuture<'_, Result<(), StoreWriteError>>;
}

/// Immutable exact-reference custody. Reads require qualified physical settlement
/// and visibility. Stage never overwrites a different sealed value or orphan.
/// Retirement requires native-established ownership, not a planned reference.
pub trait CredentialStore {
    fn check_reference(
        &self,
        reference: &super::recovery::CredentialReference,
    ) -> tembra::Result<()>;

    fn stage(
        &self,
        reference: super::recovery::CredentialReference,
        value: Zeroizing<String>,
    ) -> LocalBoxFuture<'_, Result<(), StoreWriteError>>;

    fn read(
        &self,
        reference: super::recovery::CredentialReference,
    ) -> LocalBoxFuture<'_, tembra::Result<super::recovery::CredentialRead>>;

    fn retire(
        &self,
        reference: super::recovery::CredentialReference,
    ) -> LocalBoxFuture<'_, Result<(), StoreWriteError>>;
}

/// A protocol result before account association or persistence. The concrete
/// driver validates registration and excludes secrets from private configuration.
/// Only its credential field can hold encoded secret material. No Debug or RPC
/// serialization is provided.
pub struct AccountCandidate {
    pub record: AccountRecord,
    pub credential: Option<Zeroizing<String>>,
    pub association_key: Option<String>,
}

/// Only public browser instructions enter the authorization projection.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthorizationInstructions {
    pub authorization_url: String,
    pub user_code: Option<String>,
    pub callback_uri: Option<String>,
}

/// Wrap the existing provider protocol handle. This is not another login engine.
/// The owning Tembra resource retains the handle and closes it before release.
pub trait AuthorizationFlow {
    fn instructions(&self) -> AuthorizationInstructions;

    fn poll(&self) -> LocalBoxFuture<'_, tembra::Result<Option<Zeroizing<String>>>>;

    fn submit_redirect<'a>(
        &'a self,
        callback: &'a str,
    ) -> LocalBoxFuture<'a, tembra::Result<Zeroizing<String>>>;

    fn cancel(&self);

    fn close(&self) -> LocalBoxFuture<'_, ()>;
}

/// Concrete provider facts and protocols. These methods perform no account or
/// custody writes. Account association, pending results, cancellation fences and
/// persistence ordering remain with the common Accounts owner.
pub trait AccountDriver {
    fn protocol_binding(&self) -> tembra::Result<super::recovery::ProtocolBinding>;

    fn needs_renewal(
        &self,
        account: &AccountRecord,
        credential: &Zeroizing<String>,
        now_ms: u64,
    ) -> tembra::Result<bool>;

    fn configure(
        &self,
        input: ConfigureProvider,
    ) -> LocalBoxFuture<'_, tembra::Result<AccountCandidate>>;

    fn begin_authorization<'a>(
        &'a self,
        provider: &'a str,
        returning: Option<&'a Zeroizing<String>>,
    ) -> LocalBoxFuture<'a, tembra::Result<Box<dyn AuthorizationFlow>>>;

    fn accept_authorization<'a>(
        &'a self,
        provider: &'a str,
        credential: &'a Zeroizing<String>,
    ) -> LocalBoxFuture<'a, tembra::Result<AccountCandidate>>;

    fn renew<'a>(
        &'a self,
        account: &'a AccountRecord,
        credential: &'a Zeroizing<String>,
        now_ms: u64,
    ) -> LocalBoxFuture<'a, tembra::Result<Option<Zeroizing<String>>>>;

    /// Decode and check the exact registration before a previous value can be
    /// used as a restore point. This must not refresh or perform remote I/O.
    fn validate_credential(&self, account: &AccountRecord, credential: &str) -> tembra::Result<()>;

    /// The key is scoped to a validated registration and issuer/subject, never
    /// an email or display label. This method performs no association or I/O.
    fn association_key(&self, account: &AccountRecord) -> tembra::Result<Option<String>>;

    /// Account availability describes custody presence, not model entitlement.
    /// Explicit credential-free accounts do not probe or fall back to custody.
    fn credential_requirement(
        &self,
        account: &AccountRecord,
    ) -> tembra::Result<CredentialRequirement>;
}
