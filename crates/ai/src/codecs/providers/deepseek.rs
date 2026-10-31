pub const DEEPSEEK_FLASH: &str = "deepseek-flash";
pub const DEEPSEEK_V4_FLASH: &str = "deepseek-v4-flash";
pub const DEEPSEEK_V4_PRO: &str = "deepseek-v4-pro";

pub mod extension;

use crate::codecs::json_utils::remove_preserving_order;
use serde_json::{Map, Value};

const STRICT_FORMATS: [&str; 5] = ["email", "hostname", "ipv4", "ipv6", "uuid"];

pub(crate) fn strict_schema(schema: &mut Value) -> Result<(), &'static str> {
    // Closed here rather than by the shared sanitizer, which strips the
    // keywords beside a `$ref` DeepSeek takes and joins a `oneOf` into an
    // `anyOf` beside it; before the definitions are copied, so the ones
    // written out below are closed too.
    close(schema)?;
    let definitions = schema.clone();
    strict_node(schema, &definitions, &mut Vec::new(), true)
}

fn close(node: &mut Value) -> Result<(), &'static str> {
    let Value::Object(object) = node else {
        return Ok(());
    };
    split_type_list(object)?;
    if object.get("type").and_then(Value::as_str) == Some("object")
        || object.contains_key("properties")
    {
        let properties = object
            .entry("properties")
            .or_insert_with(|| Value::Object(Map::new()));
        let required: Vec<Value> = properties
            .as_object()
            .map(|properties| properties.keys().cloned().map(Value::String).collect())
            .unwrap_or_default();
        object.insert("required".to_owned(), Value::Array(required));
        object
            .entry("additionalProperties")
            .or_insert(Value::Bool(false));
    }
    if let Some(one_of) = remove_preserving_order(object, "oneOf") {
        if object.contains_key("anyOf") {
            return Err("a `oneOf` beside an `anyOf`");
        }
        object.insert("anyOf".to_owned(), one_of);
    }
    for key in ["properties", "$defs", "$def", "definitions"] {
        if let Some(Value::Object(children)) = object.get_mut(key) {
            children.values_mut().try_for_each(close)?;
        }
    }
    for key in ["anyOf", "allOf"] {
        if let Some(Value::Array(members)) = object.get_mut(key) {
            members.iter_mut().try_for_each(close)?;
        }
    }
    match object.get_mut("items") {
        Some(items) => close(items),
        None => Ok(()),
    }
}

const TYPE_KEYWORDS: [(&str, &[&str]); 5] = [
    (
        "object",
        &[
            "properties",
            "required",
            "additionalProperties",
            "patternProperties",
            "propertyNames",
        ],
    ),
    (
        "array",
        &[
            "items",
            "prefixItems",
            "minItems",
            "maxItems",
            "uniqueItems",
            "contains",
        ],
    ),
    ("string", &["minLength", "maxLength", "pattern", "format"]),
    (
        "number",
        &[
            "minimum",
            "maximum",
            "exclusiveMinimum",
            "exclusiveMaximum",
            "multipleOf",
            "format",
        ],
    ),
    (
        "integer",
        &[
            "minimum",
            "maximum",
            "exclusiveMinimum",
            "exclusiveMaximum",
            "multipleOf",
            "format",
        ],
    ),
];

fn split_type_list(object: &mut Map<String, Value>) -> Result<(), &'static str> {
    let Some(Value::Array(types)) = object.get("type") else {
        return Ok(());
    };
    if !types
        .iter()
        .any(|kind| matches!(kind.as_str(), Some("object" | "array")))
    {
        return Ok(());
    }
    if object.contains_key("anyOf") {
        return Err("a type list naming an object or array beside an `anyOf`");
    }
    let Some(Value::Array(types)) = remove_preserving_order(object, "type") else {
        return Ok(());
    };
    let mut members: Vec<Value> = Vec::new();
    for kind in types {
        let mut member = Map::from_iter([("type".to_owned(), kind.clone())]);
        let keywords = TYPE_KEYWORDS
            .iter()
            .find(|(name, _)| kind.as_str() == Some(*name))
            .map_or(&[][..], |(_, keywords)| *keywords);
        for keyword in keywords {
            if let Some(value) = object.get(*keyword) {
                member.insert((*keyword).to_owned(), value.clone());
            }
        }
        members.push(Value::Object(member));
    }
    for (_, keywords) in TYPE_KEYWORDS {
        for keyword in keywords {
            remove_preserving_order(object, keyword);
        }
    }
    object.insert("anyOf".to_owned(), Value::Array(members));
    Ok(())
}

