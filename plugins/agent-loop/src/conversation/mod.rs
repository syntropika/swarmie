//! Optional history integration over an ordinary Tembra Conversation reference.
//! Checkpoints contain fixed history and prepared-request references, not transcripts.
//! The same engine still accepts standalone inline AgentContext values.
//!
//! ```
//! use swarmie_agent_loop::{Agent, conversation::{History, Request, ExecutionResult}};
//!
//! async fn reply(
//!     history: &History,
//!     context: &tembra::Context,
//!     agent: &Agent,
//!     request: Request,
//! ) -> tembra::Result<ExecutionResult> {
//!     Ok(history.execute(context, agent, request).await)
//! }
//! ```
//!
//! Write messages using [`Record::Message`]. This default projection accepts the
//! typed agent record format and refuses unrecognized payloads before dispatch.
//! A fixed prefix does not lock a conversation. A competing append causes a
//! confirmed conflict; this adapter never merges it or overwrites the new tail.
//! Durable execution requires both optional features and a common storage owner
//! supporting joint history/checkpoint commits.

mod checkpoint;
mod driver;
#[cfg(feature = "context-projection")]
mod projection;
mod restore;
mod settlement;
mod writer;

#[cfg(feature = "durable-tasks")]
pub mod durable;

pub use checkpoint::{
    Checkpoint, Completion, ExecutionFailure, ExecutionResult, Pending, Record, Request,
};
pub use driver::History;
pub use settlement::ToolSettlement;
