use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, Visitor};
use serde_json::Value;
use serde_json::value::RawValue;

use crate::codecs::wire::WireEvent;

pub fn classify_tagged_frame<T>(
    data: &str,
    tag: &str,
    is_known_event_type: impl Fn(&str) -> bool,
) -> WireEvent<T>
where
    T: serde::de::DeserializeOwned,
{
    match scan(data, &[tag], true) {
        Err(error) => WireEvent::Corrupt(error),
        // Non-object keep-alives must not become fatal typed-decode failures.
        Ok(None) => unknown(data, String::new()),
        Ok(Some(found)) => match text(found.first().copied().flatten()) {
            Some(event_type) if !is_known_event_type(&event_type) => unknown(data, event_type),
            _ => decode_known(data),
        },
    }
}

fn unknown<T>(data: &str, event_type: String) -> WireEvent<T> {
    match serde_json::from_str::<Value>(data) {
        Ok(value) => WireEvent::Unknown {
            event_type,
            value: value.into(),
        },
        Err(error) => WireEvent::Corrupt(error),
    }
}

pub fn classify_chat_completions_frame<T>(data: &str) -> WireEvent<T>
where
    T: serde::de::DeserializeOwned,
{
    let found = match scan(data, &["object", "choices"], true) {
        Ok(Some(found)) => found,
        // Non-object JSON is unrecognized rather than corrupt.
        Ok(None) => return unknown(data, String::new()),
        Err(error) => return WireEvent::Corrupt(error),
    };
    let object = text(found.first().copied().flatten());
    let has_choices = found.get(1).is_some_and(Option::is_some);
    if object.as_deref() == Some("chat.completion.chunk") || has_choices {
        decode_known(data)
    } else {
        unknown(data, object.unwrap_or_default())
    }
}

pub fn classify_marker_keyed_frame<T>(data: &str, marker_keys: &[&str]) -> WireEvent<T>
where
    T: serde::de::DeserializeOwned,
{
    match scan(data, marker_keys, false) {
        Err(error) => WireEvent::Corrupt(error),
        Ok(Some(found)) if found.iter().any(Option::is_some) => decode_known(data),
        Ok(_) => match serde_json::from_str::<Value>(data) {
            Ok(value) => WireEvent::Unknown {
                event_type: value
                    .as_object()
                    .map(|object| object.keys().cloned().collect::<Vec<_>>().join(","))
                    .unwrap_or_default(),
                value: value.into(),
            },
            Err(error) => WireEvent::Corrupt(error),
        },
    }
}

pub fn classify_untyped_line<T>(line: &[u8]) -> WireEvent<T>
where
    T: serde::de::DeserializeOwned,
{
    match serde_json::from_slice::<T>(line) {
        Ok(event) => WireEvent::Known(event),
        Err(error) => WireEvent::Corrupt(error),
    }
}

pub fn classify_or<T>(
    data: &str,
    first: impl Fn(&str) -> WireEvent<T>,
    then: impl Fn(&str) -> WireEvent<T>,
) -> WireEvent<T> {
    match first(data) {
        WireEvent::Corrupt(first_error) => match then(data) {
            WireEvent::Known(event) => WireEvent::Known(event),
            WireEvent::Corrupt(error) => WireEvent::Corrupt(error),
            WireEvent::Unknown { .. } => WireEvent::Corrupt(first_error),
        },
        event => event,
    }
}

#[derive(serde::Deserialize)]
struct MessageEnvelope {
    #[allow(dead_code)]
    message: String,
}

pub fn classify_reply_or_message_envelope<T>(
    data: &str,
    reply_marker: &str,
) -> WireEvent<Result<T, String>>
where
    T: serde::de::DeserializeOwned,
{
    classify_or(
        data,
        |data| classify_marker_keyed_frame::<T>(data, &[reply_marker, "message"]).map(Ok),
        |data| {
            classify_marker_keyed_frame::<MessageEnvelope>(data, &["message"])
                .map(|_| Err(data.to_owned()))
        },
    )
}

pub fn classify_or_untagged<T, E>(
    data: &str,
    tag: &str,
    is_known_event_type: impl Fn(&str) -> bool,
    tagged: impl Fn(T) -> E,
    untagged: impl Fn(&str) -> WireEvent<E>,
) -> WireEvent<E>
where
    T: serde::de::DeserializeOwned,
{
    match scan(data, &[tag], true) {
        Err(error) => WireEvent::Corrupt(error),
        Ok(None) => unknown(data, String::new()),
        Ok(Some(found)) => match found.first().copied().flatten() {
            None => match untagged(data) {
                WireEvent::Unknown { .. } => decode_known(data).map(tagged),
                event => event,
            },
            Some(value) => match text(Some(value)) {
                Some(event_type) if !is_known_event_type(&event_type) => unknown(data, event_type),
                _ => decode_known(data).map(tagged),
            },
        },
    }
}

type Found<'a> = Vec<Option<&'a RawValue>>;

fn text(value: Option<&RawValue>) -> Option<String> {
    serde_json::from_str(value?.get()).ok()
}

fn scan<'a>(
    data: &'a str,
    keys: &[&str],
    unique: bool,
) -> Result<Option<Found<'a>>, serde_json::Error> {
    let mut json = serde_json::Deserializer::from_str(data);
    let scanned = if data
        .trim_start_matches([' ', '\t', '\n', '\r'])
        .starts_with('{')
    {
        Some(serde::Deserializer::deserialize_map(&mut json, Scan(keys))?)
    } else {
        <IgnoredAny as serde::Deserialize>::deserialize(&mut json)?;
        None
    };
    json.end()?;
    match scanned {
        Some((_, Some(key))) if unique => Err(serde::de::Error::custom(format!(
            "duplicate `{key}` discriminator key in stream frame"
        ))),
        scanned => Ok(scanned.map(|(found, _)| found)),
    }
}

struct Scan<'k>(&'k [&'k str]);

impl<'de, 'k> Visitor<'de> for Scan<'k> {
    type Value = (Found<'de>, Option<&'k str>);

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut found = vec![None; self.0.len()];
        let mut duplicate = None;
        while let Some(index) = map.next_key_seed(Key(self.0))? {
            match index.and_then(|index| found.get_mut(index).zip(self.0.get(index))) {
                Some((slot @ None, _)) => *slot = Some(map.next_value()?),
                Some((Some(_), key)) => {
                    duplicate = duplicate.or(Some(*key));
                    map.next_value::<IgnoredAny>()?;
                }
                None => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok((found, duplicate))
    }
}

struct Key<'k>(&'k [&'k str]);

impl<'de> DeserializeSeed<'de> for Key<'_> {
    type Value = Option<usize>;

    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl<'de> Visitor<'de> for Key<'_> {
    type Value = Option<usize>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("an object key")
    }

    fn visit_str<E>(self, key: &str) -> Result<Self::Value, E> {
        Ok(self.0.iter().position(|candidate| *candidate == key))
    }
}

fn decode_known<T>(data: &str) -> WireEvent<T>
where
    T: serde::de::DeserializeOwned,
{
    match serde_json::from_str::<T>(data) {
        Ok(event) => WireEvent::Known(event),
        Err(error) => WireEvent::Corrupt(error),
    }
}
