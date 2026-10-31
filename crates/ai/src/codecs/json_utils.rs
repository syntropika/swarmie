use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use std::convert::Infallible;
use std::fmt;
use std::marker::PhantomData;
use std::str::FromStr;

#[allow(clippy::trivially_copy_pass_by_ref)]
pub(crate) fn is_false(value: &bool) -> bool {
    !value
}

pub fn canonical(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut entries: Vec<_> = map.into_iter().collect();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            let entries = entries
                .into_iter()
                .map(|(key, value)| (key, canonical(value)));
            serde_json::Value::Object(entries.collect())
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(canonical).collect())
        }
        other => other,
    }
}

pub fn to_canonical_string(value: &serde_json::Value) -> String {
    canonical(value.clone()).to_string()
}

pub fn remove_preserving_order(
    map: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<serde_json::Value> {
    let mut removed = None;
    map.retain(|candidate, value| {
        if candidate == key {
            removed = Some(value.take());
            false
        } else {
            true
        }
    });
    removed
}

pub fn merge(a: serde_json::Value, b: serde_json::Value) -> serde_json::Value {
    match (a, b) {
        (serde_json::Value::Object(mut a_map), serde_json::Value::Object(b_map)) => {
            b_map.into_iter().for_each(|(key, value)| {
                a_map.insert(key, value);
            });
            serde_json::Value::Object(a_map)
        }
        (a, _) => a,
    }
}

pub(crate) fn merge_params(
    existing: Option<serde_json::Value>,
    params: Option<serde_json::Value>,
) -> Option<serde_json::Value> {
    Some(match (existing, params?) {
        (Some(existing), params) => merge(existing, params),
        (None, params) => params,
    })
}

pub fn merge_inplace(a: &mut serde_json::Value, b: serde_json::Value) {
    if let (serde_json::Value::Object(a_map), serde_json::Value::Object(b_map)) = (a, b) {
        b_map.into_iter().for_each(|(key, value)| {
            a_map.insert(key, value);
        });
    }
}

pub fn value_to_json_string(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

pub fn parse_tool_arguments(arguments: &str) -> serde_json::Result<serde_json::Value> {
    if arguments.trim().is_empty() {
        return Ok(serde_json::Value::Object(serde_json::Map::new()));
    }

    serde_json::from_str(arguments)
}

pub fn is_whole_object(arguments: &str) -> bool {
    let end = arguments.trim_end();
    (end.is_empty() || end.ends_with('}'))
        && matches!(
            parse_tool_arguments(arguments),
            Ok(serde_json::Value::Object(_))
        )
}

pub fn parse_partial_object(text: &str) -> Option<serde_json::Map<String, serde_json::Value>> {
    // Bounds the work on a large cut-off input: the last valid cut is
    // almost always near its end.
    const MAX_ATTEMPTS: usize = 256;
    let mut stack = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    // Prefix lengths a value just ended at, with the containers open there.
    let mut cuts: Vec<(usize, Vec<u8>, bool)> = Vec::new();
    let bytes = text.as_bytes();
    for (at, byte) in bytes.iter().enumerate() {
        if in_string {
            match (escaped, *byte) {
                (true, _) => escaped = false,
                (false, b'\\') => escaped = true,
                (false, b'"') => {
                    in_string = false;
                    cuts.push((at + 1, stack.clone(), false));
                }
                (false, _) => {}
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                stack.push(*byte);
                cuts.push((at + 1, stack.clone(), false));
            }
            b'}' | b']' => {
                stack.pop();
                cuts.push((at + 1, stack.clone(), false));
            }
            b if b.is_ascii_alphanumeric() || *b == b'.' || *b == b'-' || *b == b'+' => {
                let next = bytes.get(at + 1).copied();
                if !next.is_some_and(|n| {
                    n.is_ascii_alphanumeric() || n == b'.' || n == b'-' || n == b'+'
                }) {
                    cuts.push((at + 1, stack.clone(), false));
                }
            }
            _ => {}
        }
    }
    if in_string {
        let at = text.len() - usize::from(escaped);
        cuts.push((at, stack.clone(), true));
    }
    cuts.iter()
        .rev()
        .take(MAX_ATTEMPTS)
        .find_map(|(at, open, quote)| {
            let mut candidate = text.get(..*at)?.to_owned();
            if *quote {
                candidate.push('"');
            }
            for container in open.iter().rev() {
                candidate.push(if *container == b'{' { '}' } else { ']' });
            }
            match serde_json::from_str(&candidate) {
                Ok(serde_json::Value::Object(object)) => Some(object),
                _ => None,
            }
        })
}

pub fn parse_partial_arguments(text: &str) -> serde_json::Map<String, serde_json::Value> {
    if text.trim().is_empty() {
        return serde_json::Map::new();
    }
    let repaired = repair_json(text);
    let complete = serde_json::from_str(text)
        .ok()
        .or_else(|| serde_json::from_str(&repaired).ok());
    if let Some(value) = complete {
        return match value {
            serde_json::Value::Object(object) => object,
            _ => serde_json::Map::new(),
        };
    }
    let cut = without_partial_escape(&repaired);
    parse_partial_object(cut)
        .or_else(|| parse_partial_object(text))
        .unwrap_or_default()
}

fn repair_json(text: &str) -> String {
    let mut repaired = String::with_capacity(text.len());
    let mut in_string = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if !in_string {
            in_string = c == '"';
            repaired.push(c);
            continue;
        }
        match c {
            '"' => {
                in_string = false;
                repaired.push(c);
            }
            '\\' => match chars.peek() {
                Some('"' | '\\' | '/' | 'b' | 'f' | 'n' | 'r' | 't' | 'u') => {
                    repaired.push(c);
                    if let Some(next) = chars.next() {
                        repaired.push(next);
                    }
                }
                // A trailing backslash may begin an escape the next fragment ends.
                None => repaired.push(c),
                Some(_) => repaired.push_str("\\\\"),
            },
            '\n' => repaired.push_str("\\n"),
            '\r' => repaired.push_str("\\r"),
            '\t' => repaired.push_str("\\t"),
            c if c.is_control() => {
                use std::fmt::Write as _;
                // Writing to a `String` cannot fail.
                let _ = write!(repaired, "\\u{:04x}", u32::from(c));
            }
            c => repaired.push(c),
        }
    }
    repaired
}

fn without_partial_escape(text: &str) -> &str {
    let bytes = text.as_bytes();
    let tail = bytes.len().saturating_sub(5);
    let Some(at) = (tail..bytes.len())
        .rev()
        .find(|at| bytes.get(*at..*at + 2) == Some(b"\\u"))
    else {
        return text;
    };
    let digits = bytes.get(at + 2..).unwrap_or_default();
    // A preceding backslash run of odd length escapes this backslash itself.
    let escaping = bytes
        .get(..at)
        .unwrap_or_default()
        .iter()
        .rev()
        .take_while(|byte| **byte == b'\\')
        .count();
    if digits.len() < 4 && digits.iter().all(u8::is_ascii_hexdigit) && escaping % 2 == 0 {
        text.get(..at).unwrap_or(text)
    } else {
        text
    }
}

pub fn string_or_vec<'de, T, D>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    T: Deserialize<'de> + FromStr<Err = Infallible>,
    D: Deserializer<'de>,
{
    struct StringOrVec<T>(PhantomData<fn() -> T>);

    impl<'de, T> Visitor<'de> for StringOrVec<T>
    where
        T: Deserialize<'de> + FromStr<Err = Infallible>,
    {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a string, sequence, or null")
        }

        fn visit_str<E>(self, value: &str) -> Result<Vec<T>, E>
        where
            E: de::Error,
        {
            let item = FromStr::from_str(value).map_err(de::Error::custom)?;
            Ok(vec![item])
        }

        fn visit_seq<A>(self, seq: A) -> Result<Vec<T>, A::Error>
        where
            A: SeqAccess<'de>,
        {
            Deserialize::deserialize(de::value::SeqAccessDeserializer::new(seq))
        }

        fn visit_map<M>(self, map: M) -> Result<Vec<T>, M::Error>
        where
            M: MapAccess<'de>,
        {
            let item = Deserialize::deserialize(de::value::MapAccessDeserializer::new(map))?;
            Ok(vec![item])
        }

        fn visit_none<E>(self) -> Result<Vec<T>, E>
        where
            E: de::Error,
        {
            Ok(vec![])
        }

        fn visit_unit<E>(self) -> Result<Vec<T>, E>
        where
            E: de::Error,
        {
            Ok(vec![])
        }
    }

    deserializer.deserialize_any(StringOrVec(PhantomData))
}

