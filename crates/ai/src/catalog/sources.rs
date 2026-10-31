//! Indexed public model facts and exact route binding. The host owns fetching.
use crate::catalog::model_schema::{Provider, providers, unique_json};
use crate::catalog::{
    Capability, Evidence, Layer, LimitClaim, Limits, ModelIdentity, ModelSpec, Precision,
    ResolutionError, SourceRecord,
};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU64,
    sync::{Arc, OnceLock},
};

type Key = (String, String);

type Origins = BTreeMap<String, Evidence>;

pub fn records_json(bytes: &[u8]) -> Result<Vec<SourceRecord>, ResolutionError> {
    serde_json::from_value(unique_json(bytes).map_err(|_| ResolutionError::InvalidSource)?)
        .map_err(|_| ResolutionError::InvalidSource)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BundledSnapshot {
    schema_version: u32,
    source_url: String,
    upstream_sha256: String,
    content_sha256: String,
    fetched_at_ms: u64,
    expires_at_ms: u64,
    providers: BTreeMap<String, Provider>,
}

/// Model facts and per-field provenance. JSON pointers identify source fields.
pub struct ModelDescription<'a> {
    pub provider: &'a str,
    pub spec: &'a ModelSpec,
    origins: &'a Origins,
}

impl ModelDescription<'_> {
    pub fn evidence(&self, pointer: &str) -> Option<&Evidence> {
        self.origins.get(pointer)
    }
}

/// Immutable source index. Clones share rows. Updates return a new source.
/// Descriptive flags never qualify an adapter or authorize account access.
#[derive(Clone)]
pub struct ModelsDev {
    providers: Arc<BTreeMap<String, Provider>>,
    origins: Arc<BTreeMap<Key, Origins>>,
    pinned: Arc<BTreeMap<Key, BTreeSet<String>>>,
    evidence: Evidence,
}

impl ModelsDev {
    pub fn from_json(bytes: &[u8], evidence: Evidence) -> Result<Self, ResolutionError> {
        if evidence.layer != Layer::Catalog {
            return Err(ResolutionError::InvalidEvidence);
        }
        crate::catalog::resolution::validate_evidence(&evidence)?;
        let providers = providers(bytes).map_err(|_| ResolutionError::InvalidSource)?;
        let mut origins = BTreeMap::new();
        for (provider, entry) in &providers {
            for (id, model) in &entry.models {
                let mut fields = BTreeMap::new();
                field_origins(
                    &serde_json::to_value(model).map_err(|_| ResolutionError::InvalidSource)?,
                    "",
                    &evidence,
                    &mut fields,
                );
                origins.insert((provider.clone(), id.clone()), fields);
            }
        }
        Ok(Self {
            providers: Arc::new(providers),
            origins: Arc::new(origins),
            pinned: Arc::default(),
            evidence,
        })
    }

    /// Parse the embedded source once per process, without runtime I/O.
    pub fn bundled() -> Result<Self, ResolutionError> {
        static BUNDLED: OnceLock<Result<ModelsDev, ResolutionError>> = OnceLock::new();
        BUNDLED
            .get_or_init(|| {
                Self::from_snapshot(include_bytes!(concat!(env!("OUT_DIR"), "/models-dev.json")))
            })
            .clone()
    }

    pub fn from_snapshot(bytes: &[u8]) -> Result<Self, ResolutionError> {
        let snapshot: BundledSnapshot =
            serde_json::from_value(unique_json(bytes).map_err(|_| ResolutionError::InvalidSource)?)
                .map_err(|_| ResolutionError::InvalidSource)?;
        let raw = crate::catalog::model_schema::canonical_bytes(&snapshot.providers)
            .map_err(|_| ResolutionError::InvalidSource)?;
        if snapshot.schema_version != 1
            || snapshot.source_url != "https://models.dev/api.json"
            || format!("{:x}", Sha256::digest(&raw)) != snapshot.content_sha256
            || snapshot.upstream_sha256.len() != 64
            || !snapshot
                .upstream_sha256
                .bytes()
                .all(|c| c.is_ascii_hexdigit())
            || snapshot.expires_at_ms <= snapshot.fetched_at_ms
        {
            return Err(ResolutionError::InvalidSource);
        }
        Self::from_json(
            &raw,
            Evidence {
                source: snapshot.source_url,
                revision: format!("{}:{}", snapshot.upstream_sha256, snapshot.content_sha256),
                layer: Layer::Catalog,
                observed_at_ms: snapshot.fetched_at_ms,
                expires_at_ms: Some(snapshot.expires_at_ms),
            },
        )
    }

