//! Adapter-owned exact binding and qualification. The common account owner loads
//! sources and uses the existing Catalog; this module has no resolver or cache.

use super::{StoredConfiguration, catalog, failure, qualifications};
use crate::{Connection, ModelId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use swarmie_model_catalog::{
    Capability, Evidence, Layer, Limits, Preparation, Requirements, SourceRecord,
};
use swarmie_providers::{EmbeddedCatalogBinding, ModelMetadataInput, settings::Account};

#[derive(Clone, Default, Deserialize, Serialize)]
pub(super) struct DiscoveredLimits {
    pub endpoint: String,
    pub models: BTreeMap<String, Limits>,
    #[serde(default)]
    pub capabilities: BTreeMap<String, qualifications::DeclaredCapabilities>,
}

pub(super) fn capture(
    saved: &StoredConfiguration,
    account: &Account,
    connection: &Connection,
    model: &ModelId,
    reasoning: Option<&str>,
) -> tembra::Result<ModelMetadataInput> {
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
    let mut embedded = None;
    let mut standard = false;
    if !saved.oauth
        && let Some(spec) = catalog::specifications().into_iter().find(|spec| {
            spec.id == account.provider_id
                && spec.provider == connection.provider
                && spec.endpoint.trim_end_matches('/') == saved.endpoint.trim_end_matches('/')
        })
    {
        standard = true;
        embedded = spec
            .catalog_provider
            .map(|provider| EmbeddedCatalogBinding {
                provider: provider.into(),
            });
    }
    let mut records = Vec::new();
    let mut declared = None;
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
        &account.provider_id,
        model.as_str(),
        reasoning.unwrap_or_default(),
        standard,
        declared,
    )
    .map_err(failure)?;
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
    Ok(ModelMetadataInput {
        preparation: Preparation {
            identity,
            configuration_revision: connection.configuration_revision(model, reasoning)?,
            requirements: Requirements::default(),
            // Keep the existing explicit offline limit grace. Catalog exposes its
            // stale evidence and never extends executable capability expiry.
            allow_stale_for_ms: u64::MAX,
        },
        embedded,
        records,
    })
}
