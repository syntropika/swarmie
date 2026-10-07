//! Optional GitHub CLI operations. Pull requests are never merged by this extension.
use crate::required;
use mpai::{Error, Result};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command};

pub async fn run(executable: &Path, cwd: &Path, args: &[String]) -> Result<String> {
    let mut child = Command::new(executable)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| {
            Error::Provider(format!(
                "GitHub CLI is unavailable: {error}; install gh and run gh auth login"
            ))
        })?;
    let mut stdout = child.stdout.take().unwrap().take(4 * 1024 * 1024 + 1);
    let mut stderr = child.stderr.take().unwrap().take(1024 * 1024 + 1);
    let result = tokio::time::timeout(Duration::from_secs(90), async {
        let mut out = vec![];
        let mut err = vec![];
        let (a, b) = tokio::join!(stdout.read_to_end(&mut out), stderr.read_to_end(&mut err));
        a?;
        b?;
        if out.len() > 4 * 1024 * 1024 || err.len() > 1024 * 1024 {
            return Err(Error::Invalid(
                "GitHub CLI output exceeded its limit".into(),
            ));
        }
        let status = child.wait().await?;
        let check_result = args.first().is_some_and(|s| s == "pr")
            && args.get(1).is_some_and(|s| s == "checks")
            && serde_json::from_slice::<Value>(&out).is_ok();
        if !status.success() && !check_result {
            return Err(Error::Provider(format!(
                "GitHub CLI: {}",
                String::from_utf8_lossy(&err).trim()
            )));
        }
        String::from_utf8(out).map_err(|_| Error::Provider("GitHub CLI output is not UTF-8".into()))
    })
    .await
    .map_err(|_| Error::Timeout("GitHub CLI".into()))?;
    if result.is_err() {
        let _ = child.kill().await;
    }
    result
}
fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| (*v).into()).collect()
}
fn selector(p: &Value) -> Result<Option<String>> {
    match p.get("number") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .filter(|n| *n > 0)
            .map(|n| Some(n.to_string()))
            .ok_or_else(|| Error::Invalid("number must be a positive pull request number".into())),
    }
}
async fn body_file(body: &str) -> Result<PathBuf> {
    if body.len() > 1024 * 1024 {
        return Err(Error::Invalid("body exceeds 1 MiB".into()));
    }
    let path = std::env::temp_dir().join(format!("mpai-github-{}.txt", uuid::Uuid::new_v4()));
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    use tokio::io::AsyncWriteExt;
    let mut file = options.open(&path).await?;
    file.write_all(body.as_bytes()).await?;
    file.flush().await?;
    Ok(path)
}
pub async fn invoke(executable: &Path, cwd: &Path, command: &str, p: &Value) -> Result<Value> {
    if command == "integration.status" {
        let version = run(executable, cwd, &strings(&["--version"])).await;
        return Ok(match version {
            Ok(version) => {
                json!({"available":true,"version":version.lines().next(),"authenticated":run(executable,cwd,&strings(&["auth","status"])).await.is_ok()})
            }
            Err(error) => {
                json!({"available":false,"authenticated":false,"message":error.to_string()})
            }
        });
    }
    let number = selector(p)?;
    let mut args = match command {
        "pr.status" => strings(&["pr", "view"]),
        "pr.checks" => strings(&["pr", "checks"]),
        "pr.comments" => strings(&["pr", "view"]),
        "pr.create" => strings(&["pr", "create", "--draft", "--title", required(p, "title")?]),
        "pr.comment" => strings(&["pr", "comment"]),
        "pr.review" => strings(&["pr", "review"]),
        "pr.viewed.list" => return viewed_list(executable, cwd, p, number).await,
        "pr.viewed" => return viewed(executable, cwd, p, number).await,
        _ => return Err(Error::Unsupported(format!("GitHub command {command}"))),
    };
    if let Some(number) = number {
        if command == "pr.create" {
            return Err(Error::Invalid(
                "new pull requests do not accept number".into(),
            ));
        }
        args.push(number);
    }
    match command {
        "pr.status"=>args.extend(strings(&["--json","number,url,title,body,state,isDraft,headRefName,baseRefName,reviewDecision,statusCheckRollup,files"])),
        "pr.comments"=>args.extend(strings(&["--json","comments,reviews"])),
        "pr.checks"=>args.extend(strings(&["--json","name,state,bucket,link,workflow"])),
        "pr.create"=>{for key in ["base","head"] {if let Some(value)=p[key].as_str() {if value.starts_with('-')||value.is_empty(){return Err(Error::Invalid(format!("invalid {key}")));}args.extend([format!("--{key}"),value.into()]);}}},
        "pr.review"=>args.push(match required(p,"event")? {"approve"=>"--approve","request_changes"=>"--request-changes","comment"=>"--comment",_=>return Err(Error::Invalid("review event must be approve, request_changes or comment".into()))}.into()),
        _=>(),
    }
    let file = if matches!(command, "pr.create" | "pr.comment" | "pr.review") {
        let body = if command == "pr.review" && p["event"] == "approve" {
            match p.get("body") {
                None => "",
                Some(value) => value
                    .as_str()
                    .ok_or_else(|| Error::Invalid("body must be a string".into()))?,
            }
        } else {
            required(p, "body")?
        };
        let path = body_file(body).await?;
        args.extend(["--body-file".into(), path.to_string_lossy().into()]);
        Some(path)
    } else {
        None
    };
    let result = run(executable, cwd, &args).await;
    if let Some(path) = file {
        let _ = tokio::fs::remove_file(path).await;
    }
    let text = match result {
        Err(error)
            if command == "pr.checks" && error.to_string().contains("unknown flag: --json") =>
        {
            return legacy_checks(executable, cwd, p).await;
        }
        result => result?,
    };
    if matches!(command, "pr.status" | "pr.comments" | "pr.checks") {
        Ok(serde_json::from_str(&text)?)
    } else {
        Ok(json!({"updated":true,"url":text.trim(),"draft":command=="pr.create"}))
    }
}
async fn legacy_checks(executable: &Path, cwd: &Path, p: &Value) -> Result<Value> {
    let mut args = strings(&["pr", "view"]);
    if let Some(number) = selector(p)? {
        args.push(number);
    }
    args.extend(strings(&["--json", "statusCheckRollup"]));
    let value: Value = serde_json::from_str(&run(executable, cwd, &args).await?)?;
    let checks = value["statusCheckRollup"]
        .as_array()
        .ok_or_else(|| Error::Provider("GitHub omitted check status data".into()))?;
    Ok(Value::Array(checks.iter().map(|check| {
        let state = check["conclusion"].as_str().filter(|value| !value.is_empty())
            .or(check["state"].as_str()).or(check["status"].as_str()).unwrap_or("UNKNOWN");
        let bucket = match state {
            "SUCCESS" | "NEUTRAL" => "pass",
            "SKIPPED" => "skipping",
            "CANCELLED" => "cancel",
            "FAILURE" | "ERROR" | "TIMED_OUT" | "ACTION_REQUIRED" | "STARTUP_FAILURE" => "fail",
            _ => "pending",
        };
        json!({"name":check["name"].as_str().or(check["context"].as_str()).unwrap_or("Check"),"state":state,"bucket":bucket,"link":check["detailsUrl"].as_str().or(check["targetUrl"].as_str()),"workflow":check["workflowName"]})
    }).collect()))
}
async fn viewed(executable: &Path, cwd: &Path, p: &Value, number: Option<String>) -> Result<Value> {
    let mut args = strings(&["pr", "view"]);
    if let Some(number) = number {
        args.push(number);
    }
    args.extend(strings(&["--json", "id"]));
    let info: Value = serde_json::from_str(&run(executable, cwd, &args).await?)?;
    let id = info["id"]
        .as_str()
        .ok_or_else(|| Error::Provider("GitHub omitted pull request node id".into()))?;
    let path = required(p, "path")?;
    if Path::new(path).is_absolute()
        || Path::new(path)
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(Error::Invalid(
            "viewed file must be a literal relative path".into(),
        ));
    }
    let action = if p["viewed"].as_bool().unwrap_or(true) {
        "markFileAsViewed"
    } else {
        "unmarkFileAsViewed"
    };
    let query = format!(
        "mutation($id:ID!,$path:String!){{{action}(input:{{pullRequestId:$id,path:$path}}){{clientMutationId}}}}"
    );
    let args = vec![
        "api".into(),
        "graphql".into(),
        "-f".into(),
        format!("query={query}"),
        "-f".into(),
        format!("id={id}"),
        "-f".into(),
        format!("path={path}"),
    ];
    let result: Value = serde_json::from_str(&run(executable, cwd, &args).await?)?;
    if !result["errors"].is_null() {
        return Err(Error::Provider(
            "GitHub rejected the viewed-file update".into(),
        ));
    }
    Ok(json!({"updated":true,"path":path,"viewed":action=="markFileAsViewed"}))
}
async fn viewed_list(
    executable: &Path,
    cwd: &Path,
    _p: &Value,
    number: Option<String>,
) -> Result<Value> {
    let mut args = strings(&["pr", "view"]);
    if let Some(number) = number {
        args.push(number);
    }
    args.extend(strings(&["--json", "id"]));
    let value: Value = serde_json::from_str(&run(executable, cwd, &args).await?)?;
    let id = value["id"]
        .as_str()
        .ok_or_else(|| Error::Provider("GitHub omitted pull request node id".into()))?;
    let query = "query($id:ID!,$cursor:String){node(id:$id){... on PullRequest{files(first:100,after:$cursor){nodes{path viewerViewedState}pageInfo{hasNextPage endCursor}}}}}";
    let mut cursor = None;
    let mut files = vec![];
    for _ in 0..8 {
        let mut args = vec![
            "api".into(),
            "graphql".into(),
            "-f".into(),
            format!("query={query}"),
            "-f".into(),
            format!("id={id}"),
        ];
        if let Some(cursor) = cursor.take() {
            args.extend(["-f".into(), format!("cursor={cursor}")]);
        }
        let value: Value = serde_json::from_str(&run(executable, cwd, &args).await?)?;
        if !value["errors"].is_null() {
            return Err(Error::Provider(
                "GitHub rejected the viewed-file query".into(),
            ));
        }
        let page = &value["data"]["node"]["files"];
        files.extend(
            page["nodes"]
                .as_array()
                .ok_or_else(|| Error::Provider("GitHub omitted viewed file data".into()))?
                .clone(),
        );
        if page["pageInfo"]["hasNextPage"] != true {
            return Ok(json!({"files":files,"truncated":false}));
        }
        cursor = page["pageInfo"]["endCursor"].as_str().map(String::from);
        if cursor.is_none() {
            return Err(Error::Provider("GitHub omitted pagination cursor".into()));
        }
    }
    Ok(json!({"files":files,"truncated":true}))
}
