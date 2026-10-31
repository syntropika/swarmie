//! Managed conversation sessions over the existing history, Agent Loop and Context capabilities.
mod conversations;
mod execution;

pub use conversations::{ExecutionPolicy, orbs};
pub use execution::{ConversationExecution, ExecutionOwner, ExecutionScope};

/// Start admitted work and checkpoint recovery after the complete host composition is installed.
/// Repeated calls do not retry held or uncertain tasks.
pub async fn start(host: &tembra::Host) -> tembra::Result<()> {
    conversations::tasks::start(host).await
}