pub fn null_or_default<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: Deserialize<'de> + Default,
    D: Deserializer<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

pub trait Lenient {
    fn str(&self, key: &str) -> Option<&str>;
    fn u64(&self, key: &str) -> Option<u64>;
    fn i64(&self, key: &str) -> Option<i64>;
    fn f64(&self, key: &str) -> Option<f64>;
    fn bool(&self, key: &str) -> Option<bool>;
    fn obj(&self, key: &str) -> Option<&serde_json::Map<String, serde_json::Value>>;
    fn arr(&self, key: &str) -> &[serde_json::Value];
    fn at(&self, pointer: &str) -> Option<&serde_json::Value>;
    fn as_u64_lenient(&self) -> Option<u64>;
}

impl Lenient for serde_json::Value {
    fn str(&self, key: &str) -> Option<&str> {
        self.get(key)?.as_str()
    }

    fn u64(&self, key: &str) -> Option<u64> {
        self.get(key)?.as_u64_lenient()
    }

    fn i64(&self, key: &str) -> Option<i64> {
        match self.get(key)? {
            serde_json::Value::Number(number) => number.as_i64(),
            serde_json::Value::String(text) => text.trim().parse().ok(),
            _ => None,
        }
    }

    fn f64(&self, key: &str) -> Option<f64> {
        match self.get(key)? {
            serde_json::Value::Number(number) => number.as_f64(),
            serde_json::Value::String(text) => text.trim().parse().ok(),
            _ => None,
        }
    }

