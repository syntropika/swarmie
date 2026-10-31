use std::borrow::Cow;

use serde::de::DeserializeOwned;
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use serde_json::{Map, Value};

use super::mapping::{Mapping, OptionName};
use super::{CacheRetention, OnUnsupported, UnsupportedOption};
use crate::codecs::completion::provider_options::SHARED;
use crate::codecs::completion::{CompletionRequest, ReplayTarget};
use crate::codecs::error::EncodeError;
use crate::codecs::json_utils::remove_preserving_order;
use crate::codecs::providers::openai::wire::BodyRewrite;
use crate::codecs::wire::Body;

#[derive(Clone, Debug, PartialEq)]
pub struct FinalBody {
    fields: Map<String, Value>,
    last: Vec<&'static str>,
}

impl Serialize for FinalBody {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.fields.len()))?;
        for (key, value) in &self.fields {
            if !self.last.contains(&key.as_str()) {
                map.serialize_entry(key, value)?;
            }
        }
        for key in &self.last {
            if let Some(value) = self.fields.get(*key) {
                map.serialize_entry(key, value)?;
            }
        }
        map.end()
    }
}

impl FinalBody {
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.fields.get(key)
    }

    pub fn pointer(&self, pointer: &str) -> Option<&Value> {
        let path = pointer.strip_prefix('/')?;
        let (head, rest) = path.split_once('/').unwrap_or((path, ""));
        let value = self
            .fields
            .get(&head.replace("~1", "/").replace("~0", "~"))?;
        if rest.is_empty() {
            Some(value)
        } else {
            value.pointer(&format!("/{rest}"))
        }
    }

    pub fn into_body(self) -> Body {
        let mut bytes = Vec::new();
        // JSON values and string keys serialize infallibly into a byte vector.
        let _ = serde_json::to_writer(&mut bytes, &self);
        Body::Bytes(bytes)
    }

    pub fn deserialize<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        // Read in place: a `Value` of the body would copy the whole history.
        let fields = self.fields.iter().map(|(key, value)| (key.as_str(), value));
        T::deserialize(serde::de::value::MapDeserializer::new(fields))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RawAt {
    Top,
    Under(&'static str),
    Split {
        top: &'static [&'static str],
        rest: &'static str,
    },
    #[doc(hidden)]
    Ignored(&'static str),
}

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Rewrite {
    #[doc(hidden)]
    OutputCapRename,
    #[doc(hidden)]
    DropUnboundThinking,
    #[doc(hidden)]
    AnthropicVersionInBody(String),
    #[doc(hidden)]
    ToolChoiceNeedsTools,
    Stream(bool),
    NoStream,
    #[doc(hidden)]
    NoBackground,
    StreamUsage,
    #[doc(hidden)]
    ReasoningCiphertext(bool),
    #[doc(hidden)]
    CodexStore,
    #[doc(hidden)]
    ChatDialect(BodyRewrite),
    #[doc(hidden)]
    GeminiCachedContent(Option<String>),
    #[doc(hidden)]
    MoveLast(&'static str),
}

pub struct BaseInput<'a> {
    target: &'a dyn ReplayTarget,
    request: &'a CompletionRequest,
    cache: Option<CacheRetention>,
    upper: &'a Map<String, Value>,
    provider: &'a Map<String, Value>,
    claimed: Vec<&'static str>,
    raw_tools: Option<Value>,
}

impl BaseInput<'_> {
    pub fn cache(&self) -> Option<CacheRetention> {
        self.cache
    }

    pub fn refuse_cache(&mut self, reason: impl Into<String>) -> Result<(), EncodeError> {
        refuse(self.target, self.request, "cache", reason.into())
    }

    pub fn param(&self, key: &str) -> Option<&Value> {
        self.upper.get(key)
    }

    pub fn claim(&mut self, key: &'static str) -> Option<Value> {
        self.claimed.push(key);
        self.provider.get(key).cloned()
    }

    pub fn raw_tools(&mut self) -> Result<Vec<Value>, EncodeError> {
        match self.raw_tools.take() {
            None | Some(Value::Null) => Ok(Vec::new()),
            Some(Value::Array(tools)) => Ok(tools),
            Some(other) => Err(EncodeError::request(format!(
                "`additional_params.tools` must be an array, got {other}"
            ))),
        }
    }
}

#[derive(Default)]
struct Settled {
    sends: Vec<Value>,
    cache: Option<CacheRetention>,
    ignored: Vec<OptionName>,
}

