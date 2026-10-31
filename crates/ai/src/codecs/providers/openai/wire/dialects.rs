use crate::codecs::completion::FinishReason;

use super::{
    Auth, AuthAlternative, BodyRewrite, Dialect, DimensionsField, EmbeddingQuirks, ImageBody,
    OutputCap, Quirks, RerankQuirks, ResponsesQuirks, Route, Routing, SpeechBody, StrictSchema,
    SystemInstructionsPlacement, TranscriptionBody,
};

pub const AZURE_DEFAULT_API_VERSION: &str = "2024-10-21";

pub const OPENAI: Dialect = Dialect {
    base_url_env: Some("OPENAI_BASE_URL"),
    request_id_header: Some("x-request-id"),
    quirks: Quirks {
        completion_route: Route::Responses,
        // OpenAI's reasoning families take `max_completion_tokens`; every
        // compatible gateway on this wire takes `max_tokens`, which is the
        // baseline.
        output_cap: OutputCap::OpenAiReasoningFamilies,
        ..Quirks::openai()
    },
    ..Dialect::gateway(
        crate::codecs::providers::openai::PROVIDER_NAME,
        "https://api.openai.com/v1",
        "OPENAI_API_KEY",
    )
};

pub const AZURE: Dialect = Dialect {
    base_url_env: Some("AZURE_ENDPOINT"),
    // Azure tokens require bearer authentication rather than the api-key header.
    alternate_auth: Some(AuthAlternative {
        api_key_env: "AZURE_TOKEN",
        auth: Auth::Bearer,
    }),
    quirks: Quirks {
        auth: Auth::ApiKeyHeader,
        routing: Routing::AzureDeployment,
        // Verifying a credential without deploying and spending tokens is
        // not offered.
        verify_path: "",
        transcription_path: "/audio/translations",
        embedding: EmbeddingQuirks {
            // The deployment is in the URL, so the body carries no model.
            sends_model_field: false,
            ..EmbeddingQuirks::openai()
        },
        ..Quirks::openai()
    },
    // The base URL is the account's own resource endpoint; there is no
    // shared host, so the dialect names none.
    ..Dialect::gateway("azure.openai", "", "AZURE_API_KEY")
};

pub const DEEPSEEK: Dialect = Dialect {
    quirks: Quirks {
        // DeepSeek accepts json_object through additional_params, not json_schema.
        supports_response_format: false,
        // Strict function calling is a beta feature, served under `/beta`.
        strict_tools_prefix: Some("/beta"),
        strict_schema: StrictSchema::DeepSeek,
        // `/beta` sits at the root, beside the `/v1` alias of the API.
        root_relative_routes: &["/beta/chat/completions"],
        verify_path: "/user/balance",
        rewrite: BodyRewrite::DeepSeek,
        // Its thinking models take `reasoning_content` back on every
        // assistant message.
        reasoning_field: Some("reasoning_content"),
        ..Quirks::openai()
    },
    ..Dialect::gateway(
        crate::codecs::providers::deepseek::PROVIDER_NAME,
        "https://api.deepseek.com",
        "DEEPSEEK_API_KEY",
    )
};

pub const GROQ: Dialect = Dialect {
    request_id_header: Some("x-request-id"),
    quirks: Quirks {
        // Its Responses route takes no `include`.
        responses: ResponsesQuirks {
            reasoning_ciphertext: false,
            ..ResponsesQuirks::openai()
        },
        ..Quirks::openai()
    },
    ..Dialect::gateway("groq", "https://api.groq.com/openai/v1", "GROQ_API_KEY")
};

pub const HYPERBOLIC: Dialect = Dialect {
    quirks: Quirks {
        // Hyperbolic does not support tool calling, and its
        // structured-output support is unverified.
        supports_tools: false,
        supports_response_format: false,
        completion_path: "/v1/chat/completions",
        embeddings_path: "/v1/embeddings",
        models_path: "/v1/models",
        verify_path: "/models",
        image_generation_path: "/v1/image/generation",
        audio_generation_path: "/v1/audio/generation",
        image_body: ImageBody::Hyperbolic,
        speech_body: SpeechBody::Hyperbolic,
        ..Quirks::openai()
    },
    // The bare host: the chat path carries its own `/v1`.
    ..Dialect::gateway(
        "hyperbolic",
        "https://api.hyperbolic.xyz",
        "HYPERBOLIC_API_KEY",
    )
};

