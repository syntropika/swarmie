//! Replaceable, bounded agent turns over Tembra operations and the Durable Tasks scheduler.
//!
//! TaskStorage owns task progress and SessionStore owns history. The selected joint capability
//! commits prepared intents with task settlement; independent providers use immediate intents.
//! An interrupted external operation can remain uncertain and require confirmed evidence.

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
mod terminal_recovery;
mod tool_steps;
mod tools;
mod workflow;

#[cfg(feature = "javascript")]
pub use javascript::register_agent_hooks;

pub use service::{HistoryCommit, Options, facet, facet_with_options};

/// Compiled plugin identity. Native implementation replacement requires a fresh Host.
pub const PLUGIN_ID: &str = "swarmie.agent-loop-default";

/// Native Facet identity advertised by the compiled application catalog.
pub const FACET_ID: &str = "swarmie.agent-loop-default";

/// Canonical task definition shared by compatible loop implementations.
pub const TURN_KIND: &str = "swarmie.agent-turn";
