use crate::*;
use serde_json::json;
use std::fs;

pub(crate) fn cwd(params: &Value) -> Result<PathBuf> {
    let path = fs::canonicalize(text(params, "cwd")?)?;
    if !path.is_dir() {
        return Err(Error::Invalid(
            "project directory is not a directory".into(),
        ));
    }
    Ok(path)
}
impl ResourcesExtension {
    pub(crate) async fn settings_command(&self, command: &str, p: &Value) -> Result<Value> {
        match command {
            "credentials.status" => Ok(
                json!({"credentials":self.0.credentials.lock().await.keys().map(|id|json!({"id":id,"configured":true})).collect::<Vec<_>>()}),
            ),
            "credentials.set" | "credentials.remove" => {
                let id = text(p, "id")?;
                identifier(id)?;
                let mut credentials = self.0.credentials.lock().await;
                if command == "credentials.set" {
                    capacity(credentials.contains_key(id), credentials.len())?;
                    credentials.insert(id.into(), text(p, "value")?.into());
                } else {
                    credentials.remove(id);
                }
                save(&self.0.root.join("credentials.json"), &*credentials)?;
                Ok(if command == "credentials.set" {
                    json!({"id":id,"configured":true})
                } else {
                    json!({"removed":true})
                })
            }
            "project.get" | "project.set" => {
                let cwd = cwd(p)?;
                let key = cwd.to_string_lossy().into_owned();
                let mut state = self.0.state.lock().await;
                if command == "project.set" {
                    if p.as_object().unwrap().keys().any(|key| {
                        !matches!(
                            key.as_str(),
                            "cwd" | "harness" | "model" | "config" | "workspace"
                        )
                    }) {
                        return Err(Error::Invalid("unknown project default field".into()));
                    }
                    let mut merged = state
                        .projects
                        .get(&key)
                        .map(serde_json::to_value)
                        .transpose()?
                        .unwrap_or_else(|| json!({}));
                    for (key, value) in p.as_object().unwrap() {
                        let fields = merged.as_object_mut().unwrap();
                        if key == "config" && value.is_object() {
                            merge_config(fields.entry(key).or_insert(Value::Null), value);
                        } else {
                            fields.insert(key.clone(), value.clone());
                        }
                    }
                    let mut defaults: ProjectDefaults = serde_json::from_value(merged)?;
                    defaults.cwd = cwd.clone();
                    validate_config(&defaults.config)?;
                    if !defaults.config.is_null() && !defaults.config.is_object() {
                        return Err(Error::Invalid("project config must be an object".into()));
                    }
                    if let Some(workspace) = &defaults.workspace {
                        let path = fs::canonicalize(workspace)?;
                        if !path.is_dir() {
                            return Err(Error::Invalid(
                                "workspace must be an existing directory".into(),
                            ));
                        }
                        defaults.workspace = Some(path);
                    }
                    capacity(state.projects.contains_key(&key), state.projects.len())?;
                    state.projects.insert(key.clone(), defaults);
                    self.persist(&state)?;
                }
                Ok(serde_json::to_value(
                    state
                        .projects
                        .get(&key)
                        .cloned()
                        .unwrap_or(ProjectDefaults {
                            cwd,
                            ..Default::default()
                        }),
                )?)
            }
            _ => Err(Error::Unsupported(command.into())),
        }
    }
    pub(crate) async fn launch_options(&self, p: &Value) -> Result<SpawnOptions> {
        let cwd = cwd(p)?;
        let state = self.0.state.lock().await;
        let defaults = state.projects.get(&cwd.to_string_lossy().into_owned());
        let harness = p["harness"]
            .as_str()
            .or(defaults.and_then(|d| d.harness.as_deref()))
            .filter(|value| !value.is_empty())
            .ok_or_else(|| Error::Invalid("harness is required".into()))?
            .to_owned();
        let model = p["model"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| defaults.and_then(|d| d.model.clone()));
        let mut config = defaults.map(|d| d.config.clone()).unwrap_or(Value::Null);
        validate_config(&config)?;
        if let Some(patch) = p.get("config").filter(|value| !value.is_null()) {
            if !patch.is_object() {
                return Err(Error::Invalid("config override must be an object".into()));
            }
            validate_config(patch)?;
            merge_config(&mut config, patch);
        }
        Ok(SpawnOptions {
            harness,
            cwd,
            model,
            config,
            execution: SessionExecutionOptions::default(),
        })
    }
    pub(crate) async fn account_options(&self, p: &Value) -> Result<SpawnOptions> {
        self.launch_options(p).await
    }
}

fn merge_config(base: &mut Value, patch: &Value) {
    if let Value::Object(fields) = patch {
        if !base.is_object() {
            *base = json!({});
        }
        let base = base.as_object_mut().unwrap();
        for (key, value) in fields {
            if value.is_object() {
                merge_config(base.entry(key).or_insert(Value::Null), value);
            } else {
                base.insert(key.clone(), value.clone());
            }
        }
    } else {
        *base = patch.clone();
    }
}

fn sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key == "env"
        || key == "headers"
        || key.contains("token")
        || key.contains("secret")
        || key.contains("password")
        || key.contains("apikey")
        || key.contains("api_key")
        || key.contains("api-key")
        || key == "authorization"
}
fn sensitive_value(key: &str, value: &Value) -> bool {
    if value.is_null() {
        return false;
    }
    let key_lower = key.to_ascii_lowercase();
    if key_lower.contains("token")
        && (value.is_number() || value.is_boolean())
        && ![
            "auth", "access", "refresh", "bearer", "secret", "password", "apikey", "api_key",
            "api-key",
        ]
        .iter()
        .any(|marker| key_lower.contains(marker))
    {
        return false;
    }
    sensitive_key(key)
}
pub(crate) fn validate_config(value: &Value) -> Result<()> {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                if sensitive_value(key, value) {
                    return Err(Error::Invalid("native configuration contains a sensitive field; use native harness credentials and clear saved native config with config:null before launching".into()));
                }
                validate_config(value)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                validate_config(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}
