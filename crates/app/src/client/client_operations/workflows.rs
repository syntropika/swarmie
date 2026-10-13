//! Built-in extension workflows own preparation, execution and partial results.
use super::*;
pub(super) async fn create_project(call: &mut Calls<'_>, params: &Value) -> Outcome {
    let source = text(params, "source")?;
    if !matches!(source, "create" | "clone" | "scratch") {
        return Err(invalid("invalid project source"));
    }
    if params.get("name").is_some() {
        text(params, "name")?;
    }
    let created = call
        .extension(
            "workspace",
            &format!("project.{source}"),
            params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({})),
            true,
        )
        .await?;
    let mut open = json!({"path":created["path"]});
    if let Some(name) = params.get("name") {
        open["name"] = name.clone();
    }
    match super::projects::project(call, &open, "client/projectOpen").await {
        Ok(project) => Ok(project),
        Err(mut error) => {
            error_data(&mut error)["createdFolder"] = created;
            Err(error)
        }
    }
}
pub(super) async fn git_text(call: &mut Calls<'_>, params: &Value) -> Outcome {
    let kind = text(params, "kind")?;
    if !matches!(kind, "commit" | "pr") {
        return Err(invalid("invalid generated text kind"));
    }
    let mut args = super::projects::git_arguments(call, params["arguments"].clone()).await?;
    args["kind"] = kind.into();
    let prepared = call.extension("git", "text.prompt", args, false).await?;
    let mut launch = super::sessions::options(call, &params["launch"]).await?;
    launch["text"] = prepared["prompt"].clone();
    let generated = call.call("runtime/generate", launch, true).await?;
    let text = generated
        .as_str()
        .ok_or_else(|| failure("invalid generated text"))?
        .trim();
    let draft = if kind == "commit" {
        json!({"message":text})
    } else {
        let stripped = text
            .strip_prefix("```json")
            .or_else(|| text.strip_prefix("```"))
            .unwrap_or(text)
            .trim();
        let text = stripped.strip_suffix("```").unwrap_or(stripped).trim();
        let parsed: Value = serde_json::from_str(text).map_err(|_| {
            invalid("the harness did not return a PR title and description; enter them below")
        })?;
        if !parsed["title"].is_string() || !parsed["body"].is_string() {
            return Err(invalid(
                "the harness did not return a PR title and description; enter them below",
            ));
        }
        json!({"title":parsed["title"],"body":parsed["body"]})
    };
    Ok(json!({"draft":draft,"truncated":prepared["truncated"]}))
}
pub(super) async fn pr_projection(call: &mut Calls<'_>, params: &Value) -> Outcome {
    let mut args = super::projects::git_arguments(call, params.clone()).await?;
    let pr = call
        .extension("git", "pr.status", args.clone(), false)
        .await?;
    args["number"] = pr["number"].clone();
    let discussion = call
        .extension("git", "pr.comments", args.clone(), false)
        .await?;
    let checks = call
        .extension("git", "pr.checks", args.clone(), false)
        .await?;
    let viewed = call.extension("git", "pr.viewed.list", args, false).await?;
    Ok(json!({"pr":pr,"discussion":discussion,"checks":checks,"files":viewed["files"]}))
}
pub(super) async fn file_save(call: &mut Calls<'_>, params: &Value) -> Outcome {
    let extension = text(params, "extensionId")?;
    let args = &params["arguments"];
    let baseline = text(params, "baseline").or_else(|error| {
        if params["baseline"] == "" {
            Ok("")
        } else {
            Err(error)
        }
    })?;
    let read = super::projects::invoke(
        call,
        &json!({"extensionId":extension,"command":"files.read","arguments":args}),
    )
    .await?;
    if read["content"] != baseline {
        return Err(invalid(
            "this file changed on disk; copy your draft or reload the file before saving",
        ));
    }
    super::projects::invoke(
        call,
        &json!({"extensionId":extension,"command":"files.write","arguments":args}),
    )
    .await
}
pub(super) async fn comparison(call: &mut Calls<'_>, params: &Value) -> Outcome {
    let targets = params["targets"]
        .as_array()
        .filter(|targets| (2..=8).contains(&targets.len()))
        .ok_or_else(|| invalid("choose between two and eight targets"))?;
    let task = text(params, "text")?;
    for target in targets {
        text(target, "harness")?;
        if target
            .get("model")
            .is_some_and(|m| !m.is_null() && !m.is_string())
        {
            return Err(invalid("invalid target model"));
        }
    }
    let cwd = super::sessions::directory(call, &params["launch"]).await?;
    let isolated = params["isolated"] == true;
    let mut git_args = if isolated {
        Some(
            super::projects::git_arguments(
                call,
                json!({"projectId":text(&params["launch"],"projectId")?}),
            )
            .await?,
        )
    } else {
        None
    };
    let mut trees = Vec::new();
    let mut launches = Vec::new();
    let result = async {
        for target in targets {
            let mut launch = json!({"harness":target["harness"],"model":target["model"]});
            if let Some(args) = git_args.as_mut() {
                args["branch"] =
                    format!("comparison-{}", &uuid::Uuid::new_v4().to_string()[..8]).into();
                args["setup"] = true.into();
                let tree = call
                    .extension("git", "worktree.create", args.clone(), true)
                    .await?;
                launch["cwd"] = tree["path"].clone();
                trees.push(tree);
            }
            launches.push(launch);
        }
        call.extension(
            "resources",
            "fanout.start",
            json!({"cwd":cwd,"text":task,"targets":launches}),
            true,
        )
        .await
    }
    .await;
    result.map_err(|mut error| {
        error_data(&mut error)["createdWorktrees"] = trees.into();
        error
    })
}
pub(super) async fn import(
    call: &mut Calls<'_>,
    submissions: &ClientSubmissions,
    request: &Value,
) -> Outcome {
    let session = call
        .call("session/import", request["params"].clone(), true)
        .await?;
    let mut query = request.clone();
    query["params"] = json!({"sessionId":session["id"]});
    super::sessions::open(call, submissions, &query)
        .await
        .map_err(|mut error| {
            error_data(&mut error)["createdSession"] = session;
            error
        })
}
pub(super) async fn schedule(call: &mut Calls<'_>, params: &Value) -> Outcome {
    let mut schedule = params["schedule"].clone();
    if !schedule.is_object() {
        return Err(invalid("schedule is required"));
    }
    text(&schedule, "text")?;
    if schedule["nextRunAtMs"].as_u64().is_none() {
        return Err(invalid("nextRunAtMs is required"));
    }
    if schedule.get("intervalSeconds").is_some_and(|v| {
        !v.is_null()
            && v.as_u64()
                .is_none_or(|seconds| !(1..=31_536_000).contains(&seconds))
    }) {
        return Err(invalid("intervalSeconds must be 1-31536000"));
    }
    if !call
        .extensions()
        .await?
        .iter()
        .any(|extension| extension["id"] == "resources")
    {
        return Err(invalid("enable the resources extension to save schedules"));
    }
    let created = schedule["sessionId"].is_null();
    let session = if created {
        super::sessions::launch(call, &params["launch"], false).await?
    } else {
        call.call(
            "session/get",
            json!({"sessionId":schedule["sessionId"]}),
            false,
        )
        .await?
    };
    schedule["sessionId"] = session["id"].clone();
    match call
        .extension("resources", "schedules.put", schedule, true)
        .await
    {
        Ok(result) => Ok(json!({"created":created,"session":session,"result":result})),
        Err(mut error) => {
            if created {
                error_data(&mut error)["createdSession"] = session;
            }
            Err(error)
        }
    }
}
