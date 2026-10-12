use super::*;

pub(super) async fn chat_directory(call: &mut Calls<'_>, params: &Value) -> Outcome {
    let set = params["set"] == true;
    if !set && params.get("path").is_some() {
        return Err(invalid("path requires an explicit directory change"));
    }
    let selected = call.call("settings/directory", json!({}), false).await?;
    if !set && !selected.is_null() {
        return call
            .call("directory/get", selected["directory"].clone(), false)
            .await;
    }
    let args = match params.get("path") {
        Some(path) if path.as_str().is_some_and(|s| !s.trim().is_empty()) => {
            json!({"path":path.as_str().unwrap().trim()})
        }
        None | Some(Value::Null) => json!({}),
        _ => return Err(invalid("directory path must be nonempty text")),
    };
    let initial = call
        .extension("workspace", "conversation.directory", args, true)
        .await?;
    let directory = call
        .call("directory/register", json!({"path":initial["path"]}), true)
        .await?;
    call.call(
        "settings/setDirectory",
        json!({"expectedRevision":selected["revision"].as_u64().unwrap_or(0),
        "directory":{"id":directory["id"],"revision":directory["revision"]}}),
        true,
    )
    .await?;
    Ok(directory)
}
async fn record_path(call: &mut Calls<'_>, record: &Value) -> Outcome {
    if record["directory"].is_null() {
        return Err(invalid("the project folder is unavailable"));
    }
    call.call("directory/get", record["directory"].clone(), false)
        .await
}
async fn view(call: &mut Calls<'_>, record: Value) -> Outcome {
    let directory = record_path(call, &record).await?;
    let path = text(&directory, "path")?;
    let extensions = call.extensions().await?;
    if extensions.iter().any(|e| e["id"] == "git") {
        match call
            .extension("git", "project.open", json!({"path":path}), false)
            .await
        {
            Ok(mut git) => {
                git["gitId"] = git["id"].clone();
                git["id"] = record["id"].clone();
                git["revision"] = record["revision"].clone();
                git["name"] = record["name"].clone();
                git["git"] = true.into();
                return Ok(git);
            }
            Err(error)
                if !unavailable(&error) && extensions.iter().any(|e| e["id"] == "workspace") => {}
            Err(error) => return Err(error),
        }
    }
    let folder = call
        .extension("workspace", "project.open", json!({"path":path}), false)
        .await?;
    Ok(
        json!({"id":record["id"],"revision":record["revision"],"path":folder["path"],"name":record["name"],"git":false,
        "worktrees":[{"id":folder["id"],"path":folder["path"],"branch":null,"isMain":true,"locked":false,"prunable":false}]}),
    )
}
pub(super) async fn project(call: &mut Calls<'_>, params: &Value, method: &str) -> Outcome {
    if method == "client/projectCatalog" {
        let records = call.call("project/list", json!({}), false).await?;
        let records = records
            .as_array()
            .ok_or_else(|| failure("invalid project catalog"))?;
        let mut projects = vec![];
        for record in records.iter().filter(|r| r["archived"] != true) {
            match view(call, record.clone()).await {
                Ok(project) => projects.push(project),
                Err(error) if !unavailable(&error) => {
                    let path = if record["directory"].is_null() {
                        Value::String(String::new())
                    } else {
                        match call
                            .call("directory/get", record["directory"].clone(), false)
                            .await
                        {
                            Ok(directory) => directory["path"].clone(),
                            Err(error) if unavailable(&error) => return Err(error),
                            Err(_) => Value::String(String::new()),
                        }
                    };
                    projects.push(json!({"id":record["id"],"revision":record["revision"],"name":record["name"],"path":path,
                        "git":false,"worktrees":[],"error":error["message"]}));
                }
                Err(error) => return Err(error),
            }
        }
        return Ok(projects.into());
    }
    let name = params
        .get("name")
        .map(|name| {
            name.as_str()
                .filter(|s| !s.trim().is_empty())
                .map(str::trim)
                .ok_or_else(|| invalid("project name must be nonempty text"))
        })
        .transpose()?;
    if method == "client/projectGet" {
        text(params, "projectId")?;
    }
    let mut record = if params.get("projectId").is_some() {
        call.call(
            "project/get",
            json!({"projectId":text(params,"projectId")?}),
            false,
        )
        .await?
    } else {
        let path = text(params, "path")?;
        let directory = call
            .call("directory/register", json!({"path":path}), true)
            .await?;
        let records = call.call("project/list", json!({}), false).await?;
        let existing = records.as_array().and_then(|rs| {
            rs.iter()
                .find(|r| r["archived"] != true && r["directory"]["id"] == directory["id"])
        });
        match existing {
            Some(record) => record.clone(),
            None => {
                call.call(
                    "project/create",
                    json!({"name":path_name(text(&directory,"path")?),
                "directory":{"id":directory["id"],"revision":directory["revision"]}}),
                    true,
                )
                .await?
            }
        }
    };
    if let Some(name) = name
        && record["name"] != name
    {
        record = call
            .call(
                "project/update",
                json!({"projectId":record["id"],"expectedRevision":record["revision"],"name":name}),
                true,
            )
            .await?;
    }
    view(call, record).await
}
fn path_name(path: &str) -> &str {
    path.rsplit('/').find(|s| !s.is_empty()).unwrap_or(path)
}