pub const MIRA: Dialect = Dialect {
    quirks: Quirks {
        // The gateway rejects tool parameters, OpenAI structured-output
        // parameters, and unknown parameters such as `stream_options`.
        supports_tools: false,
        supports_response_format: false,
        stream_include_usage: false,
        completion_path: "/v1/chat/completions",
        models_path: "/v1/models",
        verify_path: "/user-credits",
        // The gateway can answer with a bare JSON string.
        accepts_bare_string_reply: true,
        rewrite: BodyRewrite::Mira,
        ..Quirks::openai()
    },
    ..Dialect::gateway("mira", "https://api.mira.network", "MIRA_API_KEY")
};

pub const PERPLEXITY: Dialect = Dialect {
    quirks: Quirks {
        supports_tools: false,
        supports_response_format: false,
        stream_include_usage: false,
        // No endpoint checks a credential without spending tokens.
        verify_path: "",
        rewrite: BodyRewrite::Perplexity,
        ..Quirks::openai()
    },
    ..Dialect::gateway(
        "perplexity",
        "https://api.perplexity.ai",
        "PERPLEXITY_API_KEY",
    )
};

pub const TOGETHER: Dialect = Dialect {
    quirks: Quirks {
        // Structured-output support is per model on Together, so the schema
        // is dropped with a warning rather than sent and rejected.
        supports_response_format: false,
        completion_path: "/v1/chat/completions",
        embeddings_path: "/v1/embeddings",
        models_path: "/v1/models",
        verify_path: "/models",
        embedding: EmbeddingQuirks {
            requires_usage: false,
            supports_encoding_format: false,
            supports_user: false,
            ..EmbeddingQuirks::openai()
        },
        ..Quirks::openai()
    },
    // The bare host: every path carries its own `/v1`.
    ..Dialect::gateway("together", "https://api.together.xyz", "TOGETHER_API_KEY")
};

pub const HUGGINGFACE: Dialect = Dialect {
    quirks: Quirks {
        supports_response_format: false,
        // Chat lives under the router's `/v1`; verification, transcription
        // and image generation are root-relative, so the prefix cannot live
        // in the base URL.
        completion_path: "/v1/chat/completions",
        verify_path: "/api/whoami-v2",
        // Transcription and image generation address `/{model}` at the
        // router root, not a fixed path under `/v1`.
        model_is_modality_path: true,
        // The router's image endpoint takes none of OpenAI's fields and
        // answers with the image bytes rather than a JSON envelope.
        image_body: ImageBody::HuggingFace,
        rewrite: BodyRewrite::HuggingFaceRouter,
        ..Quirks::openai()
    },
    ..Dialect::gateway(
        "huggingface",
        "https://router.huggingface.co",
        "HUGGINGFACE_API_KEY",
    )
};

pub const LLAMACPP: Dialect = Dialect {
    base_url_env: Some("LLAMACPP_API_BASE_URL"),
    quirks: Quirks {
        // A server started without `--api-key` rejects a request that
        // carries an `Authorization` header.
        auth: Auth::OptionalBearer,
        supports_image_tool_results: true,
        verify_path: "/props",
        // These management routes live at the server root, outside /v1.
        root_relative_routes: &[
            "/props",
            "/health",
            "/slots",
            "/metrics",
            "/tokenize",
            "/detokenize",
            "/apply-template",
            "/infill",
            "/lora-adapters",
        ],
        rewrite: BodyRewrite::LlamaCpp,
        rerank: RerankQuirks {
            // `llama-server` serves one rerank handler behind four aliases
            // (`/rerank`, `/reranking`, `/v1/rerank`, `/v1/reranking`); the
            // base URL already carries `/v1`.
            path: "/rerank",
            // This is a batching hint; the server documents no document-count cap.
            max_documents: 1024,
            sends_model_field: true,
        },
        embedding: EmbeddingQuirks {
            // `llama-server`'s embeddings handler reads no width field, so
            // sending one would leave `ndims()` describing vectors the
            // server never returned.
            dimensions: DimensionsField::Ignored,
            ..EmbeddingQuirks::openai()
        },
        ..Quirks::openai()
    },
    ..Dialect::gateway("llamacpp", "http://localhost:8080/v1", "LLAMACPP_API_KEY")
};

