use std::{future::Future, pin::Pin, sync::Arc};

use crate::catalog::{ModelIdentity, Snapshot};
use zeroize::Zeroizing;

use crate::{AuthProfile, Error, Failure, Model, Protocol, Timeouts};

/// A credential captured for one exact route. It has no logging or serialization interface.
/// OAuth refresh tokens and account selection remain with the source's host owner.
pub struct Credentials {
    pub(crate) identity: ModelIdentity,
    pub(crate) profile: AuthProfile,
    pub(crate) secret: Option<Zeroizing<String>>,
    pub(crate) expires_at_ms: Option<u64>,
}

impl Credentials {
    /// Captures explicit authentication material; this does not qualify the auth profile.
    pub fn new(
        identity: ModelIdentity,
        profile: AuthProfile,
        secret: Option<Zeroizing<String>>,
        expires_at_ms: Option<u64>,
    ) -> Result<Self, Error> {
        identity
            .validate()
            .map_err(|_| Error::before(Failure::Configuration))?;
        match (profile, &secret) {
            (AuthProfile::Anonymous, None) if expires_at_ms.is_none() => {}
            (AuthProfile::Anonymous, _) | (_, None) => {
                return Err(Error::before(Failure::Configuration));
            }
            (_, Some(secret))
                if secret.is_empty()
                    || secret.chars().any(|c| c.is_control() || c.is_whitespace()) =>
            {
                return Err(Error::before(Failure::Configuration));
            }
            _ => {}
        }
        if matches!(profile, AuthProfile::ChatGpt | AuthProfile::LegacyCodex)
            && expires_at_ms.is_none()
        {
            return Err(Error::before(Failure::Configuration));
        }
        Ok(Self {
            identity,
            profile,
            secret,
            expires_at_ms,
        })
    }
}

/// Resolution may run on a local account actor; the future need not be Send.
pub type CredentialFuture<'a> = Pin<Box<dyn Future<Output = Result<Credentials, Error>> + 'a>>;

/// Resolves credentials for the host's already selected account and exact route.
/// Implementations coordinate renewal/publication with the account owner before returning.
/// Failure must not select a different account, consult ambient credentials or retry exchanges.
pub trait CredentialSource {
    fn resolve<'a>(&'a self, identity: &'a ModelIdentity) -> CredentialFuture<'a>;
}

/// Explicit static authentication, useful for API keys and anonymous local services.
/// This source never reads environment variables or changes the requested identity.
pub struct StaticCredentials(Credentials);

impl StaticCredentials {
    pub fn new(credentials: Credentials) -> Self {
        Self(credentials)
    }
}

impl CredentialSource for StaticCredentials {
    fn resolve<'a>(&'a self, identity: &'a ModelIdentity) -> CredentialFuture<'a> {
        Box::pin(async move {
            if identity != &self.0.identity {
                return Err(Error::before(Failure::Credential));
            }
            Ok(Credentials {
                identity: self.0.identity.clone(),
                profile: self.0.profile,
                secret: self.0.secret.clone(),
                expires_at_ms: self.0.expires_at_ms,
            })
        })
    }
}

impl Model {
    /// Validates metadata before resolving authentication. Resolution completes before
    /// preparation freezes headers and bytes; dispatch never renews a credential.
    /// Unsupported profiles remain unavailable even if the source returns a valid token.
    pub async fn from_source(
        snapshot: Arc<Snapshot>,
        protocol: Protocol,
        source: &dyn CredentialSource,
        timeouts: Timeouts,
    ) -> Result<Self, Error> {
        super::preparation::validate_route(&snapshot, protocol, timeouts)?;
        let credentials = source.resolve(snapshot.identity()).await?;
        if credentials.identity != *snapshot.identity() {
            return Err(Error::before(Failure::Credential));
        }
        Self::new(
            snapshot,
            protocol,
            credentials.profile,
            credentials.secret,
            credentials.expires_at_ms,
            timeouts,
        )
    }
}