fn model_of<'a>(target: &'a dyn ReplayTarget, request: &'a CompletionRequest) -> &'a str {
    request
        .model
        .as_deref()
        .filter(|model| !model.is_empty())
        .unwrap_or_else(|| target.model())
}

fn refuse(
    target: &dyn ReplayTarget,
    request: &CompletionRequest,
    option: impl Into<Cow<'static, str>>,
    reason: String,
) -> Result<(), EncodeError> {
    let option = option.into();
    let provider = target.provider();
    let model = model_of(target, request);
    if collected(|| UnsupportedOption::new(option.clone(), provider, model, reason.clone())) {
        return Ok(());
    }
    match request.options.unsupported_policy() {
        OnUnsupported::Error => Err(EncodeError::unsupported(UnsupportedOption::new(
            option, provider, model, reason,
        ))),
        OnUnsupported::Ignore => {
            tracing::warn!(
                option = option.as_ref(),
                provider,
                model,
                reason = reason.as_str(),
                "option skipped: the provider cannot honour it"
            );
            Ok(())
        }
    }
}

fn settle(target: &dyn ReplayTarget, request: &CompletionRequest) -> Result<Settled, EncodeError> {
    let fields = request.options.fields();
    let map = target.map_options(request, fields);
    let provider = target.provider();
    let mut settled = Settled::default();
    for (option, mapping) in map.into_slots() {
        let set = fields.is_set(option);
        let name = option.as_str();
        let fail = |what: String| Err(EncodeError::request(format!("{provider} {what}")));
        match (mapping, set) {
            (Mapping::Nothing, false) => {}
            (Mapping::Nothing, true) => {
                return fail(format!(
                    "answered `Mapping::Nothing` for the option `{name}`, which the request sets"
                ));
            }
            (_, false) => {
                return fail(format!(
                    "answered the option `{name}`, which the request does not set; \
                     an unset option answers `Mapping::Nothing` (see `Mapping::of`)"
                ));
            }
            (Mapping::Send(value), true) => {
                if !value.is_object() {
                    return fail(format!(
                        "sent a value that is not a JSON object for the option `{name}`"
                    ));
                }
                settled.sends.push(value);
                if option == OptionName::Cache {
                    settled.cache = request.options.cache;
                }
            }
            (Mapping::Omit(reason), true) => {
                tracing::debug!(
                    option = name,
                    provider,
                    reason,
                    "option honoured by sending nothing"
                );
            }
            (Mapping::Place, true) if option == OptionName::Cache => {
                settled.cache = request.options.cache
            }
            (Mapping::Place, true) => {
                return fail(format!(
                    "answered `Mapping::Place` for the option `{name}`; only `cache` places markers"
                ));
            }
            (Mapping::Unsupported(reason), true) => {
                refuse(target, request, name, reason)?;
                settled.ignored.push(option);
            }
        }
    }
    Ok(settled)
}

pub fn check(
    target: &dyn ReplayTarget,
    request: &mut CompletionRequest,
) -> Result<(), EncodeError> {
    unserialized(request)?;
    let settled = settle(target, request)?;
    for option in settled.ignored {
        option.clear(request);
    }
    model_rules(target, request)?;
    let layer = provider_layer(target, request);
    for refusal in &layer.refused {
        refuse(
            target,
            request,
            refusal.name(target),
            refusal.reason.clone(),
        )?;
    }
    let api = target.api();
    for refusal in layer.refused {
        request.provider_options.remove_field(
            target.provider(),
            &[SHARED, api.as_str()],
            refusal.field,
        );
    }
    Ok(())
}

fn model_rules(
    target: &dyn ReplayTarget,
    request: &mut CompletionRequest,
) -> Result<(), EncodeError> {
    let facts = target
        .facts()
        .unwrap_or_else(|| crate::codecs::catalog::ModelFacts::builtin());
    let Some(spec) = facts.for_model(target.provider(), model_of(target, request)) else {
        return Ok(());
    };
    for CatalogRefusal { option, reason } in spec.request_rules(request) {
        refuse(target, request, option.as_str(), reason)?;
        option.clear(request);
    }
    Ok(())
}

thread_local! {
    static COLLECTED: std::cell::RefCell<Option<Vec<UnsupportedOption>>> =
        const { std::cell::RefCell::new(None) };
}