    fn bool(&self, key: &str) -> Option<bool> {
        self.get(key)?.as_bool()
    }

    fn obj(&self, key: &str) -> Option<&serde_json::Map<String, serde_json::Value>> {
        self.get(key)?.as_object()
    }

    fn arr(&self, key: &str) -> &[serde_json::Value] {
        self.get(key)
            .and_then(serde_json::Value::as_array)
            .map_or(&[], Vec::as_slice)
    }

    fn at(&self, pointer: &str) -> Option<&serde_json::Value> {
        self.pointer(pointer).filter(|value| !value.is_null())
    }

    fn as_u64_lenient(&self) -> Option<u64> {
        match self {
            serde_json::Value::Number(number) => number.as_u64().or_else(|| {
                number
                    .as_f64()
                    .filter(|float| *float >= 0.0 && float.fract() == 0.0)
                    .map(|float| float as u64)
            }),
            serde_json::Value::String(text) => text.trim().parse().ok(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_preserving_order_keeps_remaining_entries_in_original_order() {
        let mut map: serde_json::Map<String, serde_json::Value> = [
            ("z", serde_json::json!(1)),
            ("m", serde_json::json!({"nested": [2, 3]})),
            ("a", serde_json::Value::Null),
            ("y", serde_json::json!(4)),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect();
        let expected: Vec<_> = map
            .iter()
            .filter(|(key, _)| key.as_str() != "m")
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();

        assert_eq!(
            remove_preserving_order(&mut map, "m"),
            Some(serde_json::json!({"nested": [2, 3]}))
        );
        assert_eq!(map.into_iter().collect::<Vec<_>>(), expected);
    }

    #[test]
    fn remove_preserving_order_distinguishes_missing_and_null_values() {
        let mut map = serde_json::Map::new();
        map.insert("z".to_owned(), serde_json::json!(1));
        map.insert("a".to_owned(), serde_json::Value::Null);
        let before: Vec<_> = map.clone().into_iter().collect();

        assert_eq!(remove_preserving_order(&mut map, "missing"), None);
        assert_eq!(map.clone().into_iter().collect::<Vec<_>>(), before);
        assert_eq!(
            remove_preserving_order(&mut map, "a"),
            Some(serde_json::Value::Null)
        );
        assert_eq!(remove_preserving_order(&mut map, "a"), None);
    }

    #[test]
    fn test_merge_inplace() {
        let mut a = serde_json::json!({"key1": "value1"});
        let b = serde_json::json!({"key2": "value2"});
        merge_inplace(&mut a, b);
        let expected = serde_json::json!({"key1": "value1", "key2": "value2"});
        assert_eq!(a, expected);
    }

    #[test]
    fn text_that_states_no_object_has_none() {
        for text in ["not json", "[1, 2", "\"str", "12"] {
            assert_eq!(parse_partial_object(text), None, "{text}");
        }
    }

    mod string_or_vec_shapes {
        use serde::Deserialize;
        use std::convert::Infallible;
        use std::str::FromStr;

        #[derive(Debug, Deserialize, PartialEq)]
        struct Block {
            text: String,
        }

        impl FromStr for Block {
            type Err = Infallible;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Ok(Block {
                    text: value.to_owned(),
                })
            }
        }

        #[derive(Debug, Deserialize, PartialEq)]
        struct Holder {
            #[serde(deserialize_with = "super::super::string_or_vec")]
            content: Vec<Block>,
        }

        fn decode(json: serde_json::Value) -> Vec<Block> {
            serde_json::from_value::<Holder>(json)
                .expect("shape should decode")
                .content
        }

        fn block(text: &str) -> Block {
            Block {
                text: text.to_owned(),
            }
        }

        #[test]
        fn a_bare_object_is_one_element() {
            // `visit_map`. Carried over from the removed container's
            // `string_or_one_or_many`, which had this arm where the helper it
            // merged into did not.
            assert_eq!(
                decode(serde_json::json!({"content": {"text": "hi"}})),
                vec![block("hi")]
            );
        }

        #[test]
        fn a_bare_string_becomes_one_element_via_from_str() {
            assert_eq!(
                decode(serde_json::json!({"content": "hi"})),
                vec![block("hi")]
            );
        }

        #[test]
        fn an_empty_sequence_is_an_empty_list() {
            // The non-empty container this helper replaced rejected `[]`
            // outright. It is now a value.
            assert!(decode(serde_json::json!({"content": []})).is_empty());
        }

        #[test]
        fn null_is_an_empty_list() {
            // Load-bearing: OpenAI sends `"content": null` for a message that
            // carries only tool calls, so dropping this arm would turn a normal
            // response into a decode error.
            assert!(decode(serde_json::json!({"content": null})).is_empty());
        }
    }

    mod partial_arguments {
        use super::super::parse_partial_arguments;
        use serde_json::{Value, json};

        fn parsed(text: &str) -> Value {
            Value::Object(parse_partial_arguments(text))
        }

        #[test]
        fn cut_off_text_keeps_what_it_states() {
            for (text, expected) in [
                ("", json!({})),
                ("   ", json!({})),
                ("{", json!({})),
                (r#"{"path"#, json!({})),
                (r#"{"path":"#, json!({})),
                (r#"{"path": "no"#, json!({"path": "no"})),
                (r#"{"path": "notes.md","#, json!({"path": "notes.md"})),
                (r#"{"path": "notes.md", "con"#, json!({"path": "notes.md"})),
                (r#"{"a": {"b": [1, 2"#, json!({"a": {"b": [1, 2]}})),
                (r#"{"a": [{"b": "c"}, {"d"#, json!({"a": [{"b": "c"}, {}]})),
                (r#"{"a": 1, "b": 2,"#, json!({"a": 1, "b": 2})),
                (r#"{"line": "one\ntw"#, json!({"line": "one\ntw"})),
                (r#"{"quote": "say \"hi"#, json!({"quote": "say \"hi"})),
            ] {
                assert_eq!(parsed(text), expected, "{text}");
            }
        }

        #[test]
        fn escapes_cut_short_keep_the_string_before_them() {
            for (text, expected) in [
                (r#"{"a": "x\"#, json!({"a": "x"})),
                (r#"{"a": "x\u"#, json!({"a": "x"})),
                (r#"{"a": "x\u00"#, json!({"a": "x"})),
                (r#"{"a": "x\u00e"#, json!({"a": "x"})),
                (r#"{"a": "x\u00e9"#, json!({"a": "xé"})),
                (r#"{"a": "x\\u00"#, json!({"a": "x\\u00"})),
            ] {
                assert_eq!(parsed(text), expected, "{text}");
            }
        }

        #[test]
        fn numbers_and_literals_cut_short_are_dropped_until_valid() {
            for (text, expected) in [
                (r#"{"n": 1"#, json!({"n": 1})),
                (r#"{"n": 1."#, json!({})),
                (r#"{"n": -"#, json!({})),
                (r#"{"n": 1e"#, json!({})),
                (r#"{"n": 1.5"#, json!({"n": 1.5})),
                (r#"{"b": tru"#, json!({})),
                (r#"{"b": true"#, json!({"b": true})),
                (r#"{"v": nul"#, json!({})),
                (r#"{"a": 1, "v": nul"#, json!({"a": 1})),
            ] {
                assert_eq!(parsed(text), expected, "{text}");
            }
        }

        #[test]
        fn raw_control_characters_and_stray_backslashes_are_repaired() {
            assert_eq!(
                parsed("{\"text\": \"line one\nline two\ttabbed\"}"),
                json!({"text": "line one\nline two\ttabbed"})
            );
            assert_eq!(
                parsed(r#"{"path": "C:\Users\me"}"#),
                json!({"path": "C:\\Users\\me"})
            );
            assert_eq!(
                parsed("{\"text\": \"cut\nshort"),
                json!({"text": "cut\nshort"})
            );
        }

        #[test]
        fn a_complete_object_parses_as_serde_does() {
            let text = r#"{"path": "a.md", "content": "x\ny \"q\" \u00e9", "n": [1, 2.5, -3e2], "o": {"t": true, "f": null}}"#;
            assert_eq!(
                parsed(text),
                serde_json::from_str::<Value>(text).expect("valid json")
            );
        }

        #[test]
        fn a_top_level_that_is_not_an_object_is_empty() {
            for text in [
                "[1, 2]", "[1, 2", "\"str\"", "\"str", "12", "true", "null", "not json",
            ] {
                assert_eq!(parsed(text), json!({}), "{text}");
            }
        }

        #[test]
        fn every_prefix_parses_and_the_whole_text_parses_exactly() {
            for whole in [
                json!({"path": "notes/today.md", "content": "Line one.\nLine \"two\" \\ é ✓\n\tIndented."}),
                json!({"x": 1, "y": -2.5, "z": 3e10, "flags": [true, false, null], "nested": {"a": [{"b": "c"}]}}),
                json!({"query": "", "limit": 0, "filters": {}, "tags": []}),
            ] {
                for text in [
                    whole.to_string(),
                    serde_json::to_string_pretty(&whole).expect("json"),
                ] {
                    for (cut, _) in text.char_indices() {
                        // Never fails, and always an object.
                        let _ = parse_partial_arguments(&text[..cut]);
                    }
                    assert_eq!(parsed(&text), whole);
                }
            }
        }
    }
}
