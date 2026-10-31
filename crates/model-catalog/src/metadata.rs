use crate::ModelIdentity;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU64;

/// Precision describes a claim, not a guarantee of remote provider behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Precision {
    Reported,
    Conservative,
    Estimated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    Catalog,
    Provider,
    Deployment,
    Configuration,
    Policy,
    Adapter,
}

/// Source version and validity are supplied during preparation. Missing expiry
/// means that the source declares no expiry; it does not mean live verification.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub source: String,
    pub revision: String,
    pub layer: Layer,
    pub observed_at_ms: u64,
    pub expires_at_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimitClaim {
    pub tokens: NonZeroU64,
    pub precision: Precision,
    pub evidence: Evidence,
}

/// All admitted hard ceilings constrain the result. A policy ceiling cannot
/// establish an otherwise unknown model limit. Estimates are diagnostic only.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub context: Vec<LimitClaim>,
    pub input: Vec<LimitClaim>,
    pub output: Vec<LimitClaim>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnknownReason {
    Missing,
    Expired,
    EstimatedOnly,
    Unavailable,
    Unqualified,
    NotYetValid,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Capability<T> {
    Unknown { reason: UnknownReason },
    Unsupported { evidence: Evidence, reason: String },
    Supported { evidence: Evidence, value: T },
}

impl<T> Default for Capability<T> {
    fn default() -> Self {
        Self::Unknown {
            reason: UnknownReason::Missing,
        }
    }
}

/// Adapter qualification for one exact route, including the continuation format.
/// Catalog membership cannot establish this value. The owning adapter must
/// implement the method and preserve the indicated output on subsequent input.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeCompaction {
    pub method: String,
    pub input_format: String,
    pub output_format: String,
    pub continuation_format: String,
    pub max_input_tokens: NonZeroU64,
    pub max_output_tokens: NonZeroU64,
}

/// The complete generated-token domain charged to the output/context budget.
/// A visible-text-only or estimated count cannot establish a native maximum.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputAccounting {
    TotalGeneratedTokens,
}

/// A provider-enforced upper bound when the exact profile sends no output cap.
/// The surrounding capability supplies separate adapter qualification. Effective
/// policy ceilings and ordinary reported output claims cannot establish this fact.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeOutputMaximum {
    pub tokens: NonZeroU64,
    pub configuration_revision: String,
    pub fact: Evidence,
    pub accounting: OutputAccounting,
}

/// One source's assertions bound to an exact operational route by the embedding.
/// Unknown fields are not default capability refusals.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRecord {
    pub identity: ModelIdentity,
    pub limits: Limits,
    pub tools: Capability<()>,
    pub streaming: Capability<()>,
    pub native_compaction: Capability<NativeCompaction>,
    pub native_output_maximum: Capability<NativeOutputMaximum>,
}

/// A usable effective ceiling plus all evidence used to compute it.
/// Input and output are each bounded by context. This does not reserve output
/// tokens or calculate request occupancy; the consumer owns that calculation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limit {
    pub tokens: NonZeroU64,
    pub precision: Precision,
    pub claims: Vec<LimitClaim>,
    pub bounded_by_context: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Metadata<T> {
    Known { value: T },
    Unknown { reason: UnknownReason },
}

impl<T> Metadata<T> {
    pub fn known(&self) -> Option<&T> {
        match self {
            Self::Known { value } => Some(value),
            Self::Unknown { .. } => None,
        }
    }
}