fn strict_node(
    node: &mut Value,
    root: &Value,
    inlining: &mut Vec<String>,
    top: bool,
) -> Result<(), &'static str> {
    let Value::Object(object) = node else {
        return match node {
            Value::Bool(_) => Err("an untyped schema"),
            _ => Ok(()),
        };
    };
    while let Some(all_of) = remove_preserving_order(object, "allOf") {
        let Value::Array(members) = all_of else {
            return Err("an `allOf` that is not a list");
        };
        let mut members = members.into_iter();
        match (members.next(), members.next()) {
            (Some(Value::Object(member)), None) => merge(object, member)?,
            _ => return Err("an `allOf` of several schemas"),
        }
    }
    // Live, strict mode refused `true`, `{}`, a bare `enum` and a bare
    // `const`: "one of `type`, `anyOf`, `$ref` field is required".
    if !["type", "anyOf", "$ref"]
        .iter()
        .any(|key| object.contains_key(*key))
    {
        return Err("an untyped schema");
    }
    // Live, an object with no properties was taken only as the whole tool's
    // parameters.
    if !top
        && object
            .get("properties")
            .and_then(Value::as_object)
            .is_some_and(Map::is_empty)
    {
        return Err("an object with no properties below the top");
    }
    if object.contains_key("prefixItems") || object.get("items").is_some_and(Value::is_array) {
        return Err("a tuple");
    }
    if object.contains_key("patternProperties")
        || object
            .get("additionalProperties")
            .is_some_and(|open| open != &Value::Bool(false))
    {
        return Err("an object open to further properties");
    }
    // Only a plain string's format is checked: DeepSeek took `date-time` and
    // `uri` on a `["string", "null"]` type list, and numeric formats, live.
    if object.get("type").and_then(Value::as_str) == Some("string")
        && object
            .get("format")
            .and_then(Value::as_str)
            .is_some_and(|format| !STRICT_FORMATS.contains(&format))
    {
        remove_preserving_order(object, "format");
    }
    if let Some(Value::Array(members)) = object.get_mut("anyOf") {
        for member in members {
            let depth = inlining.len();
            // Written out to the end: a merged `allOf` or a definition may
            // itself be a reference.
            while let Value::Object(fields) = member {
                if let Some(all_of) = remove_preserving_order(fields, "allOf") {
                    fields.insert("allOf".to_owned(), all_of);
                    strict_node(member, root, inlining, false)?;
                    continue;
                }
                let Some(Value::String(name)) = remove_preserving_order(fields, "$ref") else {
                    break;
                };
                if inlining.contains(&name) {
                    return Err("a type that holds itself through `anyOf`");
                }
                let Some(Value::Object(definition)) = definition(root, &name) else {
                    return Err("a `$ref` that names no definition");
                };
                merge(fields, definition.clone())?;
                inlining.push(name);
            }
            strict_node(member, root, inlining, false)?;
            inlining.truncate(depth);
        }
    }
    for key in ["properties", "$defs", "$def", "definitions"] {
        if let Some(Value::Object(children)) = object.get_mut(key) {
            for child in children.values_mut() {
                strict_node(child, root, inlining, false)?;
            }
        }
    }
    if let Some(items) = object.get_mut("items") {
        strict_node(items, root, inlining, false)?;
    }
    Ok(())
}

const ANNOTATIONS: [&str; 4] = ["description", "title", "default", "examples"];

fn merge(outer: &mut Map<String, Value>, inner: Map<String, Value>) -> Result<(), &'static str> {
    for (key, value) in inner {
        match outer.get(&key) {
            None => {
                outer.insert(key, value);
            }
            Some(existing) if *existing == value || ANNOTATIONS.contains(&key.as_str()) => {}
            Some(_) => return Err("a reference or `allOf` that restates a keyword differently"),
        }
    }
    Ok(())
}

fn definition<'a>(root: &'a Value, reference: &str) -> Option<&'a Value> {
    root.pointer(reference.strip_prefix('#')?)
}

pub const PROVIDER_NAME: &str = "deepseek";
