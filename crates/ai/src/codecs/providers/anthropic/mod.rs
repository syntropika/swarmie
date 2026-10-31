pub mod completion;
pub mod extension;
pub mod modality;
mod options;
pub mod streaming;
pub mod wire;

pub const PROVIDER_NAME: &str = "anthropic";

pub use completion::{
    CLAUDE_FABLE_5, CLAUDE_FABLE_5_1, CLAUDE_HAIKU_4_5, CLAUDE_OPUS_4_6, CLAUDE_OPUS_4_7,
    CLAUDE_OPUS_4_8, CLAUDE_OPUS_5, CLAUDE_OPUS_5_5, CLAUDE_SONNET_4_6, CLAUDE_SONNET_5,
    CLAUDE_SONNET_5_5,
};
pub use modality::{Models, Verify};
pub use wire::{
    ANTHROPIC, Addressing, AnthropicConfig, Dialect, MaxTokens, Messages, Prepared, Quirks,
    ToolInputStreaming, compatible,
};
