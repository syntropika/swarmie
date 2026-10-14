use serde_json::{Value, json};
use swarmie::{Error, Result};

pub fn validate(config: &Value, descriptors: &Value) -> Result<()> {
    let fields = config
        .as_object()
        .ok_or_else(|| Error::Invalid("configuration must be an object".into()))?;
    for (id, value) in fields {
        let descriptor = descriptors
            .as_array()
            .and_then(|a| a.iter().find(|d| d["id"] == *id))
            .ok_or_else(|| Error::Invalid(format!("unknown configuration {id}")))?;
        let valid = match descriptor["type"].as_str() {
            Some("boolean") => value.is_boolean(),
            Some("select") => descriptor["choices"]
                .as_array()
                .is_some_and(|a| a.iter().any(|c| c["id"] == *value)),
            _ => false,
        };
        if !valid {
            return Err(Error::Invalid(format!("invalid choice for {id}")));
        }
    }
    Ok(())
}

pub fn select(id: &str, label: &str, value: Value, choices: Value) -> Value {
    json!({"id":id,"label":label,"type":"select","value":value,"choices":choices,"application":"next_turn"})
}
pub fn choices(values: &[&str]) -> Value {
    Value::Array(values.iter().map(|v| json!({"id":v,"label":v})).collect())
}
pub fn models(value: &Value) -> Vec<Value> {
    value
        .as_array()
        .or_else(|| value["data"].as_array())
        .or_else(|| value["models"].as_array())
        .cloned()
        .unwrap_or_default()
}
pub fn model_choices(value: &Value) -> Value {
    Value::Array(models(value).iter().filter_map(|m| {
        let id = m["model"].as_str().or_else(|| m["id"].as_str()).or_else(|| m["value"].as_str())?;
        Some(json!({"id":id,"label":m["displayName"].as_str().or_else(|| m["name"].as_str()).unwrap_or(id),"description":m["description"].as_str(),"metadata":m}))
    }).collect())
}
pub fn questions(value: &Value) -> Value {
    Value::Array(value.as_array().map(|v| v.iter().enumerate().map(|(i,q)| {
        let id = q["id"].as_str().map(String::from).unwrap_or_else(|| i.to_string());
        let options:Vec<_> = q["options"].as_array().map(|o| o.iter().filter_map(|c| {
            let label = c.as_str().or_else(|| c["label"].as_str())?;
            Some(json!({"id":label,"label":label,"description":c["description"]}))
        }).collect()).unwrap_or_default();
        json!({"id":id,"prompt":q["question"].as_str().or_else(|| q["prompt"].as_str()).unwrap_or("Answer the question"),"options":options,"multiple":q["multiSelect"]==true || q["multiple"]==true,"allowText":q["isOther"].as_bool().or_else(|| q["custom"].as_bool()).unwrap_or(true),"secret":q["isSecret"]==true})
    }).collect()).unwrap_or_default())
}

/// Parse the adapter's common descriptors, rejecting malformed catalogs.
pub fn descriptors(value: Value) -> Result<Vec<swarmie::SettingDescriptor>> {
    serde_json::from_value(value)
        .map_err(|error| Error::Provider(format!("invalid configuration descriptors: {error}")))
}
pub fn model_descriptors(value: Value) -> Result<Vec<swarmie::ModelDescriptor>> {
    let mut entries = value
        .as_array()
        .cloned()
        .ok_or_else(|| Error::Provider("model catalog must be an array".into()))?;
    for entry in &mut entries {
        let object = entry
            .as_object_mut()
            .ok_or_else(|| Error::Provider("model descriptor must be an object".into()))?;
        let extra: Vec<_> = object
            .keys()
            .filter(|key| {
                !["id", "label", "description", "provider", "metadata"].contains(&key.as_str())
            })
            .cloned()
            .collect();
        let mut metadata = object
            .remove("metadata")
            .and_then(|value| value.as_object().cloned())
            .unwrap_or_default();
        for key in extra {
            metadata.insert(key.clone(), object.remove(&key).unwrap());
        }
        object.insert("metadata".into(), Value::Object(metadata));
    }
    serde_json::from_value(json!(entries))
        .map_err(|error| Error::Provider(format!("invalid model descriptors: {error}")))
}
