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
                let runtime = self.runtime()?;
                let project = runtime.project(text(p, "projectId")?).await?;
                let project = if command == "project.set" {
                    if p.as_object().unwrap().keys().any(|k| {
                        !matches!(
                            k.as_str(),
                            "projectId"
                                | "expectedRevision"
                                | "harness"
                                | "model"
                                | "config"
                                | "directoryPath"
                        )
                    }) {
                        return Err(Error::Invalid("unknown project default field".into()));
                    }
                    let expected_revision = p["expectedRevision"]
                        .as_u64()
                        .ok_or_else(|| Error::Invalid("expectedRevision is required".into()))?;
                    let mut merged = serde_json::to_value(&project.launch)?;
                    for key in ["harness", "model", "config"] {
                        if let Some(value) = p.get(key) {
                            if key == "config" && value.is_object() {
                                merge_config(&mut merged[key], value);
                            } else {
                                merged[key] = value.clone();
                            }
                        }
                    }
                    let launch: ProjectLaunchDefaults = serde_json::from_value(merged)?;
                    validate_config(&launch.config)?;
                    let directory = if let Some(path) = p["directoryPath"].as_str() {
                        Some(
                            runtime
                                .register_directory(std::path::Path::new(path))
                                .await?
                                .reference(),
                        )
                    } else {
                        None
                    };
                    runtime
                        .update_project(ProjectUpdate {
                            project_id: project.id,
                            expected_revision,
                            name: None,
                            archived: None,
                            directory,
                            unlink_directory: false,
                            launch: Some(launch),
                        })
                        .await?
                } else {
                    project
                };
                let catalog = runtime.local_catalog().await?;
                let cwd = project
                    .directory
                    .as_ref()
                    .map(|d| catalog.directory(d).map(|r| r.path.clone()))
                    .transpose()?;
                let mut value = serde_json::to_value(&project.launch)?;
                value["projectId"] = json!(project.id);
                value["revision"] = json!(project.revision);
                value["cwd"] = json!(cwd);
                Ok(value)
            }
            _ => Err(Error::Unsupported(command.into())),
        }
    }
    pub(crate) async fn launch_options(&self, p: &Value) -> Result<SpawnOptions> {
        let runtime = self.runtime()?;
        if let Some(session_id) = p["sessionId"].as_str() {
            if p["projectId"].is_string() || p["projectRevision"].is_number() {
                return Err(Error::Invalid(
                    "select a retained session or a current project, not both".into(),
                ));
            }
            let directory = self.resource_cwd(p).await?;
            let saved = runtime.snapshot(session_id).await?;
            let mut options = SpawnOptions::new(
                p["harness"].as_str().unwrap_or(&saved.options.harness),
                directory,
            );
            options.model = p["model"]
                .as_str()
                .map(str::to_owned)
                .or(saved.options.model);
            options.config = saved.options.config;
            if let Some(patch) = p.get("config") {
                merge_config(&mut options.config, patch);
            }
            validate_config(&options.config)?;
            return Ok(options);
        }
        let project = if let Some(id) = p["projectId"].as_str() {
            Some(runtime.project(id).await?)
        } else {
            None
        };
        let mut options = SpawnOptions::new(
            p["harness"]
                .as_str()
                .or(project.as_ref().and_then(|p| p.launch.harness.as_deref()))
                .filter(|h| !h.is_empty())
                .ok_or_else(|| Error::Invalid("harness is required".into()))?,
            p["cwd"].as_str().unwrap_or_default(),
        );
        options.project_id = project.as_ref().map(|p| p.id.clone());
        options.project_revision = p["projectRevision"]
            .as_u64()
            .or(project.as_ref().map(|p| p.revision));
        options.model = p["model"].as_str().map(str::to_owned);
        options.config = p.get("config").cloned().unwrap_or(Value::Null);
        validate_config(&options.config)?;
        Ok(options)
    }
    pub(crate) async fn resource_cwd(&self, p: &Value) -> Result<PathBuf> {
        if let Some(session_id) = p["sessionId"].as_str() {
            let status = self.runtime()?.session_context(session_id).await?;
            if !status.available {
                return Err(Error::Invalid(
                    status
                        .reason
                        .unwrap_or_else(|| "working directory unavailable".into()),
                ));
            }
            if p.get("cwd").is_some() && cwd(p)? != status.context.directory.path {
                return Err(Error::Invalid(
                    "cwd conflicts with the retained session context".into(),
                ));
            }
            Ok(status.context.directory.path)
        } else {
            cwd(p)
        }
    }
    pub(crate) async fn account_options(&self, p: &Value) -> Result<SpawnOptions> {
        self.runtime()?
            .resolve_launch_options(self.launch_options(p).await?)
            .await
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

pub(crate) fn validate_config(value: &Value) -> Result<()> {
    validate_project_config(value)
}
