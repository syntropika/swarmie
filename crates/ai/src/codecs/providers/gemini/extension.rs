use std::collections::BTreeMap;

use serde::de::Error as _;
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;

use crate::codecs::completion::provider_options::{
    ExtensionOptions, ProviderExtension, ReplyExtras,
};
use crate::codecs::message::Api;

use super::ImageSize;

const GENERATE_CONTENT: &str = "gemini.generate_content";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GeminiExt;

impl ProviderExtension for GeminiExt {
    const PROVIDER: &'static str = super::PROVIDER_NAME;
    type Options = GeminiOptions;
    type Extras = GeminiExtras;
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct GeminiOptions {
    #[serde(rename = "*")]
    pub shared: GeminiShared,
    #[serde(rename = "gemini.generate_content")]
    pub generate_content: GenerateContentOptions,
}

impl GeminiOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn store(mut self, store: bool) -> Self {
        self.shared.store = Some(store);
        self
    }

    pub fn label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.shared.labels.insert(key.into(), value.into());
        self
    }

    pub fn generate_content(mut self, section: GenerateContentOptions) -> Self {
        self.generate_content = section;
        self
    }

    fn generation_config_entry(
        mut self,
        set: impl FnOnce(GenerationConfig) -> GenerationConfig,
    ) -> Self {
        let common = std::mem::take(&mut self.generate_content.generation_config.common);
        self.generate_content.generation_config.common = set(common);
        self
    }

    pub fn include_thoughts(self, include: bool) -> Self {
        self.generation_config_entry(|config| config.include_thoughts(include))
    }

    pub fn top_k(self, top_k: u32) -> Self {
        self.generation_config_entry(|config| config.top_k(top_k))
    }

    pub fn presence_penalty(self, penalty: f64) -> Self {
        self.generation_config_entry(|config| config.presence_penalty(penalty))
    }

    pub fn frequency_penalty(self, penalty: f64) -> Self {
        self.generation_config_entry(|config| config.frequency_penalty(penalty))
    }

    pub fn response_logprobs(self, enable: bool) -> Self {
        self.generation_config_entry(|config| config.response_logprobs(enable))
    }

    pub fn logprobs(self, top: u32) -> Self {
        self.generation_config_entry(|config| config.logprobs(top))
    }

    pub fn candidate_count(self, count: CandidateCount) -> Self {
        self.generation_config_entry(|config| config.candidate_count(count))
    }

    pub fn response_modalities(
        self,
        modalities: impl IntoIterator<Item = ResponseModality>,
    ) -> Self {
        self.generation_config_entry(|config| config.response_modalities(modalities))
    }

    pub fn media_resolution(self, resolution: MediaResolution) -> Self {
        self.generation_config_entry(|config| config.media_resolution(resolution))
    }
}

