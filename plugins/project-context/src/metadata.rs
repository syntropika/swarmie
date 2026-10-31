use crate::{Error, Result};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct Metadata {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub disable_model_invocation: bool,
    #[serde(default = "enabled")]
    pub user_invocable: bool,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_yaml_ng::Value>,
}
fn enabled() -> bool {
    true
}

pub(crate) fn parse(text: &str) -> Result<(Metadata, &str)> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.split_inclusive('\n');
    if lines.next().map(str::trim_end) != Some("---") {
        return Err(Error::Invalid("SKILL.md requires YAML frontmatter".into()));
    }
    let start = text
        .find('\n')
        .map(|n| n + 1)
        .ok_or_else(|| Error::Invalid("unterminated skill frontmatter".into()))?;
    let mut end = start;
    let mut boundary = None;
    for line in lines {
        if line.trim_end() == "---" {
            boundary = Some((end, end + line.len()));
            break;
        }
        end += line.len();
    }
    let (end, body) =
        boundary.ok_or_else(|| Error::Invalid("unterminated skill frontmatter".into()))?;
    let yaml = text
        .get(start..end)
        .ok_or_else(|| Error::Invalid("invalid frontmatter offsets".into()))?;
    let metadata: Metadata = serde_yaml_ng::from_str(yaml)
        .map_err(|_| Error::Invalid("invalid skill frontmatter".into()))?;
    let name = &metadata.name;
    if name.is_empty()
        || name.len() > 64
        || name.starts_with('-')
        || name.ends_with('-')
        || name.contains("--")
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(Error::Invalid(
            "skill name must use lowercase letters, digits and single hyphens".into(),
        ));
    }
    if metadata.description.trim().is_empty()
        || metadata.description.chars().count() > 1024
        || metadata
            .description
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(Error::Invalid("invalid skill description".into()));
    }
    let body = text
        .get(body..)
        .ok_or_else(|| Error::Invalid("invalid skill body".into()))?;
    if body.trim().is_empty() {
        return Err(Error::Invalid("skill instructions are empty".into()));
    }
    Ok((metadata, body))
}

impl Metadata {
    pub fn unsupported(&self, body: &str) -> Vec<String> {
        let mut fields: Vec<_> = self
            .extra
            .keys()
            .filter(|name| {
                !matches!(
                    name.as_str(),
                    "license" | "compatibility" | "metadata" | "argument-hint"
                )
            })
            .cloned()
            .collect();
        if body.contains("!`") {
            fields.push("dynamic shell preprocessing".into());
        }
        fields
    }
}
