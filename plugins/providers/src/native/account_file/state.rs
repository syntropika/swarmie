//! Bounded typed observations and cooperative revision replacement. Initialization,
//! adoption authority and workflow interpretation remain outside this module.

use super::{NativeAccountFile, failure, validate_path};
use crate::accounts::{
    StoreWriteError,
    recovery::{LegacyObservation, ProviderOwner, ProviderState, RecoveryLimits, StateLoad},
};
use crate::native::NativeProviderIo;
use futures::future::BoxFuture;
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};
use uuid::Uuid;

impl NativeAccountFile {
    /// Observe typed state or exact legacy bytes through the shared physical
    /// owner. Absent never supplies provisioning evidence. A successful read
    /// alone does not resolve an earlier Unknown outcome or establish readiness.
    pub fn load_provider_state(
        &self,
        io: &NativeProviderIo,
        expected_owner: &ProviderOwner,
        limits: RecoveryLimits,
    ) -> BoxFuture<'static, tembra::Result<StateLoad>> {
        let path = self.path.clone();
        let io = io.clone();
        let expected_owner = expected_owner.clone();
        Box::pin(async move {
            io.run(move || read_state(&path, &expected_owner, limits))
                .await
                .map_err(StoreWriteError::into_error)?
        })
    }

    /// Replace only an initialized image with the exact predecessor revision.
    /// The embedding binds path/owner to its actual stable profile lease and
    /// excludes untracked writers; this helper does not prove that binding.
    /// File sync, same-parent replacement and directory sync precede success.
    /// There is no implicit initialize/adopt path or legacy credential access.
    pub fn replace_provider_state(
        &self,
        io: &NativeProviderIo,
        expected_owner: &ProviderOwner,
        expected_revision: Uuid,
        next: ProviderState,
        limits: RecoveryLimits,
    ) -> BoxFuture<'static, Result<(), StoreWriteError>> {
        let path = self.path.clone();
        let io = io.clone();
        let expected_owner = expected_owner.clone();
        Box::pin(async move {
            io.run(move || {
                replace_state(
                    &path,
                    &expected_owner,
                    expected_revision,
                    next,
                    limits,
                    File::sync_all,
                )
            })
            .await?
        })
    }
}

fn read_state(
    path: &Path,
    expected_owner: &ProviderOwner,
    limits: RecoveryLimits,
) -> tembra::Result<StateLoad> {
    validate_path(path)?;
    let read_limit = u64::try_from(limits.envelope_bytes())
        .ok()
        .and_then(|limit| limit.checked_add(1))
        .ok_or_else(|| failure("the native provider read bound is unsupported"))?;
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(StateLoad::Absent),
        Err(_) => {
            return Err(failure(
                "provider state could not be inspected; existing data was preserved",
            ));
        }
    };
    if !metadata.is_file() {
        return Err(failure(
            "provider state requires a regular file; existing data was preserved",
        ));
    }
    let file = File::open(path)
        .map_err(|_| failure("provider state could not be read; existing data was preserved"))?;
    let opened = file.metadata().map_err(|_| {
        failure("provider state could not be inspected; existing data was preserved")
    })?;
    if !opened.is_file() || opened.len() >= read_limit {
        return Err(failure(
            "provider state exceeds its storage bound or is not a regular file",
        ));
    }
    let mut bytes = Vec::new();
    file.take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(|_| failure("provider state could not be read; existing data was preserved"))?;
    if bytes.len() > limits.envelope_bytes() {
        return Err(failure(
            "provider state exceeds its storage bound; existing data was preserved",
        ));
    }
    if bytes.iter().find(|byte| !byte.is_ascii_whitespace()) == Some(&b'[') {
        LegacyObservation::from_bytes(expected_owner.clone(), bytes, limits).map(StateLoad::Legacy)
    } else {
        ProviderState::decode(&bytes, expected_owner, limits).map(StateLoad::Initialized)
    }
}

