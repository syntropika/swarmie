use std::collections::BTreeSet;

use sha2::{Digest, Sha256};
use swarmie_plugin_contracts::*;
use tembra_executor_js::BundledJsArtifact;

use crate::{ManagerError, Result};

pub(crate) fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub(crate) fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', ':'])
        && !path.chars().any(char::is_control)
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

pub(crate) fn validate_package(package: &AcquiredPackage) -> Result<()> {
    let manifest = &package.manifest;
    if manifest.schema_version != 1 || manifest.id.is_empty() || manifest.facets.is_empty() {
        return Err(ManagerError::InvalidPackage(
            "schema, identity or facets are invalid".into(),
        ));
    }
    if manifest.compatibility.tembra_revision != TEMBRA_REVISION {
        return Err(ManagerError::Compatibility(
            "Tembra revision differs".into(),
        ));
    }
    let version = semver::Version::parse(env!("CARGO_PKG_VERSION"))
        .map_err(|error| ManagerError::Compatibility(error.to_string()))?;
    let requirement = semver::VersionReq::parse(&manifest.compatibility.swarmie)
        .map_err(|error| ManagerError::Compatibility(error.to_string()))?;
    if !requirement.matches(&version) {
        return Err(ManagerError::Compatibility(
            "Swarmie version requirement is not met".into(),
        ));
    }
    semver::Version::parse(&manifest.version)
        .map_err(|error| ManagerError::InvalidPackage(error.to_string()))?;
    if package
        .files
        .iter()
        .any(|file| file.bytes.len() > 8 * 1024 * 1024)
    {
        return Err(ManagerError::InvalidPackage(
            "approved package files must not exceed 8 MiB each".into(),
        ));
    }
    let mut paths = BTreeSet::new();
    for expected in &manifest.files {
        if !safe_path(&expected.path) || !paths.insert(expected.path.as_str()) {
            return Err(ManagerError::InvalidPackage(
                "file path is unsafe or duplicated".into(),
            ));
        }
        let file = package
            .files
            .iter()
            .find(|file| file.path == expected.path)
            .ok_or_else(|| ManagerError::Integrity(format!("missing {}", expected.path)))?;
        if digest(&file.bytes) != expected.sha256 {
            return Err(ManagerError::Integrity(expected.path.clone()));
        }
    }
    if package.files.len() != paths.len()
        || package
            .files
            .iter()
            .map(|file| &file.path)
            .collect::<BTreeSet<_>>()
            .len()
            != paths.len()
    {
        return Err(ManagerError::Integrity(
            "unlisted or duplicate files".into(),
        ));
    }
    let mut facet_ids = BTreeSet::new();
    for facet in &manifest.facets {
        if facet.id.is_empty()
            || !facet_ids.insert(facet.id.as_str())
            || facet.selected_export.is_empty()
        {
            return Err(ManagerError::InvalidPackage(
                "duplicate facet or missing export".into(),
            ));
        }
        let file = package
            .files
            .iter()
            .find(|file| file.path == facet.artifact_path)
            .ok_or_else(|| ManagerError::Integrity(facet.artifact_path.clone()))?;
        if digest(&file.bytes) != facet.sha256 {
            return Err(ManagerError::Integrity(facet.artifact_path.clone()));
        }
        let text = std::str::from_utf8(&file.bytes)
            .map_err(|error| ManagerError::InvalidPackage(error.to_string()))?;
        let artifact = BundledJsArtifact::from_json(text)?;
        if !artifact
            .exports()
            .any(|export| export == facet.selected_export)
        {
            return Err(ManagerError::InvalidPackage(
                "selected facet export is absent from its artifact".into(),
            ));
        }
    }
    let mut contribution_ids = BTreeSet::new();
    for contribution in &manifest.ui {
        if contribution.id.is_empty()
            || !contribution_ids.insert(contribution.id.as_str())
            || contribution.plugin_id != manifest.id
            || contribution.label.is_empty()
            || contribution.slot_id.is_empty()
            || !safe_path(&contribution.module_path)
            || !paths.contains(contribution.module_path.as_str())
            || contribution.export_name.is_empty()
        {
            return Err(ManagerError::InvalidPackage(
                "UI module or export is invalid".into(),
            ));
        }
    }
    if package.provenance.provider_id.is_empty()
        || package.provenance.source.is_empty()
        || package.provenance.integrity.is_empty()
    {
        return Err(ManagerError::InvalidPackage(
            "acquisition provenance is incomplete".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package() -> AcquiredPackage {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/fixtures/dynamic-consumer/1.0.0");
        let manifest: PluginManifest =
            serde_json::from_slice(&std::fs::read(root.join("manifest.json")).unwrap()).unwrap();
        let files = manifest
            .files
            .iter()
            .map(|file| PackageFile {
                path: file.path.clone(),
                bytes: std::fs::read(root.join(&file.path)).unwrap(),
            })
            .collect();
        AcquiredPackage {
            manifest,
            files,
            provenance: PackageProvenance {
                provider_id: "test".into(),
                source: "approved fixture".into(),
                integrity: "source records retained".into(),
            },
        }
    }

    #[test]
    fn portable_paths_refuse_aliases_and_platform_specific_escape_forms() {
        for path in ["asset.js", "nested/widget.js", "assets/counter-widget.js"] {
            assert!(safe_path(path), "{path}");
        }
        for path in [
            "",
            "/asset.js",
            "../asset.js",
            "a/../b",
            "a/./b",
            "a//b",
            "a/",
            "C:asset.js",
            "a\\b",
            "a\0b",
        ] {
            assert!(!safe_path(path), "{path:?}");
        }
    }

    #[test]
    fn actual_loader_refuses_missing_exports_and_unsupported_envelopes_before_caching() {
        let mut invalid = package();
        invalid.manifest.facets[0].selected_export = "absent".into();
        assert!(matches!(
            validate_package(&invalid),
            Err(ManagerError::InvalidPackage(_))
        ));
        let mut invalid = package();
        let mut envelope: serde_json::Value =
            serde_json::from_slice(&invalid.files[0].bytes).unwrap();
        envelope["format"] = serde_json::json!("unsupported-profile");
        invalid.files[0].bytes = serde_json::to_vec(&envelope).unwrap();
        let sha256 = digest(&invalid.files[0].bytes);
        invalid.manifest.files[0].sha256 = sha256.clone();
        invalid.manifest.facets[0].sha256 = sha256;
        assert!(matches!(
            validate_package(&invalid),
            Err(ManagerError::Runtime(_))
        ));
    }

    #[test]
    fn ui_contributions_must_have_unique_ids_and_the_manifest_owner() {
        let mut invalid = package();
        let contribution = UiContribution {
            id: "slot".into(),
            plugin_id: "another-plugin".into(),
            kind: UiContributionKind::Slot,
            label: "Counter".into(),
            slot_id: "counter".into(),
            order: 0,
            module_path: "artifact.json".into(),
            export_name: "Counter".into(),
        };
        invalid.manifest.ui.push(contribution.clone());
        assert!(matches!(
            validate_package(&invalid),
            Err(ManagerError::InvalidPackage(_))
        ));
        invalid.manifest.ui[0].plugin_id = invalid.manifest.id.clone();
        let duplicate = invalid.manifest.ui[0].clone();
        invalid.manifest.ui.push(duplicate);
        assert!(matches!(
            validate_package(&invalid),
            Err(ManagerError::InvalidPackage(_))
        ));
    }
}
