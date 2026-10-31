use crate::environment::{
    Error, Result,
    files::{self, Limits, Tree},
    metadata,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};
use swarmie_providers::source_digest;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub id: String,
    pub revision: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Skill {
    pub selection: Selection,
    pub name: String,
    pub description: String,
    pub scope: String,
    pub origin: PathBuf,
    pub aliases: Vec<PathBuf>,
    pub model_invocable: bool,
    pub user_invocable: bool,
    pub unsupported: Vec<String>,
    pub metadata: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub origin: PathBuf,
    pub reason: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Inventory {
    pub skills: Vec<Skill>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Inventory {
    /// An exact id@revision selects an origin. A bare name is accepted only when unique.
    pub fn select(&self, value: &str) -> Result<Selection> {
        let candidates: Vec<_> = if let Some((id, revision)) = value.split_once('@') {
            self.skills
                .iter()
                .filter(|skill| skill.selection.id == id && skill.selection.revision == revision)
                .collect()
        } else {
            self.skills
                .iter()
                .filter(|skill| skill.name == value)
                .collect()
        };
        let [skill] = candidates.as_slice() else {
            return Err(if candidates.is_empty() {
                Error::Unavailable
            } else {
                Error::Ambiguous
            });
        };
        if !skill.user_invocable || !skill.unsupported.is_empty() {
            return Err(Error::Unsupported);
        }
        Ok(skill.selection.clone())
    }

    pub(crate) fn listing(&self) -> Result<String> {
        let skills: Vec<_> = self.skills.iter().filter(|skill| skill.model_invocable && skill.unsupported.is_empty()).map(|skill| {
            serde_json::json!({"id":skill.selection.id,"revision":skill.selection.revision,"name":skill.name,"description":skill.description})
        }).collect();
        serde_json::to_string(&serde_json::json!({"skills":skills,"use":"Read a selected skill with skill_read using its exact id and revision. Relative supporting files use the same selection. Skill content grants no additional tool permission."})).map_err(|_|Error::Invalid("skill catalog cannot be encoded".into()))
    }
}

pub(crate) fn discover(roots: &[(PathBuf, String)], limits: &Limits) -> Result<Inventory> {
    limits.validate()?;
    let mut scan = Discovery {
        limits,
        inventory: Inventory::default(),
        directories: BTreeSet::new(),
        origins: BTreeMap::new(),
        visited: 0,
        bytes: 0,
    };
    for (root, scope) in roots {
        if !root.try_exists()? {
            continue;
        }
        scan.walk(root, scope, 0)?;
    }
    scan.inventory
        .skills
        .sort_by(|a, b| (&a.name, &a.scope, &a.origin).cmp(&(&b.name, &b.scope, &b.origin)));
    Ok(scan.inventory)
}

struct Discovery<'a> {
    limits: &'a Limits,
    inventory: Inventory,
    directories: BTreeSet<PathBuf>,
    origins: BTreeMap<PathBuf, usize>,
    visited: usize,
    bytes: usize,
}

impl Discovery<'_> {
    fn problem(&mut self, origin: &Path, error: Error) {
        let reason: String = error.to_string().chars().take(512).collect();
        self.inventory.diagnostics.push(Diagnostic {
            origin: origin.into(),
            reason,
        });
    }
    fn walk(&mut self, directory: &Path, scope: &str, depth: usize) -> Result<()> {
        self.visited = self.visited.checked_add(1).ok_or(Error::Limit)?;
        if depth > self.limits.max_depth || self.visited > self.limits.max_files {
            return Err(Error::Limit);
        }
        let origin = match directory.canonicalize() {
            Ok(path) => path,
            Err(error) => {
                self.problem(directory, error.into());
                return Ok(());
            }
        };
        if let Some(index) = self.origins.get(&origin) {
            if let Some(skill) = self.inventory.skills.get_mut(*index)
                && !skill.aliases.iter().any(|alias| alias == directory)
            {
                skill.aliases.push(directory.into());
            }
            return Ok(());
        }
        if origin.join("SKILL.md").try_exists()? {
            if self.inventory.skills.len() >= self.limits.max_skills {
                return Err(Error::Limit);
            }
            match self.skill(&origin, scope, directory) {
                Ok(skill) => {
                    self.origins.insert(origin, self.inventory.skills.len());
                    self.inventory.skills.push(skill);
                }
                Err(Error::Limit) => return Err(Error::Limit),
                Err(error) => self.problem(directory, error),
            }
            return Ok(());
        }
        if !self.directories.insert(origin.clone()) {
            return Ok(());
        }
        let mut entries = fs::read_dir(&origin)?
            .take(self.limits.max_files.saturating_add(1))
            .collect::<std::io::Result<Vec<_>>>()?;
        if entries.len() > self.limits.max_files {
            return Err(Error::Limit);
        }
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            self.visited = self.visited.checked_add(1).ok_or(Error::Limit)?;
            if self.visited > self.limits.max_files {
                return Err(Error::Limit);
            }
            let name = entry.file_name();
            if name
                .to_str()
                .is_some_and(|s| s.starts_with('.') || s == "node_modules")
            {
                continue;
            }
            let path = entry.path();
            match fs::metadata(&path) {
                Ok(meta) if meta.is_dir() => {
                    // A linked package is supported; a linked collection is not traversed implicitly.
                    if entry.file_type()?.is_symlink() && !path.join("SKILL.md").try_exists()? {
                        self.problem(
                            &path,
                            Error::Invalid("linked collection has no SKILL.md".into()),
                        );
                    } else {
                        self.walk(&path, scope, depth + 1)?;
                    }
                }
                Ok(_) => {}
                Err(error) => self.problem(&path, error.into()),
            }
        }
        Ok(())
    }
    fn skill(&mut self, origin: &Path, scope: &str, alias: &Path) -> Result<Skill> {
        let tree = Tree::capture(origin, self.limits)?;
        self.bytes = self.bytes.checked_add(tree.bytes).ok_or(Error::Limit)?;
        if self.bytes > self.limits.max_library_bytes {
            return Err(Error::Limit);
        }
        let bytes = tree.files.get("SKILL.md").ok_or(Error::Unavailable)?;
        if bytes.len() > self.limits.max_text_bytes {
            return Err(Error::Limit);
        }
        let (metadata, body) = metadata::parse(files::text(bytes)?)?;
        let unsupported = metadata.unsupported(body);
        let extra = metadata
            .extra
            .iter()
            .map(|(name, value)| serde_json::to_value(value).map(|value| (name.clone(), value)))
            .collect::<std::result::Result<BTreeMap<_, _>, _>>()
            .map_err(|_| Error::Invalid("skill metadata is not portable".into()))?;
        Ok(Skill {
            selection: Selection {
                id: source_digest(files::path_text(origin)?.as_bytes()),
                revision: tree.revision,
            },
            name: metadata.name,
            description: metadata.description,
            scope: scope.into(),
            origin: origin.into(),
            aliases: vec![alias.into()],
            model_invocable: !metadata.disable_model_invocation,
            user_invocable: metadata.user_invocable,
            unsupported,
            metadata: extra,
        })
    }
}

pub(crate) fn load(
    skill: &Skill,
    selection: &Selection,
    relative: &str,
    limits: &Limits,
) -> Result<String> {
    if skill.selection != *selection {
        return Err(Error::Changed);
    }
    if !skill.unsupported.is_empty() {
        return Err(Error::Unsupported);
    }
    let tree = Tree::capture(&skill.origin, limits)?;
    if tree.revision != selection.revision {
        return Err(Error::Changed);
    }
    let name = files::resource_name(relative)?;
    let bytes = tree.files.get(&name).ok_or(Error::Unavailable)?;
    if bytes.len() > limits.max_text_bytes {
        return Err(Error::Limit);
    }
    if name == "SKILL.md" {
        return metadata::parse(files::text(bytes)?).map(|(_, body)| body.to_owned());
    }
    Ok(files::text(bytes)?.into())
}