pub(super) async fn git_arguments(call: &mut Calls<'_>, mut args: Value) -> Outcome {
    if let Some(id) = args.get("projectId") {
        let record = call
            .call("project/get", json!({"projectId":id}), false)
            .await?;
        let directory = record_path(call, &record).await?;
        let git = call
            .extension(
                "git",
                "project.open",
                json!({"path":directory["path"]}),
                false,
            )
            .await?;
        args["projectId"] = git["id"].clone();
    }
    Ok(args)
}
pub(super) async fn invoke(call: &mut Calls<'_>, params: &Value) -> Outcome {
    let extension = text(params, "extensionId")?;
    let command = text(params, "command")?;
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    let args = if extension == "git" {
        git_arguments(call, args).await?
    } else {
        args
    };
    call.extension(extension, command, args, true).await
}
pub(super) async fn worktree(call: &mut Calls<'_>, params: &Value) -> Outcome {
    if params.get("sessionId").is_some_and(|s| !s.is_null()) {
        return Err(invalid("start a new chat before changing its worktree"));
    }
    let id = text(params, "projectId")?;
    let action = text(params, "action")?;
    match action {
        "create" => {
            text(params, "branch")?;
        }
        "remove" => {
            text(params, "worktreeId")?;
        }
        _ => return Err(invalid("worktree action must be create or remove")),
    }
    let mut args = git_arguments(call, json!({"projectId":id})).await?;
    let tree = match action {
        "create" => {
            args["branch"] = text(params, "branch")?.into();
            args["setup"] = true.into();
            Some(
                call.extension("git", "worktree.create", args.clone(), true)
                    .await?,
            )
        }
        "remove" => {
            args["worktreeId"] = text(params, "worktreeId")?.into();
            call.extension("git", "worktree.remove", args.clone(), true)
                .await?;
            None
        }
        _ => return Err(invalid("worktree action must be create or remove")),
    };
    match call.extension("git", "worktree.list", args, false).await {
        Ok(worktrees) => Ok(json!({"tree":tree,"worktrees":worktrees})),
        Err(mut error) => {
            error_data(&mut error)["createdWorktree"] = json!(tree);
            Err(error)
        }
    }
}

pub(super) async fn defaults(call: &mut Calls<'_>, params: &Value) -> Outcome {
    let record = call
        .call(
            "project/get",
            json!({"projectId":text(params,"projectId")?}),
            false,
        )
        .await?;
    let directory = record_path(call, &record).await?;
    let mut defaults = record["launch"].clone();
    defaults["cwd"] = directory["path"].clone();
    defaults["revision"] = record["revision"].clone();
    Ok(defaults)
}