pub const MISTRAL: Dialect = Dialect {
    request_id_header: Some("mistral-correlation-id"),
    quirks: Quirks {
        // Mistral reports usage on its final chunk without asking.
        stream_include_usage: false,
        // Fold later system messages into the leading one.
        later_system: crate::codecs::completion::LaterSystem::Leading,
        completion_path: "/v1/chat/completions",
        embeddings_path: "/v1/embeddings",
        models_path: "/v1/models",
        verify_path: "/v1/models",
        transcription_path: "/v1/audio/transcriptions",
        rewrite: BodyRewrite::Mistral,
        embedding: EmbeddingQuirks {
            max_documents: 256,
            supports_user: false,
            // Codestral Embed takes its width as `output_dimension`.
            dimensions: DimensionsField::OutputDimension,
            widths: super::modality::MISTRAL_EMBEDDING_WIDTHS,
            ..EmbeddingQuirks::openai()
        },
        ..Quirks::openai()
    },
    // The bare host: every path carries its own `/v1`.
    ..Dialect::gateway("mistral", "https://api.mistral.ai", "MISTRAL_API_KEY")
};

pub const OPENROUTER: Dialect = Dialect {
    quirks: Quirks {
        stream_include_usage: false,
        verify_path: "/key",
        // A gateway forwards its upstream's own finish reason.
        native_finish_reason: true,
        response_format_with_tools: true,
        accepts_file_ids: false,
        rewrite: BodyRewrite::OpenRouter,
        // Its speech-to-text route takes the audio base64 in a JSON body,
        // not a multipart upload.
        transcription_body: TranscriptionBody::InputAudioJson,
        embedding: EmbeddingQuirks {
            requires_usage: false,
            ..EmbeddingQuirks::openai()
        },
        // The Responses route accepts system instructions as input messages.
        responses: ResponsesQuirks {
            system_instructions: SystemInstructionsPlacement::InputSystemMessages,
            ..ResponsesQuirks::openai()
        },
        ..Quirks::openai()
    },
    ..Dialect::gateway(
        "openrouter",
        "https://openrouter.ai/api/v1",
        "OPENROUTER_API_KEY",
    )
};

pub const VENICE: Dialect = Dialect {
    base_url_env: Some("VENICE_BASE_URL"),
    quirks: Quirks {
        image_generation_path: "/image/generate",
        // Its own endpoint, so its own body and its own reply: `width`/
        // `height` rather than OpenAI's `size`, and `images` holding the
        // base64 payloads themselves.
        image_body: ImageBody::Venice,
        ..Quirks::openai()
    },
    ..Dialect::gateway("venice", "https://api.venice.ai/api/v1", "VENICE_API_KEY")
};

pub const DOUBLEWORD: Dialect = Dialect {
    base_url_env: Some("DOUBLEWORD_BASE_URL"),
    quirks: Quirks {
        embedding: EmbeddingQuirks {
            requires_usage: false,
            supports_encoding_format: false,
            supports_user: false,
            widths: super::modality::DOUBLEWORD_EMBEDDING_WIDTHS,
            // Doubleword refuses a zero width itself, and stating it here
            // keeps the refusal ahead of a request that cannot succeed.
            refuse_zero_width: Some("to be greater than zero"),
            ..EmbeddingQuirks::openai()
        },
        // Qwen3.5 replies report more reasoning tokens than completion
        // tokens, while `total_tokens` counts only the completion.
        reliable_reasoning_count: false,
        // Its Responses route returns no ciphertext.
        responses: ResponsesQuirks {
            reasoning_ciphertext: false,
            ..ResponsesQuirks::openai()
        },
        ..Quirks::openai()
    },
    ..Dialect::gateway(
        "doubleword",
        "https://api.doubleword.ai/v1",
        "DOUBLEWORD_API_KEY",
    )
};

pub const ZAI: Dialect = Dialect {
    base_url_env: Some("ZAI_API_BASE"),
    quirks: Quirks {
        finishes: &[
            ("model_context_window_exceeded", FinishReason::Length),
            ("sensitive", FinishReason::ContentFilter),
        ],
        ..Quirks::openai()
    },
    ..Dialect::gateway(
        crate::codecs::providers::zai::PROVIDER_NAME,
        "https://api.z.ai/api/paas/v4",
        "ZAI_API_KEY",
    )
};

pub const ZAI_CODING: Dialect = Dialect {
    base_url: "https://api.z.ai/api/coding/paas/v4",
    ..ZAI
};

