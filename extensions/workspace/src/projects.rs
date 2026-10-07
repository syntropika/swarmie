use crate::{WorkspaceExtension, command, required};
use mpai::{Error, Result};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
pub async fn invoke(
    extension: &WorkspaceExtension,
    command_name: &str,
    p: &Value,
) -> Result<Value> {
    if command_name == "project.publish" {
        let root = extension.project(p).await?;
        let repo = required(p, "name")?;
        if repo.starts_with('-') || repo.contains(char::is_whitespace) || repo.len() > 200 {
            return Err(Error::Invalid("provide a literal repository name".into()));
        }
        let visibility = match p["visibility"].as_str().unwrap_or("private") {
            "private" => "--private",
            "public" => "--public",
            _ => {
                return Err(Error::Invalid(
                    "visibility must be private or public".into(),
                ));
            }
        };
        let mut args = vec![
            "repo".into(),
            "create".into(),
            repo.into(),
            visibility.into(),
            "--source".into(),
            root.to_string_lossy().into(),
            "--remote".into(),
            "origin".into(),
        ];
        if p["push"].as_bool().unwrap_or(true) {
            args.push("--push".into());
        }
        if let Some(description) = p["description"].as_str() {
            args.extend(["--description".into(), description.into()]);
        }
        let text = command::run(&extension.github_executable, &root, &args).await?;
        return Ok(json!({"published":true,"url":text.trim()}));
    }
    let parent = if command_name == "project.scratch" {
        let parent = extension.storage_root.join("projects");
        tokio::fs::create_dir_all(&parent).await?;
        parent
    } else {
        tokio::fs::canonicalize(required(p, "parent")?).await?
    };
    let name = if command_name == "project.scratch" {
        format!("scratch-{}", uuid::Uuid::new_v4())
    } else {
        required(p, "name")?.into()
    };
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', '\0'])
        || name.contains(char::is_control)
    {
        return Err(Error::Invalid("name must be one folder name".into()));
    }
    let path = parent.join(&name);
    tokio::fs::create_dir(&path).await?;
    // A failed clone retains the owned folder for inspection; existing folders are never replaced.
    let result = if command_name == "project.clone" {
        let url = required(p, "url")?;
        if url.starts_with('-') || url.contains(['\0', '\n', '\r']) {
            return Err(Error::Invalid("invalid clone URL".into()));
        }
        command::run(
            Path::new("git"),
            &parent,
            &[
                "clone".into(),
                "--".into(),
                url.into(),
                path.to_string_lossy().into(),
            ],
        )
        .await
        .map(|_| ())
    } else if p["initializeGit"] == true {
        command::run(
            Path::new("git"),
            &path,
            &command::args(&["init", "-b", "main"]),
        )
        .await
        .map(|_| ())
    } else {
        Ok(())
    };
    result.map_err(|error| {
        Error::Provider(format!(
            "Project folder remains at {}: {error}",
            path.display()
        ))
    })?;
    let path = tokio::fs::canonicalize(path).await?;
    let id = path.to_string_lossy().to_string();
    extension
        .projects
        .write()
        .await
        .insert(id.clone(), path.clone());
    Ok(json!({"id":id,"path":path,"scratch":name.starts_with("scratch-")}))
}
pub fn storage_root() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .unwrap_or_else(std::env::temp_dir)
        .join("mpai/workspace")
}
