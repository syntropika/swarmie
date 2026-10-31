//! Offline final model-content inspection, independent of Cloud or the node vault.
//! A prepared submission owns immutable bytes and one physical dispatch opportunity.

mod gate;
mod scanner;

pub use gate::*;
pub use scanner::*;
