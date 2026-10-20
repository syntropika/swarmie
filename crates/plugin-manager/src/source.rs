use std::{
    fs,
    path::{Path, PathBuf},
};

use swarmie_plugin_contracts::*;
use tembra::{core, prelude::*};

use crate::{
    ManagerError, Result as ManagerResult,
    validation::{digest, safe_path},
};

/// A local package directory served through independently named Tembra operations.
///
/// Every package has a `manifest.json`; its listed files are relative to that manifest.
/// Symbolic links are refused rather than allowing a package to escape its configured root.
#[derive(Clone)]
pub struct LocalSource {
    root: PathBuf,
    provider_id: String,
}

#[tembra::contract(local, name = "swarmie.local-plugin-source", version = 1)]
trait LocalPluginSource {
    #[portable]
    async fn search(&self, request: SearchRequest) -> Result<SearchResult>;

    #[portable]
    async fn resolve(&self, request: ResolveRequest) -> Result<ResolvedPlugin>;

    #[portable]
    async fn inspect(&self, request: InspectRequest) -> Result<PluginManifest>;

    #[portable]
    async fn acquire(&self, request: AcquireRequest) -> Result<AcquiredPackage>;
}

impl LocalSource {
    /// Creates a provider configuration without reading or modifying the directory.
    pub fn new(provider_id: impl Into<String>, root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            provider_id: provider_id.into(),
        }
    }

    /// Declares an eager compiled provider whose registry entries share its lifecycle.
    ///
    /// # Errors
    /// Returns Tembra declaration errors; file errors are reported by actual operations.
    pub fn facet(&self) -> Result<NativeFacet> {
        let source = self.clone();
        NativeFacet::new("swarmie.local-plugin-source", vec![], move |_, setup| {
            let source = source.clone();
            setup.serve_with_options(
                LocalPluginSource,
                move |_, _, declaration| {
                    let registry = source_registry()?;
                    for (operation, method) in [
                        (SourceOperation::Search, "search"),
                        (SourceOperation::Resolve, "resolve"),
                        (SourceOperation::Inspect, "inspect"),
                        (SourceOperation::Acquire, "acquire"),
                    ] {
                        declaration.core_mut().register_method(
                            &registry,
                            format!("{}/{}", source.provider_id, method),
                            tembra::serde_json::to_value(SourceRegistryMetadata {
                                provider_id: source.provider_id.clone(),
                                operation,
                            })
                            .map_err(|error| {
                                RuntimeError::invalid("source_metadata", error.to_string())
                            })?,
                            method,
                        )?;
                    }
                    Ok(source.clone())
                },
                core::ServeOptions {
                    lifetime: core::Lifetime::Host,
                    ..Default::default()
                },
            )
        })
    }

    fn manifests(&self) -> ManagerResult<Vec<(PathBuf, PluginManifest)>> {
        fn visit(
            path: &Path,
            result: &mut Vec<(PathBuf, PluginManifest)>,
            depth: usize,
        ) -> ManagerResult<()> {
            if depth > 6 {
                return Err(ManagerError::InvalidPackage(
                    "source directory depth exceeds six".into(),
                ));
            }
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                let kind = entry.file_type()?;
                if kind.is_symlink() {
                    return Err(ManagerError::InvalidPackage(
                        "source symlinks are refused".into(),
                    ));
                }
                if kind.is_dir() {
                    visit(&entry.path(), result, depth + 1)?;
                } else if entry.file_name() == "manifest.json" {
                    let manifest = serde_json::from_slice(&fs::read(entry.path())?)?;
                    result.push((path.to_path_buf(), manifest));
                }
            }
            Ok(())
        }
        let mut result = vec![];
        visit(&self.root, &mut result, 0)?;
        Ok(result)
    }

    fn find(&self, id: &str, version: &str) -> ManagerResult<(PathBuf, PluginManifest)> {
        let exact = semver::Version::parse(version).ok();
        let requirement = semver::VersionReq::parse(version)
            .map_err(|error| ManagerError::InvalidPackage(error.to_string()))?;
        let mut candidates = self
            .manifests()?
            .into_iter()
            .filter(|(_, manifest)| manifest.id == id)
            .filter_map(|pair| {
                semver::Version::parse(&pair.1.version)
                    .ok()
                    .filter(|version| {
                        exact
                            .as_ref()
                            .map_or_else(|| requirement.matches(version), |exact| exact == version)
                    })
                    .map(|version| (version, pair))
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| right.0.cmp(&left.0));
        candidates
            .into_iter()
            .next()
            .map(|(_, pair)| pair)
            .ok_or_else(|| {
                ManagerError::InvalidPackage(format!("release not found: {id}@{version}"))
            })
    }

    fn acquire_package(&self, request: AcquireRequest) -> ManagerResult<AcquiredPackage> {
        let (root, manifest) = self.find(&request.plugin_id, &request.version)?;
        if manifest.version != request.version {
            return Err(ManagerError::InvalidPackage(
                "acquire requires an exact resolved version".into(),
            ));
        }
        let canonical_root = root.canonicalize()?;
        let mut files = vec![];
        for file in &manifest.files {
            if !safe_path(&file.path) {
                return Err(ManagerError::InvalidPackage("unsafe source path".into()));
            }
            let path = root.join(&file.path);
            let mut cursor = root.clone();
            for component in Path::new(&file.path).components() {
                cursor.push(component);
                if fs::symlink_metadata(&cursor)?.file_type().is_symlink() {
                    return Err(ManagerError::InvalidPackage(
                        "package symlinks are refused".into(),
                    ));
                }
            }
            if !path.canonicalize()?.starts_with(&canonical_root) {
                return Err(ManagerError::InvalidPackage(
                    "source path escapes root".into(),
                ));
            }
            files.push(PackageFile {
                path: file.path.clone(),
                bytes: fs::read(path)?,
            });
        }
        let integrity = digest(&serde_json::to_vec(&manifest)?);
        Ok(AcquiredPackage {
            manifest,
            files,
            provenance: PackageProvenance {
                provider_id: self.provider_id.clone(),
                source: format!("local:{}@{}", request.plugin_id, request.version),
                integrity,
            },
        })
    }
}

impl LocalPluginSourceProvider for LocalSource {
    async fn search(&self, request: SearchRequest, _context: Context) -> Result<SearchResult> {
        let plugins = self
            .manifests()
            .map_err(ManagerError::into_runtime)?
            .into_iter()
            .map(|(_, manifest)| manifest)
            .filter(|manifest| manifest.id.contains(&request.query))
            .map(|manifest| PluginSummary {
                id: manifest.id,
                version: manifest.version,
                description: manifest.distribution.description,
            })
            .collect();
        Ok(SearchResult { plugins })
    }

    async fn resolve(&self, request: ResolveRequest, _context: Context) -> Result<ResolvedPlugin> {
        let (_, manifest) = self
            .find(&request.plugin_id, &request.version)
            .map_err(ManagerError::into_runtime)?;
        Ok(ResolvedPlugin { manifest })
    }

    async fn inspect(&self, request: InspectRequest, _context: Context) -> Result<PluginManifest> {
        self.find(&request.plugin_id, &request.version)
            .map(|(_, manifest)| manifest)
            .map_err(ManagerError::into_runtime)
    }

    async fn acquire(&self, request: AcquireRequest, _context: Context) -> Result<AcquiredPackage> {
        self.acquire_package(request)
            .map_err(ManagerError::into_runtime)
    }
}