fn collected(make: impl FnOnce() -> UnsupportedOption) -> bool {
    COLLECTED.with(|collected| match collected.borrow_mut().as_mut() {
        Some(refusals) => {
            refusals.push(make());
            true
        }
        None => false,
    })
}

pub(crate) struct CatalogRefusal {
    pub(crate) option: OptionName,
    pub(crate) reason: String,
}

pub(crate) struct BodyRefusal {
    pub(crate) field: &'static str,
    pub(crate) reason: String,
}

pub(crate) fn catalog_refusals(
    target: &dyn ReplayTarget,
    request: &CompletionRequest,
    body: FinalBody,
    refusals: Vec<BodyRefusal>,
) -> Result<FinalBody, EncodeError> {
    let provider = target.provider();
    let model = model_of(target, request);
    if request.options.is_default() {
        for refusal in &refusals {
            tracing::debug!(
                option = refusal.field,
                provider,
                model,
                reason = refusal.reason.as_str(),
                "catalog refusal not applied: the request sets no generation options"
            );
        }
        return Ok(body);
    }
    let mut body = body;
    for BodyRefusal { field, reason } in refusals {
        let collecting =
            collected(|| UnsupportedOption::new(field, provider, model, reason.clone()));
        if !collecting && request.options.unsupported_policy() == OnUnsupported::Error {
            return Err(EncodeError::unsupported(UnsupportedOption::new(
                field, provider, model, reason,
            )));
        }
        let raw = request
            .additional_params
            .as_ref()
            .and_then(|params| params.get(field));
        let typed = field != "tools" && raw.is_none_or(|raw| body.get(field) != Some(raw));
        if typed {
            remove_preserving_order(&mut body.fields, field);
        }
        if collecting {
            continue;
        }
        tracing::warn!(
            option = field,
            provider,
            model,
            reason = reason.as_str(),
            "{}",
            match typed {
                true => "option skipped: the model's catalog entry refuses it",
                false => "catalog refusal ignored: sent as written",
            }
        );
    }
    Ok(body)
}

struct Refused {
    field: &'static str,
    section: String,
    reason: String,
}

impl Refused {
    fn name(&self, target: &dyn ReplayTarget) -> String {
        format!("{}.{}.{}", target.provider(), self.section, self.field)
    }
}

struct ProviderLayer {
    fields: Map<String, Value>,
    refused: Vec<Refused>,
}

fn provider_layer(target: &dyn ReplayTarget, request: &CompletionRequest) -> ProviderLayer {
    let provider = target.provider();
    let api = target.api();
    let mut layer = ProviderLayer {
        fields: Map::new(),
        refused: Vec::new(),
    };
    let Some(sections) = request.provider_options.sections(provider) else {
        return layer;
    };
    let mut route = None;
    for (name, section) in sections {
        let Value::Object(section) = section else {
            continue;
        };
        if name == SHARED {
            deep_merge(&mut layer.fields, section.clone());
        } else if name == api.as_str() {
            route = Some(section);
        } else {
            tracing::debug!(
                provider,
                section = name.as_str(),
                api = api.as_str(),
                "provider options section skipped: the request takes another route"
            );
        }
    }
    if let Some(route) = route {
        deep_merge(&mut layer.fields, route.clone());
    }
    for (field, reason) in request.provider_options.refusals(provider, target, request) {
        layer.refuse(target, request, field, reason);
    }
    layer
}

impl ProviderLayer {
    fn refuse(
        &mut self,
        target: &dyn ReplayTarget,
        request: &CompletionRequest,
        field: &'static str,
        reason: String,
    ) {
        if remove_preserving_order(&mut self.fields, field).is_none() {
            return;
        }
        let api = target.api();
        let in_route = request
            .provider_options
            .sections(target.provider())
            .and_then(|sections| sections.get(api.as_str()))
            .and_then(Value::as_object)
            .is_some_and(|route| route.contains_key(field));
        let section = if in_route { api.as_str() } else { SHARED };
        self.refused.push(Refused {
            field,
            section: section.to_owned(),
            reason,
        });
    }
}

fn unserialized(request: &CompletionRequest) -> Result<(), EncodeError> {
    match request.provider_options.failure() {
        Some(error) => Err(EncodeError::request(error.clone())),
        None => Ok(()),
    }
}

fn raw_layer(request: &CompletionRequest) -> Result<Map<String, Value>, EncodeError> {
    match &request.additional_params {
        None | Some(Value::Null) => Ok(Map::new()),
        Some(Value::Object(params)) => Ok(params.clone()),
        Some(_) => Err(EncodeError::request(
            "`additional_params` must be a JSON object",
        )),
    }
}

