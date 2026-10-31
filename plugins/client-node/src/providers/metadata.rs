//! Operational metadata for one captured account configuration. This module has
//! no credential access and performs no metadata network requests.

use super::{SavedAccount, catalog, failure, qualifications};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    sync::{Arc, OnceLock},
};
use swarmie_model_catalog::{
    Capability, Catalog, Evidence, Layer, Limits, Preparation, Requirements, ResolutionError,
    Snapshot, SourceRecord, sources::ModelsDev,
};
use swarmie_model_provider_ai::{Connection, ModelId};

/// Optional facts returned during account discovery. They stay private and do
/// not enlarge the account/UI projection. The endpoint fences later rebinding.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DiscoveredLimits {
    pub endpoint: String,
    pub models: BTreeMap<String, Limits>,
    #[serde(default)]
    pub capabilities: BTreeMap<String, qualifications::DeclaredCapabilities>,
}

pub(super) struct ModelMetadata {
    source: OnceLock<Result<ModelsDev, ResolutionError>>,
    cache: RefCell<Catalog>,
}

impl Default for ModelMetadata {
    fn default() -> Self {
        Self {
            source: OnceLock::new(),
            cache: RefCell::new(Catalog::new(64)),
        }
    }
}

impl ModelMetadata {
    pub fn prepare(
        &self,
        saved: &SavedAccount,
        connection: &Connection,
        model: &ModelId,
        effort: &str,
        now_ms: u64,
    ) -> tembra::Result<Arc<Snapshot>> {
        let identity = connection.model_identity(model)?;
        if connection
            .endpoint
            .as_deref()
            .map(|value| value.trim_end_matches('/'))
            != Some(saved.endpoint.trim_end_matches('/'))
        {
            return Err(failure(
                "the captured model route differs from its account configuration",
            ));
        }
        let normalized_effort = effort.to_ascii_lowercase();
        let mut records = Vec::new();
        let mut standard = false;
        let mut declared = None;
        // Product choices make the catalog binding explicit. A custom endpoint
        // or authorized account route never inherits another deployment's facts.
        if !saved.oauth
            && let Some(spec) = catalog::specifications().into_iter().find(|spec| {
                spec.id == saved.account.provider_id
                    && spec.provider == connection.provider
                    && spec.endpoint.trim_end_matches('/') == saved.endpoint.trim_end_matches('/')
            })
            && let Some(provider) = spec.catalog_provider
        {
            standard = true;
            let source = self
                .source
                .get_or_init(ModelsDev::bundled)
                .as_ref()
                .map_err(|_| failure("the bundled model metadata is invalid"))?;
            if let Some(record) = source
                .bind_provider(identity.clone(), provider)
                .map_err(|_| failure("the model metadata binding is invalid"))?
            {
                records.push(record);
            }
        }
        if let Some(discovered) = &saved.discovered_limits
            && discovered.endpoint.trim_end_matches('/') == saved.endpoint.trim_end_matches('/')
        {
            declared = discovered.capabilities.get(model.as_str());
            if let Some(limits) = discovered.models.get(model.as_str()) {
                records.push(SourceRecord {
                    identity: identity.clone(),
                    limits: limits.clone(),
                    tools: Capability::default(),
                    streaming: Capability::default(),
                    native_output_maximum: Capability::default(),
                    native_compaction: Capability::default(),
                });
            }
        }
        let (tools, streaming) = qualifications::qualify(
            connection.provider,
            &saved.account.provider_id,
            model.as_str(),
            &normalized_effort,
            standard,
            declared,
        )
        .map_err(|reason| {
            tembra::Error::new(tembra::Code::Unavailable, "providers.model_profile", reason)
        })?;
        // The current message conversion has no compact operation and cannot
        // preserve a provider's opaque continuation item. This is an adapter
        // restriction, independent of a public model/provider capability flag.
        records.push(SourceRecord {
            identity: identity.clone(),
            limits: Limits::default(),
            tools,
            streaming,
            native_output_maximum: Capability::default(),
            native_compaction: Capability::Unsupported {
                evidence: Evidence {
                    source: "swarmie-model-provider-ai".into(),
                    revision: "swarmie-ai:v1:message-contract-v1".into(),
                    layer: Layer::Adapter,
                    observed_at_ms: qualifications::REVIEWED_AT_MS,
                    expires_at_ms: None,
                },
                reason: "the message adapter has no opaque compaction continuation contract".into(),
            },
        });
        let preparation = Preparation {
            identity: identity.clone(),
            configuration_revision: connection.configuration_revision(
                model,
                (!normalized_effort.is_empty()).then_some(normalized_effort.as_str()),
            )?,
            requirements: Requirements::default(),
            // Embedded limits are pinned build inputs. Keep offline use explicit
            // and expose source staleness; adapter expiry never uses this grace.
            allow_stale_for_ms: u64::MAX,
        };
        let mut cache = self.cache.borrow_mut();
        cache
            .replace_route(&identity, records)
            .map_err(|_| failure("the model metadata sources are invalid"))?;
        cache.prepare(preparation, now_ms).map_err(|_| tembra::Error::new(
            tembra::Code::Unavailable, "providers.model_metadata",
            "required model limits are unavailable for this exact route; supply model metadata or select a qualified model",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use swarmie_model_catalog::{LimitClaim, Precision};
    use swarmie_model_provider_ai::{ConnectionId, Provider};

    fn fixture() -> (SavedAccount, Connection, ModelId) {
        let mut saved = super::super::tests::account("deployment", 1);
        saved.account.provider_id = "openai".into();
        saved.provider = Provider::OpenAi;
        saved.endpoint = "https://api.openai.com/v1".into();
        let model = ModelId::new("gpt-5").unwrap();
        let mut connection = Connection::new(
            ConnectionId::new("deployment").unwrap(),
            Provider::OpenAi,
            [model.clone()],
        );
        connection.endpoint = Some(saved.endpoint.clone());
        (saved, connection, model)
    }

    #[test]
    fn selected_metadata_reuses_one_snapshot_and_cannot_cross_model_or_endpoint() {
        let (saved, connection, model) = fixture();
        let metadata = ModelMetadata::default();
        let now = swarmie_provider_auth::now_ms();
        let first = metadata
            .prepare(&saved, &connection, &model, "", now)
            .unwrap();
        for _ in 0..1000 {
            assert!(Arc::ptr_eq(
                &first,
                &metadata
                    .prepare(&saved, &connection, &model, "", now)
                    .unwrap()
            ));
        }
        assert!(matches!(
            first.native_compaction(),
            Capability::Unsupported { .. }
        ));
        assert!(metadata.source.get().is_some());
        assert!(matches!(first.tools(), Capability::Supported { .. }));
        assert!(matches!(first.streaming(), Capability::Supported { .. }));
        let other = metadata
            .prepare(&saved, &connection, &model, "high", now)
            .unwrap();
        assert_ne!(first.generation(), other.generation());
        let uppercase = metadata
            .prepare(&saved, &connection, &model, "HIGH", now)
            .unwrap();
        assert!(Arc::ptr_eq(&other, &uppercase));
        let mut custom = connection.clone();
        custom.endpoint = Some("https://deployment.example/v1".into());
        assert!(metadata.prepare(&saved, &custom, &model, "", now).is_err());
        let mut authorized = saved.clone();
        authorized.oauth = true;
        assert!(
            metadata
                .prepare(&authorized, &connection, &model, "", now)
                .is_err()
        );
    }

    #[test]
    fn discovered_limits_are_scoped_to_the_exact_endpoint_and_remain_explicit() {
        let (mut saved, mut connection, model) = fixture();
        saved.endpoint = "http://deployment.example/api".into();
        connection.endpoint = Some(saved.endpoint.clone());
        let claim = |tokens| LimitClaim {
            tokens: std::num::NonZeroU64::new(tokens).unwrap(),
            precision: Precision::Reported,
            evidence: Evidence {
                source: "provider-discovery".into(),
                revision: "fixture".into(),
                layer: Layer::Provider,
                observed_at_ms: 1,
                expires_at_ms: None,
            },
        };
        saved.discovered_limits = Some(DiscoveredLimits {
            endpoint: saved.endpoint.clone(),
            capabilities: BTreeMap::new(),
            models: BTreeMap::from([(
                model.as_str().into(),
                Limits {
                    context: vec![claim(4000)],
                    input: vec![],
                    output: vec![claim(400)],
                },
            )]),
        });
        let metadata = ModelMetadata::default();
        let prepared = metadata
            .prepare(&saved, &connection, &model, "", 10)
            .unwrap();
        assert_eq!(prepared.context().known().unwrap().tokens.get(), 4000);
        saved
            .discovered_limits
            .as_mut()
            .unwrap()
            .capabilities
            .insert(
                model.as_str().into(),
                qualifications::DeclaredCapabilities {
                    tools: Some(true),
                    streaming: Some(true),
                    profile: Some(qualifications::PROFILE.into()),
                    evidence: Evidence {
                        source: "fixture-provider".into(),
                        revision: "1".into(),
                        layer: Layer::Provider,
                        observed_at_ms: 1,
                        expires_at_ms: None,
                    },
                },
            );
        let qualified = metadata
            .prepare(&saved, &connection, &model, "", 10)
            .unwrap();
        assert!(matches!(qualified.tools(), Capability::Supported { .. }));
        assert!(matches!(
            qualified.streaming(),
            Capability::Supported { .. }
        ));
        connection.endpoint = Some("http://different.example/api".into());
        assert!(
            metadata
                .prepare(&saved, &connection, &model, "", 10)
                .is_err()
        );
    }
    #[tokio::test]
    async fn missing_metadata_is_refused_before_credential_access_and_preserves_settings() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct Refuse(Arc<AtomicUsize>);
        impl super::super::secrets::SecretStore for Refuse {
            fn get(&self, _: &str) -> tembra::Result<zeroize::Zeroizing<String>> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Err(failure("credential access must not occur"))
            }
            fn set(&self, _: &str, _: &str) -> tembra::Result<()> {
                panic!("no credential write is allowed");
            }
            fn delete(&self, _: &str) -> tembra::Result<()> {
                panic!("no credential deletion is allowed");
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("providers.json");
        let mut providers = super::super::NativeProviders::new(
            path.clone(),
            "fixture-node".into(),
            "fixture".into(),
            uuid::Uuid::new_v4(),
        );
        let accesses = Arc::new(AtomicUsize::new(0));
        providers.secrets = Arc::new(Refuse(accesses.clone()));
        let mut saved = super::super::tests::account("fixture-account", 1);
        saved.account.provider_id = "openai".into();
        saved.provider = Provider::OpenAi;
        saved.endpoint = "https://deployment.example/v1".into();
        providers.save(&[saved]).await.unwrap();
        let before = std::fs::read(&path).unwrap();
        let error = providers
            .model("fixture", "fixture-account", "model-0", "")
            .await
            .err()
            .unwrap();
        assert_eq!(error.reason, "providers.model_metadata");
        assert_eq!(accesses.load(Ordering::SeqCst), 0);
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
}
