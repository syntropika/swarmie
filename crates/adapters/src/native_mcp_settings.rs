use super::*;
use std::collections::BTreeMap;

pub(super) fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(String::from))
        .collect()
}
pub(super) fn env_ref(value: &str) -> Option<String> {
    if let Some(name) = value.strip_prefix('$')
        && !name.is_empty()
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        return Some(name.into());
    }
    value
        .strip_prefix("{env:")
        .and_then(|v| v.strip_suffix('}'))
        .or_else(|| value.strip_prefix("${").and_then(|v| v.strip_suffix('}')))
        .map(String::from)
}
pub(super) fn definition(harness: &str, value: &Value) -> McpDefinition {
    let command = if harness == "opencode" {
        value["command"][0].as_str()
    } else {
        value["command"].as_str()
    };
    let mut result = McpDefinition {
        transport: if command.is_some() {
            "stdio".into()
        } else if value["type"] == "sse" {
            "sse".into()
        } else {
            "http".into()
        },
        command: command.map(PathBuf::from),
        args: if harness == "opencode" {
            strings(&value["command"]).into_iter().skip(1).collect()
        } else {
            strings(&value["args"])
        },
        url: value["url"].as_str().map(String::from),
        env_vars: strings(&value["env_vars"]),
        ..Default::default()
    };
    result.url = result
        .url
        .as_ref()
        .and_then(|url| url::Url::parse(url).ok())
        .map(|mut parsed| {
            parsed.set_password(None).ok();
            parsed.set_username("").ok();
            parsed.set_query(None);
            parsed.set_fragment(None);
            parsed.into()
        });
    if harness == "codex" {
        result.bearer_token_env_var = value["bearer_token_env_var"].as_str().map(String::from);
        result.header_env_vars = value["env_http_headers"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.into())))
            .collect();
    } else {
        let env = if harness == "opencode" {
            "environment"
        } else {
            "env"
        };
        for (key, val) in value[env].as_object().into_iter().flatten() {
            if val.as_str().and_then(env_ref).as_deref() == Some(key) {
                result.env_vars.push(key.clone());
            }
        }
    }
    if harness != "codex" {
        for (key, value) in value["headers"].as_object().into_iter().flatten() {
            if let Some(variable) = value.as_str().and_then(env_ref) {
                result.header_env_vars.insert(key.clone(), variable);
            } else if let Some(variable) = value
                .as_str()
                .and_then(|v| v.strip_prefix("Bearer "))
                .and_then(env_ref)
                && key.eq_ignore_ascii_case("authorization")
            {
                result.bearer_token_env_var = Some(variable);
            }
        }
    }
    if !credentials(harness, value).is_empty() && result.transport == "stdio" {
        // Native process arguments may carry credentials; keep them local during edits.
        result.args = vec![];
    }
    result
}
pub(super) fn credentials(harness: &str, value: &Value) -> Vec<String> {
    let mut fields = vec![];
    for key in ["env", "environment", "headers", "http_headers"] {
        for (name, val) in value[key].as_object().into_iter().flatten() {
            let reference = val.as_str().and_then(env_ref).or_else(|| {
                if matches!(key, "headers" | "http_headers")
                    && name.eq_ignore_ascii_case("authorization")
                {
                    val.as_str()
                        .and_then(|value| value.strip_prefix("Bearer "))
                        .and_then(env_ref)
                } else {
                    None
                }
            });
            if reference.is_none()
                || matches!(key, "env" | "environment")
                    && val.as_str().and_then(env_ref).as_deref() != Some(name)
            {
                fields.push(format!("{key}.{name}"));
            }
        }
    }
    if value["url"].as_str().is_some_and(|url| {
        url::Url::parse(url).map_or(true, |u| {
            !u.username().is_empty() || u.password().is_some() || u.query().is_some()
        })
    }) {
        fields.push("url".into());
    }
    let args = if harness == "opencode" {
        strings(&value["command"])
            .into_iter()
            .skip(1)
            .collect::<Vec<_>>()
    } else {
        strings(&value["args"])
    };
    if !args.is_empty() {
        fields.push("args".into());
    }
    fields
}
pub(super) fn native_settings(harness: &str, definition: &McpDefinition) -> Result<Value> {
    definition.validate()?;
    if matches!(harness, "pi" | "codex") && definition.transport == "sse" {
        return Err(Error::Unsupported(
            "This harness supports Streamable HTTP, not legacy SSE".into(),
        ));
    }
    let mut value = json!({});
    if definition.transport == "stdio" {
        if harness == "opencode" {
            let mut command = vec![
                definition
                    .command
                    .as_ref()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            ];
            command.extend(definition.args.clone());
            value = json!({"type":"local","command":command});
        } else {
            value = json!({"command":definition.command,"args":definition.args});
        }
        if harness == "codex" {
            value["env_vars"] = json!(definition.env_vars);
        } else {
            let env: BTreeMap<_, _> = definition
                .env_vars
                .iter()
                .map(|name| {
                    (
                        name.clone(),
                        if harness == "opencode" {
                            format!("{{env:{name}}}")
                        } else {
                            format!("${{{name}}}")
                        },
                    )
                })
                .collect();
            if !env.is_empty() {
                value[if harness == "opencode" {
                    "environment"
                } else {
                    "env"
                }] = json!(env);
            }
        }
    } else {
        value["url"] = json!(definition.url);
        if harness == "codex" {
            if let Some(key) = &definition.bearer_token_env_var {
                value["bearer_token_env_var"] = json!(key);
            }
            value["env_http_headers"] = json!(definition.header_env_vars);
        } else {
            value["type"] = json!(if harness == "opencode" {
                "remote"
            } else {
                definition.transport.as_str()
            });
            let reference = |name: &str| {
                if harness == "opencode" {
                    format!("{{env:{name}}}")
                } else {
                    format!("${{{name}}}")
                }
            };
            let mut headers: BTreeMap<String, String> = definition
                .header_env_vars
                .iter()
                .map(|(key, name)| (key.clone(), reference(name)))
                .collect();
            if let Some(name) = &definition.bearer_token_env_var {
                headers.insert(
                    "Authorization".into(),
                    format!("Bearer {}", reference(name)),
                );
            }
            value["headers"] = json!(headers);
        }
    }
    Ok(value)
}
