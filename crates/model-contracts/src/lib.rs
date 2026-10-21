//! Provider-neutral inference vocabulary and Tembra contracts.
//!
//! Inference DTOs contain no session persistence or durable task policy. Provider-specific
//! endpoints use the same schemas under distinct Contract identities and Registry entries.

mod contract;
mod endpoint;
mod message;
mod model;
mod stream;

pub use contract::*;
pub use endpoint::*;
pub use message::*;
pub use model::*;
pub use stream::*;

/// Normalizes disjoint internally tagged enums into Tembra's supported schema profile.
pub(crate) fn tagged_union(schema: &mut tembra::schemars::Schema) {
    if let Some(object) = schema.as_object_mut()
        && let Some(branches) = object.remove("oneOf")
    {
        object.insert("anyOf".into(), branches);
    }
}
