use super::*;
use std::collections::BTreeSet;
use swarmie_skills::{Catalog, Skill, canonical_roots};

pub(crate) fn home(options: &SpawnOptions) -> Option<PathBuf> {
    options
        .execution
        .env
        .get("HOME")
        .or_else(|| options.execution.env.get("USERPROFILE"))
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(PathBuf::from)
        })
}

pub(crate) fn shared(options: &SpawnOptions) -> Catalog {
    if options.execution.tools == ToolPolicy::Disabled {
        return Catalog::default();
    }
    Catalog::discover(&canonical_roots(&options.cwd, home(options).as_deref()))
}

/// Selected paths must be advertised by this native session, not merely exist on disk.
pub(crate) fn selected<'a>(prompt: &Prompt, commands: &'a Value) -> Result<Vec<&'a Value>> {
    let mut choices = Vec::new();
    let mut seen = BTreeSet::new();
    for content in &prompt.content {
        let PromptContent::Skill { name, path } = content else {
            continue;
        };
        if !std::path::Path::new(path).is_absolute() {
            return Err(Error::Rejected(
                "Selected skill requires an absolute path on this execution node".into(),
            ));
        }
        let canonical = std::fs::canonicalize(path).map_err(|_| {
            Error::Rejected("Selected skill no longer exists on this execution node".into())
        })?;
        if canonical.file_name().is_none_or(|name| name != "SKILL.md") || !canonical.is_file() {
            return Err(Error::Rejected(
                "Selected skill must reference SKILL.md".into(),
            ));
        }
        let mut matches: Vec<_> = commands
            .as_array()
            .into_iter()
            .flatten()
            .filter(|command| {
                command["source"] == "skill"
                    && command["name"] == name.as_str()
                    && command["enabled"] != false
                    && command["userInvocable"] != false
                    && (command["availability"].is_null()
                        || command["availability"] == "advertised")
                    && command["path"].as_str().is_some_and(|path| {
                        std::fs::canonicalize(path).ok().as_ref() == Some(&canonical)
                    })
            })
            .collect();
        if matches.len() > 1 {
            matches.retain(|command| command["path"].as_str() == Some(path.as_str()));
        }
        if matches.len() != 1 {
            return Err(Error::Rejected("Selected skill origin is not uniquely advertised by this native session; reload its catalog".into()));
        }
        if seen.insert(canonical) {
            choices.push(matches[0]);
        }
    }
    Ok(choices)
}

pub(crate) fn native_command(prompt: &Prompt, commands: &Value) -> Result<Option<String>> {
    let choices = selected(prompt, commands)?;
    if choices.is_empty() {
        return Ok(None);
    }
    if choices.len() != 1 {
        return Err(Error::Rejected(
            "This harness accepts one explicitly selected native skill per message".into(),
        ));
    }
    let command = choices[0]["nativeName"]
        .as_str()
        .or_else(|| choices[0]["name"].as_str())
        .filter(|name| {
            !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '-' | '_' | '.'))
        })
        .ok_or_else(|| Error::Rejected("Native skill has no safe invocation command".into()))?;
    Ok(Some(format!("/{command}")))
}