pub const MINIMAX: Dialect = Dialect {
    base_url_env: Some("MINIMAX_API_BASE"),
    quirks: Quirks {
        image_generation_path: "/image_generation",
        audio_generation_path: "/t2a_v2",
        image_body: ImageBody::MiniMax,
        speech_body: SpeechBody::MiniMax,
        // Its Responses route documents no `include`.
        responses: ResponsesQuirks {
            reasoning_ciphertext: false,
            ..ResponsesQuirks::openai()
        },
        ..Quirks::openai()
    },
    ..Dialect::gateway(
        crate::codecs::providers::minimax::PROVIDER_NAME,
        "https://api.minimax.io/v1",
        "MINIMAX_API_KEY",
    )
};

pub const MINIMAX_CHINA: Dialect = Dialect {
    base_url: "https://api.minimax.cn/v1",
    ..MINIMAX
};

pub const MOONSHOT: Dialect = Dialect {
    base_url_env: Some("MOONSHOT_API_BASE"),
    quirks: Quirks {
        // Moonshot rejects `json_schema` response formats.
        supports_response_format: false,
        rewrite: BodyRewrite::Moonshot,
        // Its Responses input reads a `developer` message as a system
        // instruction and takes no `system` role, and its `include` takes
        // web search fields only.
        responses: ResponsesQuirks {
            system_role: "developer",
            reasoning_ciphertext: false,
            ..ResponsesQuirks::openai()
        },
        ..Quirks::openai()
    },
    ..Dialect::gateway(
        crate::codecs::providers::moonshot::PROVIDER_NAME,
        "https://api.moonshot.ai/v1",
        "MOONSHOT_API_KEY",
    )
};

pub const MOONSHOT_CHINA: Dialect = Dialect {
    base_url: "https://api.moonshot.cn/v1",
    ..MOONSHOT
};

pub const XIAOMIMIMO: Dialect = Dialect {
    base_url_env: Some("XIAOMI_MIMO_API_BASE"),
    quirks: Quirks {
        // Every MiMo model takes `reasoning_content` back on every assistant
        // message (the protocol).
        reasoning_field: Some("reasoning_content"),
        ..Quirks::openai()
    },
    ..Dialect::gateway(
        crate::codecs::providers::xiaomimimo::PROVIDER_NAME,
        "https://api.xiaomimimo.com/v1",
        "XIAOMI_MIMO_API_KEY",
    )
};

pub const COHERE: Dialect = Dialect::gateway(
    "cohere",
    "https://api.cohere.ai/compatibility/v1",
    "COHERE_API_KEY",
);

pub const OLLAMA: Dialect = Dialect {
    quirks: Quirks {
        // A local daemon takes no credential; a proxied one takes a token.
        auth: Auth::OptionalBearer,
        rewrite: BodyRewrite::Ollama,
        // A daemon is usually local; the catalog's prices are Ollama Cloud's.
        catalog_priced: false,
        ..Quirks::openai()
    },
    ..Dialect::gateway(
        crate::codecs::providers::ollama::PROVIDER_NAME,
        "http://localhost:11434/v1",
        "OLLAMA_API_KEY",
    )
};

pub const OPENCODE: Dialect = Dialect {
    base_url_env: Some("OPENCODE_BASE_URL"),
    quirks: Quirks {
        routes_by_catalog: true,
        // Each API sits under its own prefix; the model listing sits at the
        // root.
        completion_path: "/openai/v1/chat/completions",
        models_path: "/v1/models",
        // The model listing is public, and nothing else checks a
        // credential without spending tokens.
        verify_path: "",
        responses: ResponsesQuirks {
            path: "/openai/v1/responses",
            ..ResponsesQuirks::openai()
        },
        ..Quirks::openai()
    },
    ..Dialect::gateway(
        "opencode",
        "https://opencode.ai/inference",
        "OPENCODE_API_KEY",
    )
};

pub const OPENCODE_GO: Dialect = Dialect {
    name: "opencode-go",
    base_url: "https://opencode.ai/inference/go",
    base_url_env: Some("OPENCODE_GO_BASE_URL"),
    quirks: Quirks {
        session_header: Some("x-opencode-session"),
        ..OPENCODE.quirks
    },
    ..OPENCODE
};

pub fn by_name(name: &str) -> Option<&'static Dialect> {
    all().find(|dialect| dialect.name == name)
}

pub fn all() -> impl Iterator<Item = &'static Dialect> {
    crate::codecs::providers::registry::OPENAI_DIALECTS
        .iter()
        .copied()
}
