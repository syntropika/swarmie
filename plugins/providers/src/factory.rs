//! Native model construction from one captured account and configuration.
//! This module has no concrete provider, credential store or execution engine.

use crate::{Model, invalid, settings::Account};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{rc::Rc, sync::Arc};
use swarmie_ai::catalog::{Preparation, Snapshot, SourceRecord};
use zeroize::Zeroizing;

/// Native account persistence, with the existing flat private configuration.
/// The adapter must exclude secrets. Do not publish this record through RPC or
/// State; only `account` is a public account projection. No Debug is provided.
#[derive(Clone, Serialize, Deserialize)]
pub struct AccountRecord {
    pub account: Account,
    #[serde(flatten)]
    private: Map<String, Value>,
}

impl AccountRecord {
    pub fn new(account: Account, private: Map<String, Value>) -> tembra::Result<Self> {
        if private.contains_key("account") {
            return Err(invalid(
                "private account configuration contains a public account key",
            ));
        }
        Ok(Self { account, private })
    }

    /// Opaque configuration and discovery facts. The concrete adapter validates
    /// their meaning; the common account workflow preserves unrecognized fields.
    pub fn private_configuration(&self) -> &Map<String, Value> {
        &self.private
    }

    pub fn into_parts(self) -> (Account, Map<String, Value>) {
        (self.account, self.private)
    }
}

/// Resolved selection. The account owner resolves defaults and auxiliary models
/// before capture. The adapter validates canonical reasoning and configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelSelection {
    pub account_id: String,
    pub model_id: String,
    pub reasoning: Option<String>,
}

/// Exact provider key in the existing embedded catalog. This does not alter the
/// operational identity or authorize inference from a model name or endpoint.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmbeddedCatalogBinding {
    pub provider: String,
}

/// Metadata inputs for one captured configuration, consumed by the existing
/// Catalog resolver. No source loading or credential I/O occurs during capture.
pub struct ModelMetadataInput {
    pub preparation: Preparation,
    pub embedded: Option<EmbeddedCatalogBinding>,
    pub records: Vec<SourceRecord>,
}

/// Account always means the captured, authorized account ID. It is not a
/// factory-selected credential key. None requires an explicitly credential-free
/// route and cannot be a fallback after a missing or invalid account credential.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CredentialRequirement {
    None,
    Account,
}

/// The native adapter can bind this factory to an existing execution scope.
/// Capture grants no model invocation or credential access authority.
pub trait ModelFactory {
    fn capture(
        &self,
        account: &AccountRecord,
        selection: &ModelSelection,
    ) -> tembra::Result<CapturedModel>;
}

/// Owns the exact configuration and canonical reasoning used during capture.
/// Implementations must revalidate the Snapshot against that configuration and
/// retain the existing physical admission and credential checks in the Model.
pub trait ModelConstruction {
    fn construct(
        self: Box<Self>,
        snapshot: Arc<Snapshot>,
        credential: Option<Zeroizing<String>>,
    ) -> tembra::Result<Rc<dyn Model>>;
}

/// Single-use operational model capture. This is separate from final payload
/// preparation. It has no Clone, Debug, serialization or private-state projection.
pub struct CapturedModel {
    metadata: ModelMetadataInput,
    credential: CredentialRequirement,
    construction: Box<dyn ModelConstruction>,
}

impl CapturedModel {
    pub fn new(
        metadata: ModelMetadataInput,
        credential: CredentialRequirement,
        construction: Box<dyn ModelConstruction>,
    ) -> tembra::Result<Self> {
        metadata
            .preparation
            .identity
            .validate()
            .map_err(|_| invalid("invalid captured model identity"))?;
        let revision = &metadata.preparation.configuration_revision;
        if revision.trim().is_empty()
            || revision.len() > 4096
            || revision.chars().any(char::is_control)
            || metadata
                .records
                .iter()
                .any(|record| record.identity != metadata.preparation.identity)
            || metadata.embedded.as_ref().is_some_and(|binding| {
                binding.provider.trim().is_empty()
                    || binding.provider.len() > 1024
                    || binding.provider.chars().any(char::is_control)
            })
        {
            return Err(invalid("invalid captured model metadata or configuration"));
        }
        Ok(Self {
            metadata,
            credential,
            construction,
        })
    }

    pub fn metadata(&self) -> &ModelMetadataInput {
        &self.metadata
    }

    pub fn credential_requirement(&self) -> CredentialRequirement {
        self.credential
    }

    /// Consume only after the account owner resolves metadata and settles any
    /// required credential persistence. Reject a changed selection or credential
    /// mode before invoking the concrete construction. This does not dispatch.
    pub fn construct(
        self,
        snapshot: Arc<Snapshot>,
        credential: Option<Zeroizing<String>>,
    ) -> tembra::Result<Rc<dyn Model>> {
        if snapshot.preparation() != &self.metadata.preparation {
            return Err(invalid(
                "model construction uses another captured preparation",
            ));
        }
        if credential.is_some() != (self.credential == CredentialRequirement::Account) {
            return Err(invalid(
                "model construction credential requirement does not match",
            ));
        }
        self.construction.construct(snapshot, credential)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ModelRequest, ModelResponse, OperationId};
    use futures::future::LocalBoxFuture;
    use std::cell::Cell;
    use swarmie_ai::catalog::{Capability, Catalog, Limits, ModelIdentity, Requirements};
    use tembra::Context;

