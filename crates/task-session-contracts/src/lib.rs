//! Durable task admission and store-scoped routing to session history.
//!
//! Pending intents precede task creation. A confirmed binding is mandatory at an effect
//! boundary, including recovery. Confirmation does not authorize an effect or prove cancellation
//! settlement. These Contracts provide no transaction spanning task and session stores.

mod admission;
mod publisher;

pub use admission::*;
pub use publisher::*;
