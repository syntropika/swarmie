pub(crate) mod schema;
pub mod wire;
pub mod wire_ids;

macro_rules! object {
    ($($key:literal: $value:expr),* $(,)?) => {
        serde_json::Value::Object(serde_json::Map::from_iter([
            $(($key.to_owned(), serde_json::Value::from($value))),*
        ]))
    };
}
pub(crate) use object;

pub(crate) fn set_fields<const N: usize>(
    fields: [(&str, Option<serde_json::Value>); N],
) -> serde_json::Map<String, serde_json::Value> {
    fields
        .into_iter()
        .filter_map(|(key, value)| Some((key.to_owned(), value?)))
        .collect()
}

pub(crate) fn refuse_all(
    fields: crate::codecs::completion::options::OptionFields<'_>,
    reason: &str,
) -> crate::codecs::completion::options::OptionMap {
    use crate::codecs::completion::options::{Mapping, OptionMap};
    let refuse = |set: bool| match set {
        true => Mapping::unsupported(reason),
        false => Mapping::Nothing,
    };
    OptionMap {
        reasoning: refuse(fields.reasoning.is_some()),
        cache: refuse(fields.cache.is_some()),
        service_tier: refuse(fields.service_tier.is_some()),
        verbosity: refuse(fields.verbosity.is_some()),
        parallel_tool_calls: refuse(fields.parallel_tool_calls.is_some()),
        top_p: refuse(fields.top_p.is_some()),
        seed: refuse(fields.seed.is_some()),
        stop: refuse(!fields.stop.is_empty()),
        cache_key: Mapping::unrouted(fields.cache_key),
    }
}

pub(crate) fn session_id(cache_key: Option<&str>) -> String {
    cache_key.map_or_else(crate::codecs::id::generate, str::to_owned)
}

pub(crate) fn request_session_id(request: &crate::codecs::completion::CompletionRequest) -> String {
    session_id(request.options.cache_key.as_deref())
}

#[derive(Clone, Copy)]
#[doc(hidden)]
pub enum LogTarget {
    Completions,
    Streaming,
}

#[doc(hidden)]
pub fn trace_json(_target: LogTarget, _label: &str, _value: &impl serde::Serialize) {
    // Raw protocol JSON is never recorded by diagnostic tracing.
    // Content recording requires the explicit telemetry request policy.
}

pub(crate) mod named_dialect {
    pub(crate) fn serialize<S: serde::Serializer>(
        serializer: S,
        family: &str,
        name: &str,
        registered: bool,
    ) -> Result<S::Ok, S::Error> {
        if !registered {
            return Err(serde::ser::Error::custom(format!(
                "an unregistered or modified {family} dialect cannot be persisted by name; \
                 use configuration overrides"
            )));
        }
        serializer.serialize_str(name)
    }

    pub(crate) fn deserialize<'de, D, T>(
        deserializer: D,
        family: &str,
        by_name: impl FnOnce(&str) -> Option<T>,
    ) -> Result<T, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let name = <String as serde::Deserialize>::deserialize(deserializer)?;
        by_name(&name).ok_or_else(|| {
            serde::de::Error::custom(format!("`{name}` is not a registered {family} dialect"))
        })
    }
}

pub(crate) fn with_query_pairs(path: &str, pairs: &[(&str, &str)]) -> String {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (name, value) in pairs {
        serializer.append_pair(name, value);
    }
    format!("{path}?{}", serializer.finish())
}
