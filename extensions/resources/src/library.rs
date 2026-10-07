use crate::settings::cwd;
use crate::*;
use std::fs;

impl ResourcesExtension {
    pub(crate) async fn library_command(&self, command: &str, p: &Value) -> Result<Value> {
        match command {
            "templates.list" => Ok(
                json!({"templates":self.0.state.lock().await.templates.values().collect::<Vec<_>>()}),
            ),
            "templates.put" => {
                let id = text(p, "id")?;
                identifier(id)?;
                let aliases: Vec<String> = p
                    .get("aliases")
                    .map(|value| serde_json::from_value(value.clone()))
                    .transpose()?
                    .unwrap_or_default();
                if aliases.len() > 16 {
                    return Err(Error::Invalid(
                        "at most 16 template aliases are allowed".into(),
                    ));
                }
                let mut seen = std::collections::BTreeSet::new();
                for alias in &aliases {
                    identifier(alias)?;
                    if alias == id || !seen.insert(alias) {
                        return Err(Error::Invalid(
                            "template aliases must be unique and differ from its id".into(),
                        ));
                    }
                }
                let mut template = Template {
                    id: id.into(),
                    aliases,
                    name: text(p, "name")?.into(),
                    text: text(p, "text")?.into(),
                };
                if template.name.len() > 256 {
                    return Err(Error::Invalid(
                        "template name exceeds 256 characters".into(),
                    ));
                }
                let mut state = self.0.state.lock().await;
                if p.get("aliases").is_none()
                    && let Some(existing) = state.templates.get(id)
                {
                    template.aliases = existing.aliases.clone();
                }
                for other in state.templates.values().filter(|other| other.id != id) {
                    if other.aliases.iter().any(|alias| alias == id)
                        || template
                            .aliases
                            .iter()
                            .any(|alias| alias == &other.id || other.aliases.contains(alias))
                    {
                        return Err(Error::Invalid(
                            "template id or alias is already in use".into(),
                        ));
                    }
                }
                capacity(state.templates.contains_key(id), state.templates.len())?;
                state.templates.insert(id.into(), template.clone());
                self.persist(&state)?;
                Ok(serde_json::to_value(template)?)
            }
            "templates.remove" => {
                let mut state = self.0.state.lock().await;
                state.templates.remove(text(p, "id")?);
                self.persist(&state)?;
                Ok(json!({"removed":true}))
            }
            "templates.render" => {
                let id = text(p, "id")?;
                let template = self
                    .0
                    .state
                    .lock()
                    .await
                    .templates
                    .values()
                    .find(|template| {
                        template.id == id || template.aliases.iter().any(|alias| alias == id)
                    })
                    .cloned()
                    .ok_or_else(|| Error::Invalid("unknown template".into()))?;
                let vars = p["variables"]
                    .as_object()
                    .ok_or_else(|| Error::Invalid("variables must be an object".into()))?;
                let mut rendered = String::new();
                let mut rest = template.text.as_str();
                while let Some(start) = rest.find("{{") {
                    rendered.push_str(&rest[..start]);
                    let tail = &rest[start + 2..];
                    let end = tail
                        .find("}}")
                        .ok_or_else(|| Error::Invalid("unclosed template placeholder".into()))?;
                    let key = tail[..end].trim();
                    identifier(key)?;
                    let value = vars
                        .get(key)
                        .and_then(Value::as_str)
                        .ok_or_else(|| Error::Invalid(format!("missing string variable {key}")))?;
                    rendered.push_str(value);
                    if rendered.len() > MAX_TEXT {
                        return Err(Error::Invalid("rendered template exceeds 64 KiB".into()));
                    }
                    rest = &tail[end + 2..];
                }
                rendered.push_str(rest);
                if rendered.len() > MAX_TEXT {
                    return Err(Error::Invalid("rendered template exceeds 64 KiB".into()));
                }
                Ok(json!({"id":template.id,"text":rendered}))
            }
            "resources.list" => self.discover(p).await,
            "resources.read" => {
                let cwd = cwd(p)?;
                let path = fs::canonicalize(text(p, "path")?)?;
                let allowed = self
                    .0
                    .discovered
                    .lock()
                    .await
                    .get(&cwd.to_string_lossy().into_owned())
                    .is_some_and(|paths| paths.contains(&path));
                if !allowed {
                    return Err(Error::Invalid(
                        "resource must be discovered for this project before reading".into(),
                    ));
                }
                let metadata = fs::symlink_metadata(text(p, "path")?)?;
                if !metadata.is_file()
                    || metadata.file_type().is_symlink()
                    || metadata.len() > MAX_TEXT as u64
                {
                    return Err(Error::Invalid(
                        "resource must be a regular UTF-8 file no larger than 64 KiB".into(),
                    ));
                }
                Ok(json!({"path":path,"text":fs::read_to_string(path)?}))
            }
            "history.list" => {
                self.backfill().await?;
                let path = if p.get("cwd").is_some() {
                    Some(cwd(p)?)
                } else {
                    None
                };
                let query = p["query"].as_str().unwrap_or("").to_lowercase();
                let limit = p["limit"].as_u64().unwrap_or(100);
                if limit == 0 || limit > 500 {
                    return Err(Error::Invalid("history limit must be 1-500".into()));
                }
                let state = self.0.state.lock().await;
                Ok(
                    json!({"items":state.history.iter().rev().filter(|v|path.as_ref().is_none_or(|path|&v.cwd==path)&&p["sessionId"].as_str().is_none_or(|id|v.session_id==id)&&v.text.to_lowercase().contains(&query)).take(limit as usize).collect::<Vec<_>>(),"truncated":state.history_truncated}),
                )
            }
            _ => Err(Error::Unsupported(command.into())),
        }
    }
    async fn discover(&self, p: &Value) -> Result<Value> {
        let project = cwd(p)?;
        let mut roots = vec![
            (project.join(".agents/skills"), "skill", "project"),
            (project.join(".pi/skills"), "skill", "project"),
            (project.join(".pi/prompts"), "template", "project"),
            (project.join(".claude/skills"), "skill", "project"),
            (project.join(".claude/commands"), "command", "project"),
        ];
        let mut commands = Value::Null;
        let mut limitations=vec!["Files discovered in native convention directories are not proof that a running harness loaded them".to_owned()];
        let mut config_home = None;
        let mut harness = p["harness"].as_str().map(str::to_owned);
        if let Some(id) = p["sessionId"].as_str() {
            let runtime = self.runtime()?;
            let snapshot = runtime.snapshot(id).await?;
            if snapshot.options.cwd != project {
                return Err(Error::Invalid("session belongs to another project".into()));
            }
            harness = Some(snapshot.options.harness);
            config_home = snapshot.options.execution.config_home;
            if let Some(agent) = runtime.agent(id).await {
                match agent.slash_commands().await {
                    Ok(value) => commands = value,
                    Err(error) => limitations.push(format!("Native command catalog: {error}")),
                }
            } else {
                limitations.push("Native command catalog requires a live session".into());
            }
        }
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            roots.push((home.join(".agents/skills"), "skill", "user"));
            match harness.as_deref() {
                Some("claude") => {
                    let root = config_home.unwrap_or(home.join(".claude"));
                    roots.push((root.join("skills"), "skill", "user"));
                    roots.push((root.join("commands"), "command", "user"));
                }
                Some("pi") => {
                    let root = config_home.unwrap_or(home.join(".pi/agent"));
                    roots.push((root.join("skills"), "skill", "user"));
                    roots.push((root.join("prompts"), "template", "user"));
                }
                Some("codex") => roots.push((
                    config_home.unwrap_or(home.join(".codex")).join("skills"),
                    "skill",
                    "user",
                )),
                _ => {}
            }
        }
        let mut entries = Vec::new();
        let mut paths = Vec::new();
        let mut capped = false;
        for (root, kind, source) in roots {
            let mut files = Vec::new();
            walk(&root, 0, &mut files, &mut capped)?;
            for path in files {
                if kind == "skill" && path.file_name().is_none_or(|n| n != "SKILL.md") {
                    continue;
                }
                if paths.contains(&path) {
                    continue;
                }
                let name = if kind == "skill" {
                    path.parent().and_then(Path::file_name)
                } else {
                    path.file_stem()
                }
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
                entries.push(json!({"id":path.to_string_lossy(),"name":name,"kind":kind,"path":path,"source":source,"native":true}));
                paths.push(path);
                if entries.len() >= 500 {
                    capped = true;
                    break;
                }
            }
            if capped {
                break;
            }
        }
        for name in ["AGENTS.md", "CLAUDE.md"] {
            let file = project.join(name);
            if file.is_file() && !fs::symlink_metadata(&file)?.file_type().is_symlink() {
                let file = fs::canonicalize(file)?;
                entries.push(json!({"id":file,"name":name,"kind":"instruction","path":file,"source":"project","native":true}));
                paths.push(file);
            }
        }
        self.0
            .discovered
            .lock()
            .await
            .insert(project.to_string_lossy().into_owned(), paths);
        if capped {
            limitations.push("Discovery stopped at 500 files or directory depth 6".into());
        }
        Ok(
            json!({"resources":entries,"commands":commands,"limitations":limitations,"truncated":capped}),
        )
    }
}
fn walk(path: &Path, depth: usize, files: &mut Vec<PathBuf>, capped: &mut bool) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if depth > 6 || files.len() >= 500 {
        *capped = true;
        return Ok(());
    }
    if metadata.is_file() {
        if path.extension().is_some_and(|e| e == "md") {
            files.push(fs::canonicalize(path)?);
        }
        return Ok(());
    }
    if !metadata.is_dir() {
        return Ok(());
    }
    let mut children = fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?;
    children.sort_by_key(|entry| entry.file_name());
    for child in children {
        walk(&child.path(), depth + 1, files, capped)?;
        if files.len() >= 500 {
            break;
        }
    }
    Ok(())
}