    pub fn provider(&self, id: &str) -> Option<&BTreeMap<String, Value>> {
        self.providers.get(id).map(|p| &p.extra)
    }

    pub fn model(&self, provider: &str, id: &str) -> Option<ModelDescription<'_>> {
        let (provider, entry) = self.providers.get_key_value(provider)?;
        Some(ModelDescription {
            provider,
            spec: entry.models.get(id)?,
            origins: self.origins.get(&(provider.clone(), id.to_owned()))?,
        })
    }

    /// Stable provider/model order. Filtering is a model-selection operation.
    pub fn models(&self) -> impl Iterator<Item = ModelDescription<'_>> {
        self.providers.iter().flat_map(move |(provider, entry)| {
            entry
                .models
                .keys()
                .filter_map(move |id| self.model(provider, id))
        })
    }

    /// Overlay a newer public source, retaining pinned reviewed fields.
    /// Unreported rows keep their old evidence, not the new source's freshness.
    pub fn with_models_dev(
        &self,
        bytes: &[u8],
        evidence: Evidence,
    ) -> Result<Self, ResolutionError> {
        if evidence.layer != Layer::Catalog
            || evidence.observed_at_ms <= self.evidence.observed_at_ms
        {
            return Err(ResolutionError::InvalidEvidence);
        }
        let fresh = Self::from_json(bytes, evidence.clone())?;
        self.overlay(
            serde_json::to_value(&*fresh.providers).map_err(|_| ResolutionError::InvalidSource)?,
            evidence,
            false,
        )
    }

    /// Reviewed overrides use models.dev row shape. Supplied fields are pinned,
    /// including false, zero and null. This method does not read credentials.
    pub fn with_overrides(
        &self,
        bytes: &[u8],
        evidence: Evidence,
    ) -> Result<Self, ResolutionError> {
        if !matches!(
            evidence.layer,
            Layer::Configuration | Layer::Provider | Layer::Deployment
        ) {
            return Err(ResolutionError::InvalidEvidence);
        }
        crate::catalog::resolution::validate_evidence(&evidence)?;
        self.overlay(
            unique_json(bytes).map_err(|_| ResolutionError::InvalidSource)?,
            evidence,
            true,
        )
    }

    fn overlay(
        &self,
        patch: Value,
        evidence: Evidence,
        pin: bool,
    ) -> Result<Self, ResolutionError> {
        let patch = patch.as_object().ok_or(ResolutionError::InvalidSource)?;
        let mut document =
            serde_json::to_value(&*self.providers).map_err(|_| ResolutionError::InvalidSource)?;
        let target = document
            .as_object_mut()
            .ok_or(ResolutionError::InvalidSource)?;
        let mut origins = (*self.origins).clone();
        let mut pinned = (*self.pinned).clone();
        for (provider, update) in patch {
            let update = update.as_object().ok_or(ResolutionError::InvalidSource)?;
            let models = update
                .get("models")
                .and_then(Value::as_object)
                .ok_or(ResolutionError::InvalidSource)?;
            let entry = target
                .entry(provider.clone())
                .or_insert_with(|| serde_json::json!({"models":{}}))
                .as_object_mut()
                .ok_or(ResolutionError::InvalidSource)?;
            for (key, value) in update.iter().filter(|(key, _)| key.as_str() != "models") {
                entry.insert(key.clone(), value.clone());
            }
            let rows = entry
                .get_mut("models")
                .and_then(Value::as_object_mut)
                .ok_or(ResolutionError::InvalidSource)?;
            for (id, row) in models {
                if !row.is_object() {
                    return Err(ResolutionError::InvalidSource);
                }
                let key = (provider.clone(), id.clone());
                let held = rows
                    .entry(id.clone())
                    .or_insert_with(|| serde_json::json!({"id":id}));
                merge(
                    held,
                    row,
                    "",
                    &evidence,
                    origins.entry(key.clone()).or_default(),
                    pinned.entry(key).or_default(),
                    pin,
                );
            }
        }
        let providers =
            providers(&serde_json::to_vec(&document).map_err(|_| ResolutionError::InvalidSource)?)
                .map_err(|_| ResolutionError::InvalidSource)?;
        Ok(Self {
            providers: Arc::new(providers),
            origins: Arc::new(origins),
            pinned: Arc::new(pinned),
            evidence: if pin { self.evidence.clone() } else { evidence },
        })
    }

    pub fn bind(&self, identity: ModelIdentity) -> Result<Option<SourceRecord>, ResolutionError> {
        let provider = identity.provider.clone();
        self.bind_provider(identity, &provider)
    }

    /// Bind only after the embedding confirms exact endpoint/deployment applicability.
    /// No aliases, dated-name fallback or provider-family inference is performed.
    pub fn bind_provider(
        &self,
        identity: ModelIdentity,
        provider: &str,
    ) -> Result<Option<SourceRecord>, ResolutionError> {
        identity.validate()?;
        let Some(model) = self.model(provider, &identity.model) else {
            return Ok(None);
        };
        if identity.revision.is_some() {
            return Ok(None);
        }
        let claim = |tokens: Option<u64>, field: &str| {
            tokens
                .and_then(NonZeroU64::new)
                .and_then(|tokens| {
                    model.evidence(field).map(|evidence| LimitClaim {
                        tokens,
                        precision: Precision::Reported,
                        evidence: evidence.clone(),
                    })
                })
                .into_iter()
                .collect()
        };
        let limits = Limits {
            context: claim(model.spec.limit.context, "/limit/context"),
            input: claim(model.spec.limit.input, "/limit/input"),
            output: claim(model.spec.limit.output, "/limit/output"),
        };
        Ok(Some(SourceRecord {
            identity,
            limits,
            tools: Capability::default(),
            streaming: Capability::default(),
            native_output_maximum: Capability::default(),
            native_compaction: Capability::default(),
        }))
    }
}

