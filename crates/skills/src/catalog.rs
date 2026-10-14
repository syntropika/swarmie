use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
};

const MAX_SKILLS: usize = 500;
const MAX_DIRECTORIES: usize = 4096;
const MAX_INSTRUCTIONS: u64 = 64 * 1024;

#[derive(Clone, Debug)]
pub struct Root {
    pub path: PathBuf,
    pub scope: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct Origin {
    pub path: PathBuf,
    pub scope: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    pub name: String,
    pub description: Option<String>,
    pub path: PathBuf,
    pub directory: PathBuf,
    pub scope: String,
    pub origins: Vec<Origin>,
    pub instruction_digest: String,
    pub user_invocable: bool,
    pub model_invocable: bool,
    pub diagnostics: Vec<String>,
    pub frontmatter: serde_json::Value,
    pub conflicts: Vec<PathBuf>,
}

impl Skill {
    pub fn usable(&self) -> bool {
        self.diagnostics.is_empty()
    }

    /// A deterministic, origin-specific directory command; it never renames user files.
    pub fn alias(&self) -> String {
        let digest = Sha256::digest(self.path.to_string_lossy().as_bytes());
        let name: String = self
            .name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .take(32)
            .collect();
        format!("swarmie-{}-{:x}", name, digest)[..("swarmie-".len() + name.len() + 1 + 12)]
            .to_owned()
    }

    pub fn unchanged(&self) -> bool {
        fs::canonicalize(&self.path).ok().as_ref() == Some(&self.path)
            && read_instructions(&self.path).is_ok_and(|bytes| {
                format!("{:x}", Sha256::digest(bytes)) == self.instruction_digest
            })
    }
}

#[derive(Default, Debug)]
pub struct Catalog {
    pub skills: Vec<Skill>,
    pub diagnostics: Vec<String>,
    pub truncated: bool,
}

/// Roots are ordered by catalog preference. Native precedence is reported separately.
pub fn canonical_roots(cwd: &Path, home: Option<&Path>) -> Vec<Root> {
    let repository = cwd
        .ancestors()
        .find(|directory| directory.join(".git").exists());
    let mut roots = Vec::new();
    for directory in cwd.ancestors() {
        roots.push(Root {
            path: directory.join(".agents/skills"),
            scope: "project",
        });
        if repository.is_none() || Some(directory) == repository {
            break;
        }
    }
    if let Some(home) = home {
        roots.push(Root {
            path: home.join(".agents/skills"),
            scope: "user",
        });
    }
    roots
}

impl Catalog {
    pub fn discover(roots: &[Root]) -> Self {
        let mut catalog = Self::default();
        for root in roots {
            let mut visited = BTreeSet::new();
            catalog.walk(&root.path, root.scope, 0, &mut visited);
        }
        for index in 0..catalog.skills.len() {
            let conflicts = catalog
                .skills
                .iter()
                .filter(|other| {
                    other.name == catalog.skills[index].name
                        && other.path != catalog.skills[index].path
                })
                .map(|skill| skill.path.clone())
                .collect();
            catalog.skills[index].conflicts = conflicts;
        }
        catalog
    }