fn placed(raw: Map<String, Value>, raw_at: RawAt) -> Map<String, Value> {
    match raw_at {
        RawAt::Top => raw,
        RawAt::Under(_) if raw.is_empty() => Map::new(),
        RawAt::Under(pointer) => {
            let mut value = Value::Object(raw);
            for key in pointer.rsplit('/').filter(|key| !key.is_empty()) {
                value = Value::Object(Map::from_iter([(key.to_owned(), value)]));
            }
            match value {
                Value::Object(map) => map,
                _ => Map::new(),
            }
        }
        RawAt::Split { top, rest } => {
            let mut body = Map::new();
            let mut under = Map::new();
            for (key, value) in raw {
                if key == rest || top.contains(&key.as_str()) {
                    deep_merge(&mut body, Map::from_iter([(key, value)]));
                } else {
                    under.insert(key, value);
                }
            }
            if !under.is_empty() {
                deep_merge(
                    &mut body,
                    Map::from_iter([(rest.to_owned(), Value::Object(under))]),
                );
            }
            body
        }
        RawAt::Ignored(warning) => {
            if !raw.is_empty() {
                tracing::warn!("{warning}");
            }
            Map::new()
        }
    }
}

pub(crate) fn deep_merge(body: &mut Map<String, Value>, upper: Map<String, Value>) {
    for (key, value) in upper {
        match (body.get_mut(&key), value) {
            (Some(Value::Object(lower)), Value::Object(upper)) => deep_merge(lower, upper),
            (_, value) => {
                body.insert(key, value);
            }
        }
    }
}

fn upper_layers(
    sends: &[Value],
    provider: &Map<String, Value>,
    raw: &Map<String, Value>,
) -> Map<String, Value> {
    let mut upper = Map::new();
    for send in sends {
        if let Value::Object(send) = send {
            deep_merge(&mut upper, send.clone());
        }
    }
    deep_merge(&mut upper, provider.clone());
    deep_merge(&mut upper, raw.clone());
    upper
}

pub fn param(target: &dyn ReplayTarget, request: &CompletionRequest, key: &str) -> Option<Value> {
    let map = target.map_options(request, request.options.fields());
    let mut upper = Map::new();
    for (_, mapping) in map.into_slots() {
        if let Mapping::Send(Value::Object(send)) = mapping {
            deep_merge(&mut upper, send);
        }
    }
    deep_merge(&mut upper, provider_layer(target, request).fields);
    if let Some(Value::Object(raw)) = &request.additional_params {
        deep_merge(&mut upper, raw.clone());
    }
    remove_preserving_order(&mut upper, key)
}

