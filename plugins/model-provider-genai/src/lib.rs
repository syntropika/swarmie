//! Provider inference through a compiled Tembra Core Plugin. Contains no Agent Loop,
//! session store or Durable Tasks ownership. Native replacement requires a fresh Host.

mod backend;
mod configuration;
mod conversion;
mod error;
mod generation;
mod service;

pub use configuration::{Configuration, Connection, Credentials, EnvironmentCredentials, Limits};
pub use service::{facet, facet_with_credentials};

pub const PLUGIN_ID: &str = "swarmie.model-provider-genai";
pub const FACET_ID: &str = "swarmie.model-provider-genai.runtime";
pub const PROVIDER_CONTRACT: &str = "swarmie.model-provider-genai.provider";
pub const GENERATION_CONTRACT: &str = "swarmie.model-provider-genai.generation";
