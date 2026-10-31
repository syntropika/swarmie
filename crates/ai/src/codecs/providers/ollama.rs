pub mod chat;
pub mod embedding;
pub mod extension;
pub mod model_listing;
pub mod streaming;
pub mod wire;

pub use chat::Chat;
pub use embedding::{
    ALL_MINILM, BGE_M3, EMBEDDINGGEMMA, EmbeddingResponse, Embeddings, MXBAI_EMBED_LARGE,
    NOMIC_EMBED_TEXT, QWEN3_EMBEDDING,
};
pub use model_listing::{ListModelEntry, ListModelsResponse, Models};
pub use wire::OllamaConfig;

const OLLAMA_API_BASE_URL: &str = "http://localhost:11434";

pub const PROVIDER_NAME: &str = "ollama";

pub const LLAMA3_2: &str = "llama3.2";
pub const LLAMA3_1: &str = "llama3.1";
pub const LLAMA3_3: &str = "llama3.3";
pub const LLAMA4: &str = "llama4";
pub const LLAVA: &str = "llava";
pub const MISTRAL: &str = "mistral";
pub const MISTRAL_SMALL3_2: &str = "mistral-small3.2";
pub const GEMMA3: &str = "gemma3";
pub const GEMMA4: &str = "gemma4";
pub const QWEN3: &str = "qwen3";
pub const QWEN3_5: &str = "qwen3.5";
pub const QWEN3_6: &str = "qwen3.6";
pub const QWEN3_8: &str = "qwen3.8";
pub const QWEN3_CODER: &str = "qwen3-coder";
pub const DEEPSEEK_R1: &str = "deepseek-r1";
pub const DEEPSEEK_V3_1: &str = "deepseek-v3.1";
pub const GPT_OSS: &str = "gpt-oss";
pub const PHI4: &str = "phi4";
