//! Durable task admission and store-scoped routing to session history.
//!
//! Pending intents precede task creation. A confirmed binding is mandatory at an effect
//! boundary, including recovery. Confirmation does not authorize an effect or prove cancellation
//! settlement. A separately admitted joint capability can commit typed task and history
//! writes only when their selected providers share one physical transaction owner.

mod admission;
mod commit;
mod publisher;

pub use admission::*;
pub use commit::*;
pub use publisher::*;
