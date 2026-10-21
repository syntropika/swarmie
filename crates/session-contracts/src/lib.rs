//! Provider-independent immutable session history and bounded deterministic projections.
//! SQLite and inference are deliberately outside this contract crate.

mod api;
mod events;
mod projection;
mod transport;

pub use api::*;
pub use events::*;
pub use projection::*;
pub use transport::*;
