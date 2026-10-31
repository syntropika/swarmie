//! Stable application commands. Tembra owns handlers, authority and invocation lifetime.
//! Aliases are presentation metadata; dispatch always selects an exact command identity.

mod catalog;
mod orb;
mod types;

pub use catalog::{Catalog, Registration};
pub use orb::{SERVICE, orb};
pub use types::{Command, CommandId, Invocation, Target};

pub const COMPACT: &str = "swarmie.context.compact";

fn invalid(message: impl ToString) -> tembra::Error {
    tembra::Error::invalid("swarmie.commands.input", message.to_string())
}
