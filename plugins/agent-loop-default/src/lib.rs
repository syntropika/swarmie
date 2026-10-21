//! Replaceable, bounded agent turns over Tembra operations and the Durable Tasks scheduler.
//!
//! Session history and task transitions have independent commit authorities. Stable receipts
//! reconcile them; an interrupted external operation can remain uncertain and require evidence.

#[cfg(feature = "javascript")]
mod javascript;

mod admission;
mod context;
mod journal;
mod model;
mod model_steps;
mod observers;
mod service;
mod state;
mod tool_steps;
mod tools;
mod workflow;

#[cfg(feature = "javascript")]
pub use javascript::register_agent_hooks;

pub use service::facet;

/// Compiled plugin identity. Native implementation replacement requires a fresh Host.
pub const PLUGIN_ID: &str = "swarmie.agent-loop-default";

/// Native Facet identity advertised by the compiled application catalog.
pub const FACET_ID: &str = "swarmie.agent-loop-default";

/// Canonical task definition shared by compatible loop implementations.
pub const TURN_KIND: &str = "swarmie.agent-turn";