    fn metadata(revision: &str) -> ModelMetadataInput {
        ModelMetadataInput {
            preparation: Preparation {
                identity: ModelIdentity {
                    provider: "fixture".into(),
                    adapter: "fixture-v1".into(),
                    endpoint: "https://provider.example/".into(),
                    deployment: Some("account".into()),
                    model: "selected".into(),
                    revision: None,
                },
                configuration_revision: revision.into(),
                requirements: Requirements {
                    context: false,
                    input: false,
                    output: false,
                    ..Requirements::default()
                },
                allow_stale_for_ms: 0,
            },
            embedded: None,
            records: vec![],
        }
    }

    struct Construction {
        calls: Rc<Cell<usize>>,
        expected: Arc<Snapshot>,
    }

    impl ModelConstruction for Construction {
        fn construct(
            self: Box<Self>,
            snapshot: Arc<Snapshot>,
            _credential: Option<Zeroizing<String>>,
        ) -> tembra::Result<Rc<dyn Model>> {
            assert!(Arc::ptr_eq(&snapshot, &self.expected));
            self.calls.set(self.calls.get() + 1);
            Ok(Rc::new(UnusedModel))
        }
    }

    struct UnusedModel;

    impl Model for UnusedModel {
        fn complete<'a>(
            &'a self,
            _context: &'a Context,
            _operation: OperationId,
            _request: &'a ModelRequest,
        ) -> LocalBoxFuture<'a, tembra::Result<ModelResponse>> {
            Box::pin(async { Err(invalid("the fixture does not dispatch")) })
        }
    }

    fn capture(
        input: ModelMetadataInput,
        mode: CredentialRequirement,
        expected: Arc<Snapshot>,
        calls: Rc<Cell<usize>>,
    ) -> tembra::Result<CapturedModel> {
        CapturedModel::new(input, mode, Box::new(Construction { calls, expected }))
    }

    #[test]
    fn account_round_trip_preserves_flat_private_fields_and_rejects_public_key_collision() {
        let raw = serde_json::json!({
            "account": {
                "id":"account", "providerId":"fixture", "providerLabel":"Fixture",
                "label":"Account", "models":[], "available":true,
                "defaults":{"model":null,"effort":null,"auxiliaryModel":null},
                "authorizationMethod":"local"
            },
            "provider":"fixture", "endpoint":"https://provider.example/",
            "oauth":false, "oauth_identity":null,
            "future_private_field":{"revision":7}
        });
        let record: AccountRecord = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(serde_json::to_value(&record).unwrap(), raw);
        let (account, private) = record.clone().into_parts();
        let restored = AccountRecord::new(account.clone(), private).unwrap();
        assert_eq!(
            restored.private_configuration(),
            record.private_configuration()
        );
        let mut collision = Map::new();
        collision.insert("account".into(), Value::Null);
        assert!(AccountRecord::new(account, collision).is_err());
    }

    #[test]
    fn capture_rejects_foreign_records_and_invalid_configuration_before_construction() {
        let mut catalog = Catalog::new(1);
        let snapshot = catalog.prepare(metadata("one").preparation, 42).unwrap();
        let calls = Rc::new(Cell::new(0));
        let mut input = metadata("one");
        let mut foreign = input.preparation.identity.clone();
        foreign.deployment = Some("another-account".into());
        input.records.push(SourceRecord {
            identity: foreign,
            limits: Limits::default(),
            tools: Capability::default(),
            streaming: Capability::default(),
            native_output_maximum: Capability::default(),
            native_compaction: Capability::default(),
        });
        assert!(
            capture(
                input,
                CredentialRequirement::None,
                snapshot.clone(),
                calls.clone()
            )
            .is_err()
        );
        assert!(
            capture(
                metadata("\n"),
                CredentialRequirement::None,
                snapshot,
                calls.clone()
            )
            .is_err()
        );
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn consuming_construction_requires_the_captured_preparation_and_credential_mode() {
        let mut catalog = Catalog::new(4);
        let snapshot = catalog.prepare(metadata("one").preparation, 42).unwrap();
        let other = catalog.prepare(metadata("two").preparation, 42).unwrap();
        let calls = Rc::new(Cell::new(0));
        for mode in [CredentialRequirement::None, CredentialRequirement::Account] {
            let make = || capture(metadata("one"), mode, snapshot.clone(), calls.clone()).unwrap();
            let credential = || {
                (mode == CredentialRequirement::Account).then(|| Zeroizing::new("fixture".into()))
            };
            let value = make();
            assert_eq!(value.credential_requirement(), mode);
            assert_eq!(value.metadata().preparation, snapshot.preparation().clone());
            let before = calls.get();
            assert!(value.construct(other.clone(), credential()).is_err());
            let wrong =
                (mode == CredentialRequirement::None).then(|| Zeroizing::new("fixture".into()));
            assert!(make().construct(snapshot.clone(), wrong).is_err());
            assert_eq!(calls.get(), before);
            let _model = make().construct(snapshot.clone(), credential()).unwrap();
            assert_eq!(calls.get(), before + 1);
        }
    }
}
