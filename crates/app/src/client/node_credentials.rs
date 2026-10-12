//! Private immutable credential files referenced by client node metadata.
use swarmie::{Error, Result};
use std::{
    io::Write,
    path::{Path, PathBuf},
};
pub struct Credentials {
    directory: PathBuf,
}
impl Credentials {
    pub fn new(root: &Path) -> Self {
        Self {
            directory: root.join("credentials/node-connections"),
        }
    }
    fn path(&self, reference: &str) -> Result<PathBuf> {
        uuid::Uuid::parse_str(reference)
            .map_err(|_| Error::Invalid("invalid node credential reference".into()))?;
        self.check_directories(false)?;
        Ok(self.directory.join(format!("{reference}.json")))
    }
    fn check_directories(&self, create: bool) -> Result<()> {
        for path in [self.directory.parent().unwrap(), self.directory.as_path()] {
            match std::fs::symlink_metadata(path) {
                Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink() => {}
                Ok(_) => {
                    return Err(Error::Storage(
                        "node credential directory must be a real directory".into(),
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound && create => {
                    match std::fs::create_dir(path) {
                        Ok(()) => {}
                        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                            let meta = std::fs::symlink_metadata(path)?;
                            if !meta.is_dir() || meta.file_type().is_symlink() {
                                return Err(Error::Storage(
                                    "node credential directory must be a real directory".into(),
                                ));
                            }
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
                Err(error) => return Err(error.into()),
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
            }
        }
        Ok(())
    }
    pub fn save(&self, token: &str) -> Result<String> {
        if token.is_empty() || token.len() > 32 * 1024 || token.chars().any(char::is_control) {
            return Err(Error::Invalid("invalid node bearer credential".into()));
        }
        self.check_directories(true)?;
        let reference = uuid::Uuid::new_v4().to_string();
        let path = self.path(&reference)?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let result = (|| -> Result<()> {
            let mut file = options.open(&path)?;
            file.write_all(&serde_json::to_vec(token)?)?;
            file.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(path);
        }
        result?;
        Ok(reference)
    }
    pub fn read(&self, reference: &str) -> Result<String> {
        let path = self.path(reference)?;
        let meta = std::fs::symlink_metadata(&path)?;
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 64 * 1024 {
            return Err(Error::Storage("invalid node credential file".into()));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(serde_json::from_slice(&std::fs::read(path)?)?)
    }
    pub fn remove(&self, reference: &str) -> Result<()> {
        let path = self.path(reference)?;
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}