impl ExtensionOptions for GeminiOptions {
    type Ext = GeminiExt;
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct GeminiShared {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<bool>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct GenerateContentOptions {
    #[serde(rename = "generationConfig", skip_serializing_if = "is_default")]
    pub generation_config: GeminiGenerationConfig,
    #[serde(rename = "safetySettings", skip_serializing_if = "Vec::is_empty")]
    pub safety_settings: Vec<SafetySetting>,
}

impl GenerateContentOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn generation_config(mut self, config: GenerationConfig) -> Self {
        self.generation_config.common = config;
        self
    }

    pub fn enable_enhanced_civic_answers(mut self, enable: bool) -> Self {
        self.generation_config.enable_enhanced_civic_answers = Some(enable);
        self
    }

    pub fn safety_setting(mut self, category: HarmCategory, threshold: HarmBlockThreshold) -> Self {
        self.safety_settings
            .push(SafetySetting::new(category, threshold));
        self
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct GeminiGenerationConfig {
    #[serde(flatten)]
    pub common: GenerationConfig,
    #[serde(
        rename = "enableEnhancedCivicAnswers",
        skip_serializing_if = "Option::is_none"
    )]
    pub enable_enhanced_civic_answers: Option<bool>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationConfig {
    #[serde(skip_serializing_if = "is_default")]
    pub thinking_config: ThinkingConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_logprobs: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_count: Option<CandidateCount>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub response_modalities: Vec<ResponseModality>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_config: Option<ImageConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speech_config: Option<SpeechConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_resolution: Option<MediaResolution>,
}

impl GenerationConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn include_thoughts(mut self, include: bool) -> Self {
        self.thinking_config.include_thoughts = Some(include);
        self
    }

    pub fn top_k(mut self, top_k: u32) -> Self {
        self.top_k = Some(top_k);
        self
    }

    pub fn presence_penalty(mut self, penalty: f64) -> Self {
        self.presence_penalty = Some(penalty);
        self
    }

    pub fn frequency_penalty(mut self, penalty: f64) -> Self {
        self.frequency_penalty = Some(penalty);
        self
    }

    pub fn response_logprobs(mut self, enable: bool) -> Self {
        self.response_logprobs = Some(enable);
        self
    }

    pub fn logprobs(mut self, top: u32) -> Self {
        self.logprobs = Some(top);
        self
    }

    pub fn candidate_count(mut self, count: CandidateCount) -> Self {
        self.candidate_count = Some(count);
        self
    }

    pub fn response_modalities(
        mut self,
        modalities: impl IntoIterator<Item = ResponseModality>,
    ) -> Self {
        self.response_modalities = modalities.into_iter().collect();
        self
    }

    pub fn image_config(mut self, config: ImageConfig) -> Self {
        self.image_config = Some(config);
        self
    }

    pub fn speech_config(mut self, config: SpeechConfig) -> Self {
        self.speech_config = Some(config);
        self
    }

    pub fn media_resolution(mut self, resolution: MediaResolution) -> Self {
        self.media_resolution = Some(resolution);
        self
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_thoughts: Option<bool>,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CandidateCount {
    One,
}

impl Serialize for CandidateCount {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::One => serializer.serialize_u32(1),
        }
    }
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ResponseModality {
    Text,
    Image,
    Audio,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum MediaResolution {
    #[serde(rename = "MEDIA_RESOLUTION_LOW")]
    Low,
    #[serde(rename = "MEDIA_RESOLUTION_MEDIUM")]
    Medium,
    #[serde(rename = "MEDIA_RESOLUTION_HIGH")]
    High,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_size: Option<ImageSize>,
}

impl ImageConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn aspect_ratio(mut self, ratio: impl Into<String>) -> Self {
        self.aspect_ratio = Some(ratio.into());
        self
    }

    pub fn image_size(mut self, size: ImageSize) -> Self {
        self.image_size = Some(size);
        self
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice_config: Option<VoiceConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multi_speaker_voice_config: Option<MultiSpeakerVoiceConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language_code: Option<String>,
}

impl SpeechConfig {
    pub fn voice(voice_name: impl Into<String>) -> Self {
        Self {
            voice_config: Some(VoiceConfig::prebuilt(voice_name)),
            multi_speaker_voice_config: None,
            language_code: None,
        }
    }

    pub fn speakers<S: Into<String>, V: Into<String>>(
        speakers: impl IntoIterator<Item = (S, V)>,
    ) -> Self {
        let speaker_voice_configs = speakers
            .into_iter()
            .map(|(speaker, voice)| SpeakerVoiceConfig {
                speaker: speaker.into(),
                voice_config: VoiceConfig::prebuilt(voice),
            })
            .collect();
        Self {
            voice_config: None,
            multi_speaker_voice_config: Some(MultiSpeakerVoiceConfig {
                speaker_voice_configs,
            }),
            language_code: None,
        }
    }

    pub fn language_code(mut self, code: impl Into<String>) -> Self {
        self.language_code = Some(code.into());
        self
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceConfig {
    pub prebuilt_voice_config: PrebuiltVoiceConfig,
}

impl VoiceConfig {
    pub fn prebuilt(voice_name: impl Into<String>) -> Self {
        Self {
            prebuilt_voice_config: PrebuiltVoiceConfig {
                voice_name: voice_name.into(),
            },
        }
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrebuiltVoiceConfig {
    pub voice_name: String,
}

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MultiSpeakerVoiceConfig {
    pub speaker_voice_configs: Vec<SpeakerVoiceConfig>,
}

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerVoiceConfig {
    pub speaker: String,
    pub voice_config: VoiceConfig,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HarmCategory {
    HateSpeech,
    DangerousContent,
    Harassment,
    SexuallyExplicit,
    CivicIntegrity,
    Jailbreak,
}

impl HarmCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HateSpeech => "HARM_CATEGORY_HATE_SPEECH",
            Self::DangerousContent => "HARM_CATEGORY_DANGEROUS_CONTENT",
            Self::Harassment => "HARM_CATEGORY_HARASSMENT",
            Self::SexuallyExplicit => "HARM_CATEGORY_SEXUALLY_EXPLICIT",
            Self::CivicIntegrity => "HARM_CATEGORY_CIVIC_INTEGRITY",
            Self::Jailbreak => "HARM_CATEGORY_JAILBREAK",
        }
    }
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HarmBlockThreshold {
    BlockLowAndAbove,
    BlockMediumAndAbove,
    BlockOnlyHigh,
    BlockNone,
    Off,
}

impl HarmBlockThreshold {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BlockLowAndAbove => "BLOCK_LOW_AND_ABOVE",
            Self::BlockMediumAndAbove => "BLOCK_MEDIUM_AND_ABOVE",
            Self::BlockOnlyHigh => "BLOCK_ONLY_HIGH",
            Self::BlockNone => "BLOCK_NONE",
            Self::Off => "OFF",
        }
    }
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SafetySetting {
    pub category: HarmCategory,
    pub threshold: HarmBlockThreshold,
}

impl SafetySetting {
    pub fn new(category: HarmCategory, threshold: HarmBlockThreshold) -> Self {
        Self {
            category,
            threshold,
        }
    }
}

impl Serialize for SafetySetting {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("category", self.category.as_str())?;
        map.serialize_entry("threshold", self.threshold.as_str())?;
        map.end()
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GeminiExtras {
    pub model_version: Option<String>,
    pub response_id: Option<String>,
    pub service_tier: Option<String>,
    pub prompt_tokens_details: Option<Vec<ModalityTokenCount>>,
    pub cache_tokens_details: Option<Vec<ModalityTokenCount>>,
    pub candidates_tokens_details: Option<Vec<ModalityTokenCount>>,
    pub tool_use_prompt_tokens_details: Option<Vec<ModalityTokenCount>>,
    pub prompt_feedback: Option<PromptFeedback>,
    pub safety_ratings: Option<Vec<SafetyRating>>,
    pub finish_message: Option<String>,
    pub citation_metadata: Option<CitationMetadata>,
    pub grounding_metadata: Option<GroundingMetadata>,
    pub url_context_metadata: Option<UrlContextMetadata>,
    pub avg_logprobs: Option<f64>,
    pub logprobs_result: Option<LogprobsResult>,
    pub id: Option<String>,
    pub status: Option<InteractionStatus>,
    pub created: Option<String>,
    pub updated: Option<String>,
    pub input_tokens_by_modality: Option<Vec<ModalityTokens>>,
    pub output_tokens_by_modality: Option<Vec<ModalityTokens>>,
    pub cached_tokens_by_modality: Option<Vec<ModalityTokens>>,
    pub grounding_tool_count: Option<Vec<GroundingToolCount>>,
}

impl ReplyExtras for GeminiExtras {
    fn from_reply(api: &Api, raw: &Value) -> Result<Self, serde_json::Error> {
        match api.as_str() {
            GENERATE_CONTENT => Ok(GenerateContentReply::deserialize(raw)?.into()),

            other => Err(serde_json::Error::custom(format!(
                "the Gemini API returns no `{other}` reply"
            ))),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GenerateContentReply {
    model_version: Option<String>,
    response_id: Option<String>,
    usage_metadata: Option<UsageDetails>,
    prompt_feedback: Option<PromptFeedback>,
    #[serde(default)]
    candidates: Vec<CandidateDetails>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageDetails {
    service_tier: Option<String>,
    prompt_tokens_details: Option<Vec<ModalityTokenCount>>,
    cache_tokens_details: Option<Vec<ModalityTokenCount>>,
    candidates_tokens_details: Option<Vec<ModalityTokenCount>>,
    tool_use_prompt_tokens_details: Option<Vec<ModalityTokenCount>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CandidateDetails {
    safety_ratings: Option<Vec<SafetyRating>>,
    finish_message: Option<String>,
    citation_metadata: Option<CitationMetadata>,
    grounding_metadata: Option<GroundingMetadata>,
    url_context_metadata: Option<UrlContextMetadata>,
    avg_logprobs: Option<f64>,
    logprobs_result: Option<LogprobsResult>,
}

impl From<GenerateContentReply> for GeminiExtras {
    fn from(reply: GenerateContentReply) -> Self {
        let mut extras = Self {
            model_version: reply.model_version,
            response_id: reply.response_id,
            prompt_feedback: reply.prompt_feedback,
            ..Self::default()
        };
        if let Some(usage) = reply.usage_metadata {
            extras.service_tier = usage.service_tier;
            extras.prompt_tokens_details = usage.prompt_tokens_details;
            extras.cache_tokens_details = usage.cache_tokens_details;
            extras.candidates_tokens_details = usage.candidates_tokens_details;
            extras.tool_use_prompt_tokens_details = usage.tool_use_prompt_tokens_details;
        }
        if let Some(candidate) = reply.candidates.into_iter().next() {
            extras.safety_ratings = candidate.safety_ratings;
            extras.finish_message = candidate.finish_message;
            extras.citation_metadata = candidate.citation_metadata;
            extras.grounding_metadata = candidate.grounding_metadata;
            extras.url_context_metadata = candidate.url_context_metadata;
            extras.avg_logprobs = candidate.avg_logprobs;
            extras.logprobs_result = candidate.logprobs_result;
        }
        extras
    }
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModalityTokenCount {
    pub modality: Option<String>,
    pub token_count: Option<u64>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptFeedback {
    pub block_reason: Option<String>,
    pub safety_ratings: Option<Vec<SafetyRating>>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SafetyRating {
    pub category: Option<String>,
    pub probability: Option<String>,
    pub blocked: Option<bool>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CitationMetadata {
    pub citation_sources: Option<Vec<CitationSource>>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CitationSource {
    pub start_index: Option<u64>,
    pub end_index: Option<u64>,
    pub uri: Option<String>,
    pub license: Option<String>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroundingMetadata {
    pub web_search_queries: Option<Vec<String>>,
    pub search_entry_point: Option<SearchEntryPoint>,
    pub grounding_chunks: Option<Vec<GroundingChunk>>,
    pub grounding_supports: Option<Vec<GroundingSupport>>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchEntryPoint {
    pub rendered_content: Option<String>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroundingChunk {
    pub web: Option<WebChunk>,
    pub retrieved_context: Option<RetrievedContext>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct WebChunk {
    pub uri: Option<String>,
    pub title: Option<String>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct RetrievedContext {
    pub uri: Option<String>,
    pub title: Option<String>,
    pub text: Option<String>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroundingSupport {
    pub segment: Option<Segment>,
    pub grounding_chunk_indices: Option<Vec<u32>>,
    pub confidence_scores: Option<Vec<f64>>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub part_index: Option<u32>,
    pub start_index: Option<u64>,
    pub end_index: Option<u64>,
    pub text: Option<String>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UrlContextMetadata {
    pub url_metadata: Option<Vec<UrlMetadata>>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UrlMetadata {
    pub retrieved_url: Option<String>,
    pub url_retrieval_status: Option<String>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogprobsResult {
    pub top_candidates: Option<Vec<TopCandidates>>,
    pub chosen_candidates: Option<Vec<LogprobsCandidate>>,
    pub log_probability_sum: Option<f64>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct TopCandidates {
    pub candidates: Option<Vec<LogprobsCandidate>>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogprobsCandidate {
    pub token: Option<String>,
    pub token_id: Option<i64>,
    pub log_probability: Option<f64>,
}

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InteractionStatus {
    InProgress,
    RequiresAction,
    Incomplete,
    BudgetExceeded,
    Completed,
    Failed,
    Cancelled,
    Queued,
    #[serde(untagged)]
    Unknown(String),
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct ModalityTokens {
    pub modality: Option<String>,
    pub tokens: Option<u64>,
}

#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct GroundingToolCount {
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub count: Option<u64>,
    pub search_query_count: Option<u64>,
}
