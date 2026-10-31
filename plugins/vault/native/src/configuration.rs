use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
use swarmie_cloud::account::Endpoints;
use swarmie_vault::{CeremonyReceipt, VaultIdentity};
use uuid::Uuid;

/// Safe launch metadata only. OS protected storage owns credentials, private keys and trust pins.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub installation: Uuid,
    pub endpoints: Option<Endpoints>,
    pub account: Option<Uuid>,
    pub roots: Vec<Root>,
    pub joins: Vec<PendingJoin>,
    pub ceremonies: Vec<CeremonyReceipt>,
    pub detector_enabled: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct PendingJoin {
    pub identity: VaultIdentity,
    pub device: Uuid,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Root {
    pub identity: VaultIdentity,
    pub device: Uuid,
    /// Retained local key locators; accepting replacement enrollment never deletes old keys.
    pub previous_devices: Vec<Uuid>,
    pub pending_replacement: Option<AcceptedDevice>,
    pub pending: Option<CeremonyReceipt>,
    pub verified_code: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct AcceptedDevice {
    pub device: Uuid,
    pub verified_code: String,
}
pub fn load(root: &Path) -> Result<Configuration, ()> {
    fs::create_dir_all(root).map_err(|_| ())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root, fs::Permissions::from_mode(0o700)).map_err(|_| ())?;
    }
    match fs::read(root.join("vault-configuration.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| ()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Configuration {
            installation: Uuid::new_v4(),
            endpoints: None,
            account: None,
            roots: vec![],
            joins: vec![],
            ceremonies: vec![],
            detector_enabled: true,
        }),
        Err(_) => Err(()),
    }
}
pub fn save(root: &Path, configuration: &Configuration) -> Result<(), ()> {
    let temporary = root.join("vault-configuration.pending");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary).map_err(|_| ())?;
    use std::io::Write;
    file.write_all(&serde_json::to_vec(configuration).map_err(|_| ())?)
        .map_err(|_| ())?;
    file.sync_all().map_err(|_| ())?;
    fs::rename(temporary, root.join("vault-configuration.json")).map_err(|_| ())?;
    #[cfg(unix)]
    fs::File::open(root)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| ())?;
    Ok(())
}
