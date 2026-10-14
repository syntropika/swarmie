use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use swarmie::{Error, Result};

const DATABASE: &str = "state/host.sqlite";
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Bootstrap {
    pub version: u32,
    pub database: String,
    pub store_id: String,
}

/// Resolve on the runtime node, independently of its execution directories.
pub fn default_data_root() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::Invalid("home directory unavailable; supply --state-dir".into()))?;
    Ok(PathBuf::from(home).join(".swarmie"))
}
pub(crate) fn private_directory(path: &Path) -> Result<()> {
    if let Ok(meta) = fs::symlink_metadata(path)
        && (!meta.is_dir() || meta.file_type().is_symlink())
    {
        return Err(Error::Storage(
            "application directory must be a real directory".into(),
        ));
    }
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
pub(crate) fn regular_file(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => Ok(true),
        Ok(_) => Err(Error::Storage(
            "application state must be a regular file".into(),
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
pub(crate) fn load(root: &Path) -> Result<Option<Bootstrap>> {
    let path = root.join("settings.json");
    if !regular_file(&path)? {
        return Ok(None);
    }
    if fs::metadata(&path)?.len() > 8192 {
        return Err(Error::Storage("bootstrap settings exceed 8 KiB".into()));
    }
    let settings: Bootstrap = serde_json::from_slice(&fs::read(path)?)?;
    if settings.version != 1 || settings.database != DATABASE {
        return Err(Error::Storage(
            "unsupported application bootstrap settings".into(),
        ));
    }
    Ok(Some(settings))
}
pub(crate) fn save(root: &Path, store_id: &str) -> Result<()> {
    let settings = Bootstrap {
        version: 1,
        database: DATABASE.into(),
        store_id: store_id.into(),
    };
    let path = root.join("settings.json");
    let temporary = root.join(format!(".bootstrap-{}.tmp", uuid::Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| -> Result<()> {
        let mut file = options.open(&temporary)?;
        file.write_all(&serde_json::to_vec(&settings)?)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        sync_directory(root)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}
pub(crate) fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    fs::File::open(path)?.sync_all()?;
    Ok(())
}
pub(crate) fn reject_legacy_root(root: &Path) -> Result<()> {
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok())
            && path
                .extension()
                .is_some_and(|s| s == "json" || s == "jsonl")
        {
            return Err(Error::Storage("legacy session files exist; select a fresh development data root; existing files were preserved".into()));
        }
    }
    Ok(())
}
