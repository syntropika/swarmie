//! Model metadata preparation without inference, credentials or a network runtime.
//!
//! Prepare a snapshot when a model is selected. Retain that snapshot for requests.
//! Sources are explicit inputs. Reading a prepared snapshot does not refresh sources.

mod cache;
mod identity;
mod listing;
mod metadata;
#[path = "../../catalog/model_schema.rs"]
mod model_schema;
mod resolution;
pub mod sources;
mod spec;

pub use cache::{Catalog, Preparation, Requirements, ResolutionError, Snapshot};
pub use identity::ModelIdentity;
pub use listing::{ListedModel, ListingProtocol, ModelListing};
pub use metadata::{
    Capability, Evidence, Layer, Limit, LimitClaim, Limits, Metadata, NativeCompaction,
    NativeOutputMaximum, OutputAccounting, Precision, SourceRecord, UnknownReason,
};
pub use model_schema::{
    Compatibility, Interleaved, Modalities, ModelLimits, ModelSpec, Pricing, ReasoningOption,
};
pub use sources::{ModelDescription, ModelsDev};
pub use spec::{ModelFeature, ModelFeatureError, ModelFeatureStatus};
