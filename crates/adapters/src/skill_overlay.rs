use fs2::FileExt;
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
};
use swarmie::Result;

const PREFIX: &str = "swarmie-skill-overlay-";
const MARKER: &str = ".swarmie-skill-overlay.json";
const FORMAT: &str = "swarmie-skill-overlay-v1";

pub(crate) struct Overlay {
    directory: tempfile::TempDir,
    lease: File,
    manifest: Value,
}

impl Overlay {
    pub(crate) fn create(links: &[(String, PathBuf)]) -> Result<Self> {
        let directory = tempfile::Builder::new().prefix(PREFIX).tempdir()?;
        #[cfg(target_os = "linux")]
        sweep(directory.path().parent().unwrap(), owner(directory.path())?);
        let lease = File::options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(directory.path().join(MARKER))?;
        lease.lock_exclusive()?;
        let manifest = json!({"format":FORMAT,"root":directory.path(),"links":links});
        let mut overlay = Self {
            directory,
            lease,
            manifest,
        };
        overlay.persist()?;
        let root = overlay.path().join(".claude/skills");
        fs::create_dir_all(&root)?;
        for (name, target) in links {
            #[cfg(unix)]
            std::os::unix::fs::symlink(target, root.join(name))?;
            #[cfg(not(unix))]
            {
                let _ = (name, target);
                return Err(swarmie::Error::Unsupported(
                    "Directory skill links are not qualified on this platform".into(),
                ));
            }
        }
        Ok(overlay)
    }

    pub(crate) fn path(&self) -> &Path {
        self.directory.path()
    }

    pub(crate) fn bind_native(&mut self, pid: Option<u64>) -> Result<()> {
        self.manifest["nativePid"] = json!(pid);
        #[cfg(target_os = "linux")]
        {
            self.manifest["nativeStart"] =
                json!(pid.and_then(|pid| process_start(pid).ok().flatten()));
        }
        self.persist()
    }

    pub(crate) fn intact(&self) -> bool {
        let root = self.path();
        if !fs::symlink_metadata(root.join(".claude")).is_ok_and(|metadata| metadata.is_dir())
            || !fs::symlink_metadata(root.join(".claude/skills"))
                .is_ok_and(|metadata| metadata.is_dir())
        {
            return false;
        }
        self.manifest["links"].as_array().is_some_and(|links| {
            links.iter().all(|link| {
                link[0]
                    .as_str()
                    .zip(link[1].as_str())
                    .is_some_and(|(name, target)| {
                        fs::read_link(root.join(".claude/skills").join(name))
                            .is_ok_and(|actual| actual == Path::new(target))
                    })
            })
        })
    }

    fn persist(&mut self) -> Result<()> {
        self.lease.rewind()?;
        self.lease.set_len(0)?;
        self.lease
            .write_all(serde_json::to_string(&self.manifest)?.as_bytes())?;
        self.lease.sync_all()?;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn owner(path: &Path) -> std::io::Result<u32> {
    use std::os::unix::fs::MetadataExt;
    Ok(fs::symlink_metadata(path)?.uid())
}

#[cfg(target_os = "linux")]
fn process_start(pid: u64) -> std::io::Result<Option<String>> {
    let stat = match fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => stat,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let start = stat
        .rsplit_once(") ")
        .and_then(|(_, fields)| fields.split_whitespace().nth(19))
        .ok_or_else(|| std::io::Error::other("Cannot identify native process lifetime"))?;
    Ok(Some(start.to_owned()))
}

/// Reap only unlocked, structurally intact overlays owned by this user. Never follow target links.
#[cfg(target_os = "linux")]
fn sweep(parent: &Path, uid: u32) {
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten().take(4096) {
        if !entry.file_name().to_string_lossy().starts_with(PREFIX) {
            continue;
        }
        let _ = reap(&entry.path(), uid);
    }
}

#[cfg(target_os = "linux")]
fn reap(root: &Path, uid: u32) -> std::io::Result<()> {
    if !fs::symlink_metadata(root)?.is_dir() || owner(root)? != uid {
        return Ok(());
    }
    let marker = root.join(MARKER);
    let metadata = fs::symlink_metadata(&marker)?;
    if !metadata.is_file() || metadata.len() > 256 * 1024 || owner(&marker)? != uid {
        return Ok(());
    }
    let mut lease = File::options().read(true).write(true).open(&marker)?;
    if lease.try_lock_exclusive().is_err() {
        return Ok(());
    }
    let mut text = String::new();
    lease.read_to_string(&mut text)?;
    let manifest: Value = serde_json::from_str(&text)?;
    if manifest["format"] != FORMAT || manifest["root"].as_str() != root.to_str() {
        return Ok(());
    }
    let Some(pid) = manifest["nativePid"].as_u64() else {
        return Ok(());
    };
    if let Some(start) = process_start(pid)?
        && (manifest["nativeStart"].as_str().is_none() || manifest["nativeStart"] == start)
    {
        return Ok(());
    }
    let Some(links) = manifest["links"].as_array() else {
        return Ok(());
    };
    let claude = root.join(".claude");
    let skills = claude.join("skills");
    if !fs::symlink_metadata(&claude)?.is_dir() || !fs::symlink_metadata(&skills)?.is_dir() {
        return Ok(());
    }
    let children = fs::read_dir(&skills)?.collect::<std::io::Result<Vec<_>>>()?;
    if children.len() != links.len()
        || fs::read_dir(root)?.count() != 2
        || fs::read_dir(&claude)?.count() != 1
    {
        return Ok(());
    }
    for child in &children {
        let name = child.file_name();
        let target = fs::read_link(child.path())?;
        if !links
            .iter()
            .any(|link| link[0].as_str() == name.to_str() && link[1].as_str() == target.to_str())
        {
            return Ok(());
        }
    }
    for child in children {
        fs::remove_file(child.path())?;
    }
    fs::remove_dir(skills)?;
    fs::remove_dir(claude)?;
    fs::remove_file(marker)?;
    fs::remove_dir(root)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn stale_cleanup_preserves_live_leases_native_processes_targets_and_foreign_files() {
        let target = tempfile::tempdir().unwrap();
        fs::write(target.path().join("owned"), "keep").unwrap();
        let mut overlay = Overlay::create(&[("alias".into(), target.path().to_owned())]).unwrap();
        let root = overlay.path().to_owned();
        let uid = owner(&root).unwrap();
        reap(&root, uid).unwrap();
        assert!(root.exists());
        overlay
            .bind_native(Some(std::process::id().into()))
            .unwrap();
        FileExt::unlock(&overlay.lease).unwrap();
        reap(&root, uid).unwrap();
        assert!(root.exists());
        overlay.manifest["nativePid"] = json!(u32::MAX);
        overlay.persist().unwrap();
        fs::write(root.join("foreign-file"), "keep").unwrap();
        reap(&root, uid).unwrap();
        assert!(root.join("foreign-file").exists());
        fs::remove_file(root.join("foreign-file")).unwrap();
        reap(&root, uid).unwrap();
        assert!(!root.exists());
        assert_eq!(
            fs::read_to_string(target.path().join("owned")).unwrap(),
            "keep"
        );
    }
}
