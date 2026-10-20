//! Pure application deployment composition, separate from Tembra object readiness and lifecycle.
//!
//! Resolution validates immutable metadata, selects configured instances and records why each
//! deployment is enabled, disabled or unavailable. Selection does not establish runtime readiness.

mod error;
mod model;
mod resolve;
mod validation;

pub use error::CompositionError;
pub use model::*;
pub use resolve::resolve;

pub use swarmie_plugin_contracts::{FacetTarget, PluginKind};
