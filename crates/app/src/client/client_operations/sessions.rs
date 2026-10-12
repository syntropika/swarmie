use super::*;
use serde::Deserialize;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Launch {
    harness: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    config: Value,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    project_revision: Option<u64>,
    #[serde(default)]
    directory_path: Option<String>,
}
async fn options(call: &mut Calls<'_>, params: &Value) -> Outcome {
    let intent: Launch =
        serde_json::from_value(params.clone()).map_err(|e| invalid(&e.to_string()))?;
    if intent.harness.trim().is_empty() || (!intent.config.is_null() && !intent.config.is_object())
    {
        return Err(invalid("a harness and configuration object are required"));
    }
    if intent.project_id.is_none()
        && (intent.project_revision.is_some() || intent.directory_path.is_some())
    {
        return Err(invalid(
            "project revision and worktree selection require a project",
        ));
    }
    let mut options = json!({"harness":intent.harness,"model":intent.model,"config":if intent.config.is_null() { json!({}) } else { intent.config }});
    if let Some(project) = intent.project_id {
        let record = call
            .call("project/get", json!({"projectId":project}), false)
            .await?;
        if Some(
            record["revision"]
                .as_u64()
                .ok_or_else(|| failure("invalid project revision"))?,
        ) != intent.project_revision
        {
            return Err(invalid(
                "the project defaults changed; refresh the project before launching",
            ));
        }
        if record["directory"].is_null() {
            return Err(invalid("the project folder is unavailable"));
        }
        let directory = call
            .call("directory/get", record["directory"].clone(), false)
            .await?;
        match intent.directory_path.as_deref() {
            None => {
                options["projectId"] = project.into();
                options["projectRevision"] = record["revision"].clone();
            }
            Some(path) if path == directory["path"].as_str().unwrap_or("") => {
                options["projectId"] = project.into();
                options["projectRevision"] = record["revision"].clone();
            }
            Some(path) => {
                let args =
                    super::projects::git_arguments(call, json!({"projectId":project})).await?;
                let trees = call.extension("git", "worktree.list", args, false).await?;
                if !trees.as_array().is_some_and(|trees| {
                    trees
                        .iter()
                        .any(|t| (t["id"] == path || t["path"] == path) && t["prunable"] != true)
                }) {
                    return Err(invalid(
                        "the selected worktree does not belong to this project",
                    ));
                }
                options["cwd"] = path.into();
            }
        }
    } else {
        // Projectless sessions use the runtime's selected directory, not a client-local fallback.
        super::projects::chat_directory(call, &json!({})).await?;
    }
    Ok(options)
}
pub(super) async fn launch(call: &mut Calls<'_>, params: &Value, preview: bool) -> Outcome {
    let options = options(call, params).await?;
    call.call(
        if preview {
            "runtime/configuration"
        } else {
            "session/create"
        },
        options,
        !preview,
    )
    .await
}
pub(super) async fn controls(call: &mut Calls<'_>, session: &str) -> Outcome {
    let snapshot = call
        .call("session/get", json!({"sessionId":session}), false)
        .await?;
    let configuration = call
        .call("agent/configuration", json!({"sessionId":session}), false)
        .await?;
    let models = if snapshot["capabilities"]["models"]["availability"] == "available" {
        call.call("agent/models", json!({"sessionId":session}), false)
            .await?
    } else {
        json!([])
    };
    let mut values = serde_json::Map::new();
    let mut policy = snapshot["executionPolicy"].clone();
    for descriptor in configuration
        .as_array()
        .ok_or_else(|| failure("invalid session controls"))?
    {
        if descriptor["id"] == "executionProfile" {
            if !descriptor["metadata"]["policy"].is_null() {
                policy = descriptor["metadata"]["policy"].clone();
            }
        } else if let (Some(id), Some(value)) = (descriptor["id"].as_str(), descriptor.get("value"))
        {
            values.insert(id.into(), value.clone());
        }
    }
    Ok(
        json!({"configuration":configuration,"models":models,"values":values,"executionPolicy":policy}),
    )
}
pub(super) async fn configure(call: &mut Calls<'_>, params: &Value) -> Outcome {
    let session = text(params, "sessionId")?;
    if !params["config"].is_object() {
        return Err(invalid("configuration must be an object"));
    }
    match call.call("agent/configure", params.clone(), true).await {
        Ok(receipt) => Ok(receipt),
        Err(mut error) => {
            // Recover observed settings without repeating a potentially partially applied update.
            match controls(call, session).await {
                Ok(controls) => error_data(&mut error)["controls"] = controls,
                Err(recovery) => error_data(&mut error)["recoveryError"] = recovery,
            }
            Err(error)
        }
    }
}
pub(super) async fn open(
    call: &mut Calls<'_>,
    submissions: &ClientSubmissions,
    request: &Value,
) -> Outcome {
    let session = text(&request["params"], "sessionId")?;
    let mode = request["params"]["resume"].as_str().unwrap_or("active");
    if !matches!(mode, "active" | "explicit" | "never") {
        return Err(invalid("invalid session resume mode"));
    }
    let mut projection =
        crate::client_projection::restore(call.nodes, submissions, call.node, request.clone())
            .await
            .map_err(|e| failure(&e.to_string()))?["result"]
            .clone();
    let snapshot = &projection["snapshot"];
    let unresolved = projection["unresolved"] == true;
    if unresolved && mode == "explicit" {
        return Err(invalid(
            "resolve the unconfirmed input before resuming this session",
        ));
    }
    let inactive = matches!(snapshot["state"].as_str(), Some("closed" | "failed"));
    let resume = mode != "never"
        && !unresolved
        && (mode == "explicit" || (!inactive && snapshot["metadata"]["archived"] != true));
    let mut activation = json!({"status":if unresolved { "blocked" } else { "inactive" }});
    if resume {
        if inactive && snapshot["capabilities"]["resume"]["availability"] != "available" {
            return Err(invalid(
                "this harness cannot resume its saved session; start a new chat to preserve history",
            ));
        }
        let resources = call
            .extensions()
            .await?
            .iter()
            .any(|e| e["id"] == "resources");
        let resumed = if resources {
            call.extension(
                "resources",
                "session.resume",
                json!({"sessionId":session}),
                true,
            )
            .await
        } else {
            call.call("session/resume", json!({"sessionId":session}), true)
                .await
        };
        match resumed {
            Ok(_) => {
                projection = crate::client_projection::restore(
                    call.nodes,
                    submissions,
                    call.node,
                    request.clone(),
                )
                .await
                .map_err(|e| failure(&e.to_string()))?["result"]
                    .clone();
                activation = json!({"status":"attached"});
                projection["queue"] = call
                    .call("session/queue/list", json!({"sessionId":session}), false)
                    .await?;
            }
            Err(error) if mode == "active" && !unavailable(&error) => {
                activation = json!({"status":"unavailable","error":error})
            }
            Err(error) => return Err(error),
        }
    }
    projection["activation"] = activation;
    Ok(projection)
}