    fn walk(&mut self, path: &Path, scope: &str, depth: usize, visited: &mut BTreeSet<PathBuf>) {
        let directory = match fs::canonicalize(path) {
            Ok(path) => path,
            Err(error) => {
                if error.kind() != io::ErrorKind::NotFound || fs::symlink_metadata(path).is_ok() {
                    self.diagnostics.push(format!(
                        "Cannot resolve skill directory {}: {error}",
                        path.display()
                    ));
                }
                return;
            }
        };
        if !directory.is_dir() {
            return;
        }
        if directory.ancestors().any(|ancestor| {
            let marker = ancestor.join(".swarmie-skill-overlay.json");
            fs::metadata(&marker)
                .ok()
                .filter(|metadata| metadata.is_file() && metadata.len() < 256 * 1024)
                .and_then(|_| fs::read(&marker).ok())
                .filter(|bytes| bytes.len() < 256 * 1024)
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                .is_some_and(|manifest| {
                    manifest["format"] == "swarmie-skill-overlay-v1"
                        && manifest["root"].as_str() == ancestor.to_str()
                })
        }) {
            return;
        }
        if !visited.insert(directory.clone()) {
            if let Some(skill) = self
                .skills
                .iter_mut()
                .find(|skill| skill.directory == directory)
                && !skill
                    .origins
                    .iter()
                    .any(|origin| origin.path == path && origin.scope == scope)
            {
                skill.origins.push(Origin {
                    path: path.to_owned(),
                    scope: scope.into(),
                });
            }
            return;
        }
        if depth > 6 || visited.len() > MAX_DIRECTORIES || self.skills.len() >= MAX_SKILLS {
            self.truncated = true;
            return;
        }
        let instructions = directory.join("SKILL.md");
        if fs::symlink_metadata(&instructions).is_ok() {
            match read_skill(&directory, &instructions, path, scope) {
                Ok(skill) => {
                    if let Some(existing) = self
                        .skills
                        .iter_mut()
                        .find(|existing| existing.path == skill.path)
                    {
                        existing.origins.extend(skill.origins);
                    } else {
                        self.skills.push(skill);
                    }
                }
                Err(error) => self.diagnostics.push(format!(
                    "Cannot read skill {}: {error}",
                    instructions.display()
                )),
            }
            // Supporting files belong to this skill; never discover them as separate skills.
            return;
        }
        let entries =
            match fs::read_dir(path).and_then(|entries| entries.collect::<io::Result<Vec<_>>>()) {
                Ok(mut entries) => {
                    entries.sort_by_key(|entry| entry.file_name());
                    entries
                }
                Err(error) => {
                    self.diagnostics.push(format!(
                        "Cannot scan skill directory {}: {error}",
                        path.display()
                    ));
                    return;
                }
            };
        for entry in entries {
            self.walk(&entry.path(), scope, depth + 1, visited);
            if self.truncated {
                break;
            }
        }
    }
}

fn read_skill(
    directory: &Path,
    instructions: &Path,
    origin: &Path,
    scope: &str,
) -> io::Result<Skill> {
    if instructions.to_str().is_none() || origin.to_str().is_none() {
        return Err(io::Error::other("Skill paths must be valid UTF-8"));
    }
    let metadata = fs::symlink_metadata(instructions)?;
    if !metadata.is_file() || metadata.len() > MAX_INSTRUCTIONS {
        return Err(io::Error::other(
            "SKILL.md must be a regular file no larger than 64 KiB",
        ));
    }
    let bytes = read_instructions(instructions)?;
    let text = std::str::from_utf8(&bytes).map_err(io::Error::other)?;
    let mut diagnostics = Vec::new();
    let frontmatter = parse_metadata(text).unwrap_or_else(|error| {
        diagnostics.push(error);
        serde_json::json!({})
    });
    let folder_name = directory
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("skill");
    let name = frontmatter["name"]
        .as_str()
        .unwrap_or(folder_name)
        .to_owned();
    if name.is_empty()
        || name.len() > 128
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        diagnostics
            .push("Skill name must contain only letters, numbers, hyphens or underscores".into());
    }
    let description = frontmatter["description"]
        .as_str()
        .filter(|description| !description.trim().is_empty())
        .map(str::to_owned);
    if description.is_none() {
        diagnostics.push("Skill description is missing".into());
    }
    if directory.join(".claude-plugin").exists() || directory.join(".claude").exists() {
        diagnostics.push("Bundled Claude customization requires a separate integration".into());
    }
    Ok(Skill {
        name,
        description,
        path: instructions.to_owned(),
        directory: directory.to_owned(),
        scope: scope.into(),
        origins: vec![Origin {
            path: origin.to_owned(),
            scope: scope.into(),
        }],
        instruction_digest: format!("{:x}", Sha256::digest(&bytes)),
        user_invocable: !is_false(&frontmatter["user-invocable"]),
        model_invocable: !is_true(&frontmatter["disable-model-invocation"]),
        diagnostics,
        frontmatter,
        conflicts: Vec::new(),
    })
}

fn read_instructions(path: &Path) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_INSTRUCTIONS + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_INSTRUCTIONS {
        return Err(io::Error::other("Skill instructions exceed 64 KiB"));
    }
    Ok(bytes)
}

fn parse_metadata(text: &str) -> Result<serde_json::Value, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.lines();
    if lines.next() != Some("---") {
        return Err("Skill YAML frontmatter is missing".into());
    }
    let mut yaml = String::new();
    let mut closed = false;
    for line in lines {
        if line == "---" {
            closed = true;
            break;
        }
        yaml.push_str(line);
        yaml.push('\n');
    }
    if !closed {
        return Err("Skill YAML frontmatter is not closed".into());
    }
    let metadata: serde_json::Value =
        serde_yaml_ng::from_str(&yaml).map_err(|error| format!("Invalid skill YAML: {error}"))?;
    if !metadata.is_object() {
        return Err("Skill frontmatter must be a mapping".into());
    }
    Ok(metadata)
}

fn is_true(value: &serde_json::Value) -> bool {
    value == true
        || value == 1
        || value.as_str().is_some_and(|value| {
            matches!(value.to_ascii_lowercase().as_str(), "true" | "yes" | "on")
        })
}
fn is_false(value: &serde_json::Value) -> bool {
    value == false
        || value == 0
        || value.as_str().is_some_and(|value| {
            matches!(value.to_ascii_lowercase().as_str(), "false" | "no" | "off")
        })
}