fn replace_state(
    path: &Path,
    expected_owner: &ProviderOwner,
    expected_revision: Uuid,
    next: ProviderState,
    limits: RecoveryLimits,
    sync_parent: impl Fn(&File) -> std::io::Result<()>,
) -> Result<(), StoreWriteError> {
    let not_applied = StoreWriteError::NotApplied;
    validate_path(path).map_err(not_applied)?;
    if next.owner() != expected_owner
        || next.revision() == expected_revision
        || expected_revision.is_nil()
    {
        return Err(not_applied(failure(
            "invalid provider state owner or replacement revision",
        )));
    }
    let encoded = next.encode(limits).map_err(not_applied)?;
    let current = read_state(path, expected_owner, limits).map_err(not_applied)?;
    if !matches!(current, StateLoad::Initialized(state) if state.revision() == expected_revision) {
        return Err(not_applied(failure(
            "provider state is not the expected initialized revision",
        )));
    }
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let directory = File::open(parent)
        .map_err(|_| not_applied(failure("provider state directory is unavailable")))?;
    // Refuse unsupported directory synchronization before any publication.
    sync_parent(&directory).map_err(|_| {
        not_applied(failure(
            "provider state directory could not be synchronized",
        ))
    })?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|_| not_applied(failure("provider state could not be staged")))?;
    temporary
        .write_all(&encoded)
        .and_then(|()| temporary.flush())
        .and_then(|()| temporary.as_file().sync_all())
        .map_err(|_| not_applied(failure("provider state could not be synchronized")))?;
    temporary.persist(path).map_err(|_| {
        StoreWriteError::Unknown(failure("provider state publication is uncertain"))
    })?;
    sync_parent(&directory)
        .map_err(|_| StoreWriteError::Unknown(failure("provider state durability is uncertain")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::NativeProviderLease;
    use crate::{
        AccountRecord,
        settings::{Account, ModelDefaults},
    };
    #[cfg(unix)]
    use futures::FutureExt;
    use std::collections::BTreeMap;
    use tokio::runtime::Handle;

    fn owner() -> ProviderOwner {
        ProviderOwner::new("principal".into(), Uuid::from_u128(1)).unwrap()
    }

    fn image(revision: u128, label: &str) -> ProviderState {
        let account = AccountRecord::new(
            Account {
                id: "existing-account".into(),
                provider_id: "fixture".into(),
                provider_label: "Fixture".into(),
                label: label.into(),
                models: vec![],
                available: false,
                defaults: ModelDefaults::default(),
                authorization_method: None,
                models_truncated: false,
            },
            serde_json::from_value(serde_json::json!({"extension":{"preserve":[1,2,3]}})).unwrap(),
        )
        .unwrap();
        ProviderState::new(
            owner(),
            Uuid::from_u128(revision),
            vec![account],
            BTreeMap::new(),
            None,
            RecoveryLimits::default(),
        )
        .unwrap()
    }

    fn io(path: &Path) -> NativeProviderIo {
        NativeProviderIo::new(
            NativeProviderLease::acquire(path.join("owner.lock")).unwrap(),
            Handle::current(),
        )
    }

    #[tokio::test(flavor = "current_thread")]
    async fn classified_load_preserves_legacy_bytes_and_never_invents_provisioning() {
        let directory = tempfile::tempdir().unwrap();
        let io = io(directory.path());
        let path = directory.path().join("accounts.json");
        let store = NativeAccountFile::new(path.clone());
        let limits = RecoveryLimits::default();
        assert!(matches!(
            store
                .load_provider_state(&io, &owner(), limits)
                .await
                .unwrap(),
            StateLoad::Absent
        ));
        assert!(!path.exists());
        let legacy = format!(
            " \n{}\n",
            serde_json::to_string(image(2, "original").accounts()).unwrap()
        )
        .into_bytes();
        std::fs::write(&path, &legacy).unwrap();
        let StateLoad::Legacy(observed) = store
            .load_provider_state(&io, &owner(), limits)
            .await
            .unwrap()
        else {
            panic!("expected legacy observation")
        };
        assert_eq!(observed.bytes(), legacy);
        assert!(observed.predecessor().matches_bytes(&owner(), &legacy));
        assert_eq!(std::fs::read(&path).unwrap(), legacy);
        let original = image(2, "original").encode(limits).unwrap();
        std::fs::write(&path, &original).unwrap();
        let StateLoad::Initialized(state) = store
            .load_provider_state(&io, &owner(), limits)
            .await
            .unwrap()
        else {
            panic!("expected initialized observation")
        };
        assert_eq!(state.encode(limits).unwrap(), original);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn malformed_foreign_oversized_and_nonregular_inputs_remain_intact_errors() {
        let directory = tempfile::tempdir().unwrap();
        let io = io(directory.path());
        let path = directory.path().join("accounts.json");
        let store = NativeAccountFile::new(path.clone());
        let limits = RecoveryLimits::default();
        for bytes in [b"".as_slice(), b"{broken", b"null", b"{\"accounts\":[]}"] {
            std::fs::write(&path, bytes).unwrap();
            assert!(
                store
                    .load_provider_state(&io, &owner(), limits)
                    .await
                    .is_err()
            );
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        let original = image(2, "original").encode(limits).unwrap();
        std::fs::write(&path, &original).unwrap();
        let foreign = ProviderOwner::new("other".into(), owner().profile()).unwrap();
        assert!(
            store
                .load_provider_state(&io, &foreign, limits)
                .await
                .is_err()
        );
        assert!(
            store
                .load_provider_state(&io, &owner(), RecoveryLimits::new(1, 1, 16).unwrap())
                .await
                .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let existing = directory.path().join("existing-directory");
        std::fs::create_dir(&existing).unwrap();
        std::fs::write(existing.join("owned"), b"preserve").unwrap();
        assert!(
            NativeAccountFile::new(existing.clone())
                .load_provider_state(&io, &owner(), limits)
                .await
                .is_err()
        );
        assert_eq!(std::fs::read(existing.join("owned")).unwrap(), b"preserve");
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn replacement_checks_exact_revision_owner_and_bounds_without_dropping_extensions() {
        let directory = tempfile::tempdir().unwrap();
        let io = io(directory.path());
        let path = directory.path().join("accounts.json");
        let store = NativeAccountFile::new(path.clone());
        let limits = RecoveryLimits::default();
        std::fs::write(&path, image(2, "original").encode(limits).unwrap()).unwrap();
        assert!(
            store
                .replace_provider_state(
                    &io,
                    &owner(),
                    Uuid::from_u128(2),
                    image(3, "replacement"),
                    limits
                )
                .await
                .is_ok()
        );
        let expected = image(3, "replacement").encode(limits).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), expected);
        for (revision, next, bounds) in [
            (Uuid::from_u128(2), image(4, "stale"), limits),
            (Uuid::from_u128(3), image(3, "same revision"), limits),
            (Uuid::nil(), image(4, "nil revision"), limits),
            (
                Uuid::from_u128(3),
                image(4, "oversized"),
                RecoveryLimits::new(1, 1, 16).unwrap(),
            ),
        ] {
            assert!(matches!(
                store
                    .replace_provider_state(&io, &owner(), revision, next, bounds)
                    .await,
                Err(StoreWriteError::NotApplied(_))
            ));
            assert_eq!(std::fs::read(&path).unwrap(), expected);
        }
        let foreign = ProviderOwner::new("other".into(), owner().profile()).unwrap();
        assert!(matches!(
            store
                .replace_provider_state(
                    &io,
                    &foreign,
                    Uuid::from_u128(3),
                    image(4, "foreign"),
                    limits
                )
                .await,
            Err(StoreWriteError::NotApplied(_))
        ));
        assert_eq!(std::fs::read(&path).unwrap(), expected);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn replacement_never_initializes_absence_or_adopts_existing_legacy_data() {
        let directory = tempfile::tempdir().unwrap();
        let io = io(directory.path());
        let path = directory.path().join("accounts.json");
        let store = NativeAccountFile::new(path.clone());
        let limits = RecoveryLimits::default();
        assert!(matches!(
            store
                .replace_provider_state(
                    &io,
                    &owner(),
                    Uuid::from_u128(2),
                    image(3, "refused"),
                    limits
                )
                .await,
            Err(StoreWriteError::NotApplied(_))
        ));
        assert!(!path.exists());
        for bytes in [b"[]".as_slice(), b"{broken"] {
            std::fs::write(&path, bytes).unwrap();
            assert!(matches!(
                store
                    .replace_provider_state(
                        &io,
                        &owner(),
                        Uuid::from_u128(2),
                        image(3, "refused"),
                        limits
                    )
                    .await,
                Err(StoreWriteError::NotApplied(_))
            ));
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn failed_directory_sync_after_publication_is_unknown_with_target_bytes_retained() {
        let directory = tempfile::tempdir().unwrap();
        let io = io(directory.path());
        let path = directory.path().join("accounts.json");
        let limits = RecoveryLimits::default();
        std::fs::write(&path, image(2, "original").encode(limits).unwrap()).unwrap();
        let target = path.clone();
        let result = io
            .run(move || {
                let calls = std::cell::Cell::new(0);
                replace_state(
                    &target,
                    &owner(),
                    Uuid::from_u128(2),
                    image(3, "published"),
                    limits,
                    |directory| {
                        calls.set(calls.get() + 1);
                        if calls.get() == 1 {
                            directory.sync_all()
                        } else {
                            Err(std::io::Error::other("fixture directory sync failure"))
                        }
                    },
                )
            })
            .await
            .unwrap_or_else(|_| panic!("dispatch failed"));
        assert!(matches!(result, Err(StoreWriteError::Unknown(_))));
        assert_eq!(
            std::fs::read(&path).unwrap(),
            image(3, "published").encode(limits).unwrap()
        );
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn queued_publication_finishes_after_its_observer_is_dropped() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(1)
            .build()
            .unwrap();
        runtime.block_on(async {
            let directory = tempfile::tempdir().unwrap();
            let io = io(directory.path());
            let path = directory.path().join("accounts.json");
            let store = NativeAccountFile::new(path.clone());
            let limits = RecoveryLimits::default();
            std::fs::write(&path, image(2, "original").encode(limits).unwrap()).unwrap();
            let (started, ready) = tokio::sync::oneshot::channel();
            let (release, wait) = std::sync::mpsc::channel();
            let occupied = tokio::task::spawn_blocking(move || {
                let _ = started.send(());
                let _ = wait.recv();
            });
            ready.await.unwrap();
            assert!(
                store
                    .replace_provider_state(
                        &io,
                        &owner(),
                        Uuid::from_u128(2),
                        image(3, "published"),
                        limits
                    )
                    .now_or_never()
                    .is_none()
            );
            drop(store);
            release.send(()).unwrap();
            occupied.await.unwrap();
            let observed = NativeAccountFile::new(path)
                .load_provider_state(&io, &owner(), limits)
                .await
                .unwrap();
            let StateLoad::Initialized(state) = observed else {
                panic!("expected published image")
            };
            assert_eq!(
                state.encode(limits).unwrap(),
                image(3, "published").encode(limits).unwrap()
            );
        });
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn dangling_and_existing_symbolic_links_are_refused_without_replacement() {
        let directory = tempfile::tempdir().unwrap();
        let io = io(directory.path());
        let path = directory.path().join("accounts.json");
        let target = directory.path().join("actual.json");
        std::os::unix::fs::symlink(&target, &path).unwrap();
        let store = NativeAccountFile::new(path.clone());
        let limits = RecoveryLimits::default();
        assert!(
            store
                .load_provider_state(&io, &owner(), limits)
                .await
                .is_err()
        );
        let original = image(2, "original").encode(limits).unwrap();
        std::fs::write(&target, &original).unwrap();
        assert!(
            store
                .load_provider_state(&io, &owner(), limits)
                .await
                .is_err()
        );
        assert!(matches!(
            store
                .replace_provider_state(
                    &io,
                    &owner(),
                    Uuid::from_u128(2),
                    image(3, "refused"),
                    limits
                )
                .await,
            Err(StoreWriteError::NotApplied(_))
        ));
        assert_eq!(std::fs::read_link(path).unwrap(), target);
        assert_eq!(std::fs::read(target).unwrap(), original);
    }
}
