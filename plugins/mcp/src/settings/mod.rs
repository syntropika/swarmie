//! Principal-scoped MCP configuration and explicit connection administration.
mod administration;
mod orb;
mod store;
mod types;

pub use orb::{SETTINGS_KEY, SETTINGS_SERVICE, settings_orb};
pub use store::{DefinitionStore, MemoryDefinitions};
pub use types::{Definition, SaveDefinition, Scope, ServerAction, ServerStatus};

mod file;

/// Host-selected native MCP definition file.
pub use file::FileDefinitions;