pub fn request_params(
    target: &dyn ReplayTarget,
    request: &CompletionRequest,
    base: impl FnOnce(&mut BaseInput<'_>) -> Result<Map<String, Value>, EncodeError>,
    raw_at: RawAt,
    rewrites: &[Rewrite],
) -> Result<FinalBody, EncodeError> {
    unserialized(request)?;
    let settled = settle(target, request)?;
    let mut provider = provider_layer(target, request);
    // The rewrite drops a raw `background` as it always has; a typed one is
    // refused rather than dropped unseen.
    if rewrites.contains(&Rewrite::NoBackground) {
        provider.refuse(
            target,
            request,
            "background",
            "a WebSocket session takes no `background`".to_owned(),
        );
    }
    for refusal in provider.refused {
        refuse(target, request, refusal.name(target), refusal.reason)?;
    }
    let mut provider = provider.fields;
    let mut raw = raw_layer(request)?;
    let raw_tools = match raw_at {
        RawAt::Top | RawAt::Split { .. } => remove_preserving_order(&mut raw, "tools"),
        RawAt::Under(_) | RawAt::Ignored(_) => None,
    };
    let mut handles = Vec::new();
    if rewrites
        .iter()
        .any(|rewrite| matches!(rewrite, Rewrite::GeminiCachedContent(_)))
    {
        for spelling in ["cachedContent", "cached_content"] {
            match remove_preserving_order(&mut raw, spelling) {
                None => {}
                Some(Value::String(name)) => handles.push(name),
                Some(other) => {
                    return Err(EncodeError::request(format!(
                        "Gemini `additional_params.{spelling}` should be a string, got {other}"
                    )));
                }
            }
        }
        // GenerateContent has always read a raw `generationConfig: null` as
        // absent, so the typed and mapped fields under it are still sent.
        if raw.get("generationConfig").is_some_and(Value::is_null) {
            remove_preserving_order(&mut raw, "generationConfig");
        }
    }
    let raw = placed(raw, raw_at);
    let upper = upper_layers(&settled.sends, &provider, &raw);
    let mut input = BaseInput {
        target,
        request,
        cache: settled.cache,
        upper: &upper,
        provider: &provider,
        claimed: Vec::new(),
        raw_tools,
    };
    let mut body = base(&mut input)?;
    for key in input.claimed {
        remove_preserving_order(&mut provider, key);
    }
    for send in settled.sends {
        if let Value::Object(send) = send {
            deep_merge(&mut body, send);
        }
    }
    deep_merge(&mut body, provider);
    deep_merge(&mut body, raw);
    let mut body = FinalBody {
        fields: body,
        last: Vec::new(),
    };
    for rewrite in rewrites {
        apply(target, rewrite, &mut body, &handles)?;
    }
    Ok(body)
}

fn apply(
    target: &dyn ReplayTarget,
    rewrite: &Rewrite,
    body: &mut FinalBody,
    handles: &[String],
) -> Result<(), EncodeError> {
    let FinalBody { fields: body, last } = body;
    match rewrite {
        Rewrite::OutputCapRename => {
            // Read on the full id: a `vendor/model` id (a LiteLLM-style
            // proxy behind `OPENAI_BASE_URL`) keeps `max_tokens`.
            let reasoning = body
                .get("model")
                .and_then(Value::as_str)
                .is_some_and(|model| {
                    let facts = target
                        .facts()
                        .unwrap_or_else(|| crate::codecs::catalog::ModelFacts::builtin());
                    !model.contains('/')
                        && crate::codecs::providers::openai::options::reasons(facts, model)
                            == Some(true)
                });
            if reasoning && let Some(max_tokens) = remove_preserving_order(body, "max_tokens") {
                body.entry("max_completion_tokens").or_insert(max_tokens);
            }
        }
        Rewrite::DropUnboundThinking => {
            let adaptive = body.get("thinking").is_none_or(|thinking| {
                thinking.get("type").and_then(Value::as_str) == Some("adaptive")
            });
            if adaptive {
                crate::codecs::providers::anthropic::completion::drop_unbound_thinking(body);
            }
        }
        Rewrite::AnthropicVersionInBody(version) => {
            remove_preserving_order(body, "model");
            body.insert("anthropic_version".to_owned(), version.as_str().into());
        }
        Rewrite::ToolChoiceNeedsTools => {
            let has_tools = body
                .get("tools")
                .and_then(Value::as_array)
                .is_some_and(|tools| !tools.is_empty());
            if has_tools {
                body.entry("tool_choice")
                    .or_insert_with(|| serde_json::json!({ "type": "auto" }));
            } else {
                remove_preserving_order(body, "tool_choice");
            }
        }
        Rewrite::Stream(stream) => {
            body.insert("stream".to_owned(), Value::Bool(*stream));
        }
        Rewrite::NoStream => {
            remove_preserving_order(body, "stream");
        }
        Rewrite::NoBackground => {
            remove_preserving_order(body, "background");
        }
        Rewrite::StreamUsage => {
            if let Some(options) = body
                .entry("stream_options")
                .or_insert_with(|| Value::Object(Map::new()))
                .as_object_mut()
            {
                options.entry("include_usage").or_insert(Value::Bool(true));
            }
        }
        Rewrite::ReasoningCiphertext(always) => {
            let wanted = *always
                || body.get("reasoning").is_some()
                || body.get("store") == Some(&Value::Bool(false));
            if wanted {
                crate::codecs::providers::openai::responses_api::include_ciphertext(body);
            }
        }
        Rewrite::CodexStore => {
            body.insert("store".to_owned(), Value::Bool(false));
        }
        Rewrite::ChatDialect(kind) => {
            crate::codecs::providers::openai::wire::chat::rewrite_body(*kind, body)?;
        }
        Rewrite::GeminiCachedContent(handle) => {
            for name in handles.iter().chain(handle) {
                crate::codecs::providers::gemini::completion::with_cached_content(body, name)?;
            }
        }
        Rewrite::MoveLast(key) => {
            if body.contains_key(*key) {
                last.retain(|previous| previous != key);
                last.push(*key);
            }
        }
    }
    Ok(())
}