fn path(parent: &str, key: &str) -> String {
    format!("{parent}/{}", key.replace('~', "~0").replace('/', "~1"))
}

fn field_origins(value: &Value, at: &str, evidence: &Evidence, origins: &mut Origins) {
    if let Value::Object(fields) = value {
        for (key, value) in fields {
            field_origins(value, &path(at, key), evidence, origins);
        }
    } else {
        origins.insert(at.to_owned(), evidence.clone());
    }
}

fn merge(
    held: &mut Value,
    patch: &Value,
    at: &str,
    evidence: &Evidence,
    origins: &mut Origins,
    pins: &mut BTreeSet<String>,
    pin: bool,
) {
    if !pin
        && pins.iter().any(|key| {
            at == key
                || at.starts_with(&format!("{key}/"))
                || (!patch.is_object() && key.starts_with(&format!("{at}/")))
        })
    {
        return;
    }
    if let Value::Object(fields) = patch {
        if !held.is_object() {
            *held = Value::Object(Default::default());
        }
        if let Some(target) = held.as_object_mut() {
            for (key, value) in fields {
                merge(
                    target.entry(key.clone()).or_insert(Value::Null),
                    value,
                    &path(at, key),
                    evidence,
                    origins,
                    pins,
                    pin,
                );
            }
        }
    } else {
        *held = patch.clone();
        origins.retain(|key, _| key != at && !key.starts_with(&format!("{at}/")));
        origins.insert(at.to_owned(), evidence.clone());
        if pin {
            pins.insert(at.to_owned());
        }
    }
}
