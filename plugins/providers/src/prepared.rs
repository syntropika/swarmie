use crate::{MetadataFreshness, ModelResponse, OperationId, invalid};
use futures::future::LocalBoxFuture;
use serde::{Deserialize, Serialize};
use swarmie_model_catalog::{
    Capability, Limit, ModelIdentity, NativeCompaction, NativeOutputMaximum, Snapshot,
};

/// Safe description from a validated operational Snapshot. Receiving this DTO
/// does not establish catalog authority or grant model/operator permission.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationalModel {
    pub identity: ModelIdentity,
    pub generation: String,
    pub metadata_freshness: MetadataFreshness,
    pub context: Limit,
    pub input: Limit,
    pub output: Limit,
    pub native_compaction: Capability<NativeCompaction>,
    pub native_output_maximum: Capability<NativeOutputMaximum>,
}

impl OperationalModel {
    pub fn from_snapshot(snapshot: &Snapshot) -> tembra::Result<Self> {
        let limit = |value: &swarmie_model_catalog::Metadata<Limit>| {
            value
                .known()
                .cloned()
                .ok_or_else(|| invalid("required operational model limit is unknown"))
        };
        Ok(Self {
            identity: snapshot.identity().clone(),
            generation: snapshot.generation().into(),
            metadata_freshness: MetadataFreshness::from_snapshot(snapshot)?,
            context: limit(snapshot.context())?,
            input: limit(snapshot.input())?,
            output: limit(snapshot.output())?,
            native_compaction: snapshot.native_compaction().clone(),
            native_output_maximum: snapshot.native_output_maximum().clone(),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CountPrecision {
    Exact,
    Estimated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentKind {
    Instructions,
    Conversation,
    Tools,
    Other,
}

/// Disjoint attribution from the final provider payload. MCP tool definitions
/// belong to Tools; they are not counted a second time as another category.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Component {
    pub kind: ComponentKind,
    pub tokens: u64,
    pub bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Measurement {
    pub input_tokens: u64,
    pub final_payload_bytes: u64,
    pub precision: CountPrecision,
    pub method: String,
    pub components: Vec<Component>,
}

impl Measurement {
    pub fn validate(&self) -> tembra::Result<()> {
        let mut kinds = std::collections::BTreeSet::new();
        let mut tokens = 0_u64;
        let mut bytes = 0_u64;
        for component in &self.components {
            if !kinds.insert(component.kind) {
                return Err(invalid("request attribution contains duplicate categories"));
            }
            tokens = tokens
                .checked_add(component.tokens)
                .ok_or_else(|| invalid("token attribution overflow"))?;
            bytes = bytes
                .checked_add(component.bytes)
                .ok_or_else(|| invalid("byte attribution overflow"))?;
        }
        if tokens != self.input_tokens
            || bytes != self.final_payload_bytes
            || self.method.is_empty()
            || self.method.len() > 255
        {
            return Err(invalid(
                "request attribution does not cover its final payload",
            ));
        }
        Ok(())
    }
}

/// The basis of the single numeric output reservation. A sent cap must occur
/// in the retained final payload. An uncapped native maximum requires the same
/// operational snapshot's distinct qualified proof, never its effective minimum.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OutputReservation {
    SentCap,
    NativeMaximum,
}

/// Fingerprint and measurement of the exact private immutable preparation that
/// will execute. Final payload bytes and authentication never enter this value.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreparedDescription {
    pub operation: OperationId,
    pub model: OperationalModel,
    pub fingerprint: [u8; 32],
    pub streaming: bool,
    pub output_reserve: u64,
    pub output_reservation: OutputReservation,
    pub measurement: Measurement,
}

impl PreparedDescription {
    pub fn validate(&self) -> tembra::Result<()> {
        self.model
            .identity
            .validate()
            .map_err(|_| invalid("invalid prepared model identity"))?;
        self.model.metadata_freshness.validate()?;
        if self.output_reservation == OutputReservation::NativeMaximum
            && !matches!(&self.model.native_output_maximum,
                Capability::Supported { value, .. } if value.tokens.get() == self.output_reserve)
        {
            return Err(invalid(
                "output reservation lacks the matching qualified native maximum",
            ));
        }
        if self.model.generation.len() != 64
            || !self.model.generation.bytes().all(|v| v.is_ascii_hexdigit())
            || self.model.input.tokens > self.model.context.tokens
            || self.model.output.tokens > self.model.context.tokens
            || self.output_reserve == 0
            || self.output_reserve > self.model.output.tokens.get()
        {
            return Err(invalid(
                "invalid prepared operational model limits or generation",
            ));
        }
        self.measurement.validate()
    }
}

/// Single-use native handle. The adapter retains its private final bytes and
/// execution context in the closure. No Clone, Debug or serialization is exposed.
pub struct PreparedModelRequest<'a> {
    description: PreparedDescription,
    execute: Box<dyn FnOnce() -> LocalBoxFuture<'a, tembra::Result<ModelResponse>> + 'a>,
}

impl<'a> PreparedModelRequest<'a> {
    pub fn new(
        description: PreparedDescription,
        execute: impl FnOnce() -> LocalBoxFuture<'a, tembra::Result<ModelResponse>> + 'a,
    ) -> tembra::Result<Self> {
        description.validate()?;
        Ok(Self {
            description,
            execute: Box::new(execute),
        })
    }

    pub fn description(&self) -> &PreparedDescription {
        &self.description
    }

    /// Call only after the owner confirms dispatch intent. Dropping an unused
    /// handle performs no inference. An unknown result never grants another handle.
    pub async fn execute(self) -> tembra::Result<ModelResponse> {
        (self.execute)().await
    }
}

/// Pure admission against final preparation, outside retryable persistence closures.
/// Implementations must not call models, tools or resolve metadata over the network.
pub trait PreparationPolicy {
    /// An inspection-only lower bound, prepared and dropped without inference.
    fn fixed_request(
        &self,
        _request: &crate::ModelRequest,
        _description: &PreparedDescription,
    ) -> tembra::Result<Option<crate::ModelRequest>> {
        Ok(None)
    }

    fn check(
        &self,
        context: &tembra::Context,
        description: &PreparedDescription,
        fixed: Option<&PreparedDescription>,
    ) -> tembra::Result<()>;
}
