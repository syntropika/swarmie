const FRAGMENT_KEYS: [&str; 13] = [
    "content",
    "input",
    "refusal",
    "reasoning",
    "reasoning_content",
    "reasoning_text",
    "thinking",
    "tool_plan",
    "transcript",
    "data",
    "arguments",
    "text",
    "summary",
];

pub(crate) fn merge_fields(
    target: &mut serde_json::Map<String, serde_json::Value>,
    delta: &serde_json::Map<String, serde_json::Value>,
) {
    use serde_json::Value;
    for (key, value) in delta {
        let fragment = FRAGMENT_KEYS.contains(&key.as_str());
        match (target.get_mut(key), value) {
            (Some(_), Value::Null) => {}
            (Some(existing), more)
                if matches!(key.as_str(), "content" | "thinking")
                    && (existing.is_array() || more.is_array()) =>
            {
                merge_content(existing, more);
            }
            (Some(Value::String(existing)), Value::String(more)) if fragment => {
                if existing.trim() == "null" && !more.trim().is_empty() {
                    existing.clear();
                }
                existing.push_str(more);
            }
            (Some(Value::String(existing)), Value::String(more))
                if more.is_empty() && !existing.is_empty() => {}
            (Some(Value::Array(existing)), Value::Array(more)) => {
                existing.extend(more.iter().cloned());
            }
            (Some(Value::Object(existing)), Value::Object(more)) => merge_fields(existing, more),
            (Some(existing), more)
                if !existing.is_null()
                    && std::mem::discriminant(existing) != std::mem::discriminant(more) => {}
            _ => {
                target.insert(key.clone(), value.clone());
            }
        }
    }
}

fn merge_content(existing: &mut serde_json::Value, more: &serde_json::Value) {
    use serde_json::Value;
    fn parts(value: Value) -> Vec<Value> {
        match value {
            Value::Array(parts) => parts,
            Value::String(text) if !text.is_empty() => {
                vec![serde_json::json!({"type": "text", "text": text})]
            }
            _ => Vec::new(),
        }
    }
    let mut merged = parts(std::mem::take(existing));
    for part in parts(more.clone()) {
        let kind = part.get("type").and_then(Value::as_str).map(str::to_owned);
        let kind = kind.as_deref();
        match (merged.last_mut(), part) {
            (Some(Value::Object(last)), Value::Object(next))
                if matches!(kind, Some("text" | "thinking"))
                    && last.get("type").and_then(Value::as_str) == kind =>
            {
                merge_fields(last, &next);
            }
            (_, part) => merged.push(part),
        }
    }
    *existing = Value::Array(merged);
}
