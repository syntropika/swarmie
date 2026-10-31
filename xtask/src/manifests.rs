//! Check all actual Cargo workspace members, including synthetic fixtures.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::Error;

#[derive(Deserialize)]
struct Metadata {
    workspace_members: Vec<String>,
    packages: Vec<Package>,
}

#[derive(Deserialize)]
struct Package {
    id: String,
    manifest_path: PathBuf,
}

pub(crate) fn check(root: &Path) -> Result<(), Error> {
    let output = Command::new(env!("CARGO"))
        .current_dir(root)
        .args([
            "metadata",
            "--locked",
            "--offline",
            "--no-deps",
            "--format-version",
            "1",
        ])
        .output()?;
    if !output.status.success() {
        return Err(Error::Policy(format!(
            "Cargo could not enumerate workspace members: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let metadata: Metadata = serde_json::from_slice(&output.stdout)?;
    if metadata.workspace_members.is_empty() {
        return Err(Error::Policy("Cargo reported no workspace members".into()));
    }
    for member in metadata.workspace_members {
        let package = metadata
            .packages
            .iter()
            .find(|package| package.id == member)
            .ok_or_else(|| Error::Policy(format!("workspace member has no package: {member}")))?;
        let source = std::fs::read_to_string(&package.manifest_path)?;
        if !inherits_lints(&source)? {
            return Err(Error::Policy(format!(
                "{} must set [lints] workspace = true",
                package.manifest_path.display()
            )));
        }
    }
    Ok(())
}

fn inherits_lints(source: &str) -> Result<bool, Error> {
    let manifest: toml::Value = toml::from_str(source)?;
    Ok(manifest
        .get("lints")
        .and_then(|lints| lints.get("workspace"))
        .and_then(toml::Value::as_bool)
        == Some(true))
}

#[cfg(test)]
mod tests {
    use super::inherits_lints;

    #[test]
    fn only_explicit_valid_inheritance_passes() {
        assert!(matches!(
            inherits_lints("[lints]\nworkspace = true"),
            Ok(true)
        ));
        assert!(matches!(
            inherits_lints("[package]\nname = 'fixture'"),
            Ok(false)
        ));
        assert!(matches!(
            inherits_lints("[lints]\nworkspace = false"),
            Ok(false)
        ));
        assert!(matches!(
            inherits_lints("[lints]\nworkspace = 'true'"),
            Ok(false)
        ));
        assert!(inherits_lints("[lints").is_err());
    }
}
