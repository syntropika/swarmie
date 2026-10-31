use crate::catalog::{
    Capability, Evidence, Limit, LimitClaim, Metadata, ModelIdentity, NativeCompaction,
    NativeOutputMaximum, SourceRecord, UnknownReason,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ResolutionError {
    #[error("invalid model identity")]
    InvalidIdentity,
    #[error("invalid metadata evidence")]
    InvalidEvidence,
    #[error("invalid model configuration revision")]
    InvalidConfiguration,
    #[error("model configuration does not match the qualification for {0}")]
    ConfigurationMismatch(&'static str),
    #[error("duplicate metadata source record")]
    DuplicateSource,
    #[error("conflicting adapter qualification for {0}")]
    ConflictingCapability(&'static str),
    #[error("required model limit is unknown: {field} ({reason:?})")]
    MissingLimit {
        field: &'static str,
        reason: UnknownReason,
    },
    #[error("required model capability is unavailable: {0}")]
    MissingCapability(&'static str),
    #[error("invalid catalog source data")]
    InvalidSource,
}

/// Limits are required for ordinary operational preparation. Consumers can
/// request capability qualification before dispatch, without provider heuristics.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requirements {
    pub context: bool,
    pub input: bool,
    pub output: bool,
    pub tools: bool,
    pub streaming: bool,
    pub native_compaction: bool,
}

impl Default for Requirements {
    fn default() -> Self {
        Self {
            context: true,
            input: true,
            output: true,
            tools: false,
            streaming: false,
            native_compaction: false,
        }
    }
}

/// The configuration revision must change when request-affecting configuration
/// changes. It contains no credentials. Source replacement separately invalidates
/// the cache. Expiry is checked at preparation, never by a hidden request hook.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preparation {
    pub identity: ModelIdentity,
    pub configuration_revision: String,
    pub requirements: Requirements,
    pub allow_stale_for_ms: u64,
}

/// Immutable metadata of one prepared operational model. The generation is a
/// SHA-256 digest of the selection, resolution policy, evidence and result.
/// This object grants no inference authority and owns no physical invocation.
#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub(crate) preparation: Preparation,
    pub(crate) generation: String,
    pub(crate) context: Metadata<Limit>,
    pub(crate) input: Metadata<Limit>,
    pub(crate) output: Metadata<Limit>,
    pub(crate) tools: Capability<()>,
    pub(crate) streaming: Capability<()>,
    pub(crate) native_compaction: Capability<NativeCompaction>,
    pub(crate) native_output_maximum: Capability<NativeOutputMaximum>,
    pub(crate) discarded_limits: Vec<LimitClaim>,
    pub(crate) stale_evidence: Vec<Evidence>,
    pub(crate) prepared_at_ms: u64,
    pub(crate) valid_until_ms: Option<u64>,
}

impl Snapshot {
    pub fn preparation(&self) -> &Preparation {
        &self.preparation
    }
    pub fn identity(&self) -> &ModelIdentity {
        &self.preparation.identity
    }
    pub fn generation(&self) -> &str {
        &self.generation
    }
    pub fn context(&self) -> &Metadata<Limit> {
        &self.context
    }
    pub fn input(&self) -> &Metadata<Limit> {
        &self.input
    }
    pub fn output(&self) -> &Metadata<Limit> {
        &self.output
    }
    pub fn tools(&self) -> &Capability<()> {
        &self.tools
    }
    pub fn streaming(&self) -> &Capability<()> {
        &self.streaming
    }
    pub fn native_compaction(&self) -> &Capability<NativeCompaction> {
        &self.native_compaction
    }

    pub fn native_output_maximum(&self) -> &Capability<NativeOutputMaximum> {
        &self.native_output_maximum
    }

    pub fn discarded_limits(&self) -> &[LimitClaim] {
        &self.discarded_limits
    }
    pub fn stale_evidence(&self) -> &[Evidence] {
        &self.stale_evidence
    }
    pub fn prepared_at_ms(&self) -> u64 {
        self.prepared_at_ms
    }
    pub fn valid_until_ms(&self) -> Option<u64> {
        self.valid_until_ms
    }

    /// A local validity check for a later admission. It performs no refresh.
    /// An expired snapshot does not authorize replay of an admitted operation.
    pub fn is_valid_at(&self, now_ms: u64) -> bool {
        now_ms >= self.prepared_at_ms && self.valid_until_ms.is_none_or(|until| now_ms < until)
    }
}

/// A bounded cache over an indexed, explicitly loaded source set. Replacing the
/// sources is atomic on success. Existing snapshots retain their original values.
/// The host owns synchronization and any remote source fetching.
pub struct Catalog {
    records: BTreeMap<ModelIdentity, Vec<SourceRecord>>,
    cache: BTreeMap<Preparation, Arc<Snapshot>>,
    capacity: usize,
}

impl Catalog {
    pub fn new(capacity: usize) -> Self {
        Self {
            records: BTreeMap::new(),
            cache: BTreeMap::new(),
            capacity,
        }
    }

    pub fn replace_sources(&mut self, records: Vec<SourceRecord>) -> Result<(), ResolutionError> {
        let mut indexed: BTreeMap<ModelIdentity, Vec<SourceRecord>> = BTreeMap::new();
        for mut record in records {
            record.identity.validate()?;
            crate::catalog::resolution::validate_record(&record)?;
            for claims in [
                &mut record.limits.context,
                &mut record.limits.input,
                &mut record.limits.output,
            ] {
                claims.sort_by_cached_key(|claim| {
                    serde_json::to_string(claim).expect("metadata is serializable")
                });
                claims.dedup();
            }
            let entries = indexed.entry(record.identity.clone()).or_default();
            if entries.contains(&record) {
                return Err(ResolutionError::DuplicateSource);
            }
            entries.push(record);
        }
        // Stable source ordering also makes generations independent of load order.
        for records in indexed.values_mut() {
            records.sort_by_cached_key(|record| {
                serde_json::to_string(record).expect("metadata is serializable")
            });
        }
        self.records = indexed;
        self.cache.clear();
        Ok(())
    }

    /// Resolve once at model selection. Retain the returned Arc for ordinary
    /// requests. No source loader, inference call or network client is invoked.
    pub fn prepare(
        &mut self,
        preparation: Preparation,
        now_ms: u64,
    ) -> Result<Arc<Snapshot>, ResolutionError> {
        preparation.identity.validate()?;
        validate_configuration_revision(&preparation.configuration_revision)?;
        if let Some(snapshot) = self.cache.get(&preparation)
            && now_ms >= snapshot.prepared_at_ms
            && snapshot.valid_until_ms.is_none_or(|until| now_ms < until)
        {
            return Ok(snapshot.clone());
        }
        let records = self
            .records
            .get(&preparation.identity)
            .map_or(&[][..], Vec::as_slice);
        let mut snapshot =
            crate::catalog::resolution::resolve(preparation.clone(), records, now_ms)?;
        let digest = serde_json::to_vec(&(
            &snapshot.preparation,
            &snapshot.context,
            &snapshot.input,
            &snapshot.output,
            &snapshot.tools,
            &snapshot.streaming,
            &snapshot.native_compaction,
            &snapshot.native_output_maximum,
            &snapshot.discarded_limits,
            &snapshot.stale_evidence,
        ))
        .expect("metadata is serializable");
        snapshot.generation = format!("{:x}", Sha256::digest(digest));
        let snapshot = Arc::new(snapshot);
        if self.capacity > 0 {
            if self.cache.len() >= self.capacity {
                self.cache.pop_first();
            }
            self.cache.insert(preparation, snapshot.clone());
        }
        Ok(snapshot)
    }

    pub fn invalidate(&mut self, identity: &ModelIdentity) {
        self.cache.retain(|key, _| &key.identity != identity);
    }

    /// Replace one exact route without invalidating other prepared selections.
    /// An unchanged source set keeps the cached snapshot. Failed validation leaves
    /// all sources and snapshots unchanged.
    pub fn replace_route(
        &mut self,
        identity: &ModelIdentity,
        records: Vec<SourceRecord>,
    ) -> Result<(), ResolutionError> {
        identity.validate()?;
        if records.iter().any(|record| &record.identity != identity) {
            return Err(ResolutionError::InvalidIdentity);
        }
        let mut candidate = Self::new(0);
        candidate.replace_sources(records)?;
        let entries = candidate.records.remove(identity).unwrap_or_default();
        if self.records.get(identity).map_or(&[][..], Vec::as_slice) == entries.as_slice() {
            return Ok(());
        }
        if entries.is_empty() {
            self.records.remove(identity);
        } else {
            self.records.insert(identity.clone(), entries);
        }
        self.invalidate(identity);
        Ok(())
    }
}

pub(crate) fn validate_configuration_revision(revision: &str) -> Result<(), ResolutionError> {
    if revision.trim().is_empty() || revision.len() > 4096 || revision.chars().any(char::is_control)
    {
        return Err(ResolutionError::InvalidConfiguration);
    }
    Ok(())
}