pub(crate) fn command_text(prompt: &Prompt, command: &str) -> String {
    let mut text = prompt
        .content
        .iter()
        .filter_map(|content| {
            if let PromptContent::Text(text) = content {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    // The picker can already insert the invocation. Avoid interpreting it as skill arguments.
    if let Some(rest) = text
        .strip_prefix(command)
        .filter(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
    {
        text = rest.trim_start().to_owned();
    }
    format!(
        "{command}{}{}",
        if text.is_empty() { "" } else { " " },
        text
    )
}

pub(crate) fn pi_catalog(result: &Value, disabled: bool) -> Value {
    let Some(commands) = result["commands"].as_array() else {
        return Value::Null;
    };
    json!(
        commands
            .iter()
            .filter_map(|entry| {
                let name = entry["name"]
                    .as_str()
                    .filter(|name| !name.trim().is_empty())?;
                let mut command = entry.clone();
                {
                    command["invocation"] = json!(format!("/{} ", name.trim_start_matches('/')));
                    if entry["source"] == "skill" {
                        command["nativeName"] = json!(name);
                        command["name"] = json!(name.strip_prefix("skill:").unwrap_or(name));
                        command["path"] = entry["sourceInfo"]["path"]
                            .as_str()
                            .or_else(|| entry["path"].as_str())
                            .map_or(Value::Null, |path| json!(path));
                        command["scope"] = entry["sourceInfo"]["scope"].clone();
                        command["enabled"] = json!(!disabled && entry["enabled"] != false);
                        command["availability"] = json!(if disabled {
                            "blocked-by-policy"
                        } else if entry["enabled"] == false {
                            "blocked-by-native"
                        } else if command["path"]
                            .as_str()
                            .is_some_and(|path| std::path::Path::new(path).is_absolute()
                                && std::fs::canonicalize(path).is_ok_and(|path| path.is_file()
                                    && path.file_name().is_some_and(|name| name == "SKILL.md")))
                        {
                            "advertised"
                        } else {
                            "origin-unverified"
                        });
                    }
                }
                Some(command)
            })
            .collect::<Vec<_>>()
    )
}

/// Owns links only. Canonical skill folders remain user-owned throughout launch and cleanup.
pub(crate) struct ClaudeSkills {
    overlay: std::sync::Mutex<Option<super::skill_overlay::Overlay>>,
    skills: Vec<Skill>,
}

impl ClaudeSkills {
    pub(crate) fn prepare(options: &SpawnOptions, args: &mut Vec<String>) -> Result<Self> {
        let skills: Vec<_> = shared(options)
            .skills
            .into_iter()
            .filter(Skill::usable)
            .collect();
        if skills.is_empty() {
            return Ok(Self {
                overlay: std::sync::Mutex::new(None),
                skills,
            });
        }
        if args.iter().any(|arg| {
            matches!(
                arg.as_str(),
                "--bare" | "--safe-mode" | "--disable-slash-commands"
            )
        }) {
            return Ok(Self {
                overlay: std::sync::Mutex::new(None),
                skills: vec![],
            });
        }
        #[cfg(not(unix))]
        return Err(Error::Unsupported(
            "Shared Claude skills require qualified directory symlink support".into(),
        ));
        #[cfg(unix)]
        {
            let links = skills
                .iter()
                .map(|skill| (skill.alias(), skill.directory.clone()))
                .collect::<Vec<_>>();
            let overlay = super::skill_overlay::Overlay::create(&links)?;
            args.extend([
                "--add-dir".into(),
                overlay.path().to_string_lossy().into_owned(),
            ]);
            Ok(Self {
                overlay: std::sync::Mutex::new(Some(overlay)),
                skills,
            })
        }
    }

    fn available(&self, skill: &Skill) -> bool {
        skill.unchanged()
            && self
                .overlay
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|overlay| overlay.intact())
    }

    pub(crate) fn catalog(&self, raw: &Value) -> Value {
        let Some(commands) = raw.as_array() else {
            return Value::Null;
        };
        json!(commands.iter().filter_map(|entry| {
            let native_name = entry["name"].as_str().or_else(|| entry.as_str()).filter(|name| !name.trim().is_empty())?;
            if let Some(skill) = self.skills.iter().find(|skill| skill.alias() == native_name || entry["aliases"].as_array().is_some_and(|aliases| aliases.iter().any(|alias| alias.as_str() == Some(skill.alias().as_str())))) {
                Some(json!({"name":skill.name,"description":skill.description,"path":skill.path,"source":"skill","scope":skill.scope,"nativeName":skill.alias(),"invocation":format!("/{} ", skill.alias()),"enabled":true,"userInvocable":skill.user_invocable && self.available(skill),"modelInvocable":skill.model_invocable,"availability":if self.available(skill){"advertised"}else{"requires-restart"},"metadata":entry}))
            } else {
                Some(json!({"name":native_name,"description":entry["description"],"source":"native","metadata":entry}))
            }
        }).collect::<Vec<_>>())
    }

    pub(crate) fn refresh(&self, commands: &Value) -> Value {
        let mut commands = commands.clone();
        for command in commands.as_array_mut().into_iter().flatten() {
            if let Some(skill) = self
                .skills
                .iter()
                .find(|skill| command["path"].as_str() == skill.path.to_str())
                && !self.available(skill)
            {
                command["availability"] = json!("requires-restart");
                command["userInvocable"] = json!(false);
            }
        }
        commands
    }

    pub(crate) fn bind_native(&self, pid: Option<u64>) -> Result<()> {
        if let Some(overlay) = self.overlay.lock().unwrap().as_mut() {
            overlay.bind_native(pid)?;
        }
        Ok(())
    }

    pub(crate) fn cleanup(&self) {
        self.overlay.lock().unwrap().take();
    }
}
