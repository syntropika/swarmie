//! Physical serialization and local file-lock lifetime for native provider I/O.
//! This is not profile initialization, workflow authority or a platform fence.

use futures::future::BoxFuture;
use std::{fs::File, path::Path, sync::Arc};
use swarmie_providers::accounts::StoreWriteError;
use tokio::{runtime::Handle, sync::Semaphore};

fn failure(message: &str) -> tembra::Error {
    tembra::Error::new(tembra::Code::Unavailable, "providers.native_io", message)
}

/// A real exclusive file lock with one shared serialization gate. Acquire it in
/// the embedding's setup/blocking context using its trusted lock-file path. The
/// file is never truncated, and no profile directory or identity is created.
/// This proves local file ownership only: it does not authenticate a principal,
/// validate profile identity or settle earlier platform/remote-filesystem calls.
pub struct NativeProviderLease {
    _file: File,
    serial: Arc<Semaphore>,
}

impl NativeProviderLease {
    pub fn acquire(path: impl AsRef<Path>) -> tembra::Result<Arc<Self>> {
        let path = path.as_ref();
        if path.file_name().is_none() {
            return Err(failure("a native profile lock file is required"));
        }
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|_| failure("native profile ownership is unavailable"))?;
        if !file
            .metadata()
            .map_err(|_| failure("native profile ownership is unavailable"))?
            .is_file()
        {
            return Err(failure("a native profile lock file is required"));
        }
        file.try_lock()
            .map_err(|_| failure("native profile ownership is unavailable"))?;
        Ok(Arc::new(Self {
            _file: file,
            serial: Arc::new(Semaphore::new(1)),
        }))
    }
}

/// Blocking I/O on an embedding-supplied executor and actual retained lease.
/// Clones and separately constructed handles sharing a lease use the same gate.
/// The embedding keeps its runtime alive until physical operations finish.
#[derive(Clone)]
pub struct NativeProviderIo {
    lease: Arc<NativeProviderLease>,
    executor: Handle,
}

impl NativeProviderIo {
    /// Captures the actual lease and executor without I/O or another runtime.
    pub fn new(lease: Arc<NativeProviderLease>, executor: Handle) -> Self {
        Self { lease, executor }
    }

    /// Acquire the shared permit before dispatch, then retain both permit and
    /// lease inside the queued/running physical closure. Dropping an observer
    /// cannot cancel already dispatched blocking work or release its ownership.
    /// A later recognition read must use this same owner and waits behind it.
    ///
    /// The callback must finish its owned synchronous work before returning.
    /// Its result determines publication outcomes; this layer only classifies
    /// dispatch/join failure. Closure completion is not proof that an external
    /// service settled an uncertain request, or a distributed takeover fence.
    pub fn run<T, F>(&self, work: F) -> BoxFuture<'static, Result<T, StoreWriteError>>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        let lease = self.lease.clone();
        let executor = self.executor.clone();
        Box::pin(async move {
            let permit = lease.serial.clone().acquire_owned().await.map_err(|_| {
                StoreWriteError::NotApplied(failure("native provider I/O is closed"))
            })?;
            executor
                .spawn_blocking(move || {
                    let _lease = lease;
                    let _permit = permit;
                    work()
                })
                .await
                .map_err(|_| {
                    StoreWriteError::Unknown(failure("native provider I/O did not finish"))
                })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::FutureExt;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn the_real_lease_preserves_file_contents_and_excludes_another_owner() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("owner.lock");
        std::fs::write(&path, "preserved metadata").unwrap();
        let lease = NativeProviderLease::acquire(&path).unwrap();
        assert!(NativeProviderLease::acquire(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"preserved metadata");
        drop(lease);
        assert!(NativeProviderLease::acquire(&path).is_ok());
        assert_eq!(std::fs::read(&path).unwrap(), b"preserved metadata");
        assert!(NativeProviderLease::acquire(directory.path()).is_err());
        assert!(NativeProviderLease::acquire(directory.path().join("missing/lock")).is_err());
        assert!(!directory.path().join("missing").exists());
    }

    #[test]
    fn shared_ownership_prevents_cancelled_waiters_from_dispatching_and_orders_reads() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(1)
            .build()
            .unwrap();
        let cancelled_work_ran = Arc::new(AtomicBool::new(false));
        runtime.block_on(async {
            let directory = tempfile::tempdir().unwrap();
            let lease = NativeProviderLease::acquire(directory.path().join("owner.lock")).unwrap();
            let first = NativeProviderIo::new(lease.clone(), Handle::current());
            let second = NativeProviderIo::new(lease, Handle::current());
            let written = Arc::new(AtomicBool::new(false));
            let changed = written.clone();
            let (started, ready) = tokio::sync::oneshot::channel();
            let (release, wait) = std::sync::mpsc::channel();
            assert!(
                first
                    .run(move || {
                        let _ = started.send(());
                        wait.recv().unwrap();
                        changed.store(true, Ordering::SeqCst);
                    })
                    .now_or_never()
                    .is_none()
            );
            ready.await.unwrap();
            let observed = cancelled_work_ran.clone();
            // This observer disappears while awaiting the shared permit. Its
            // callback must never enter the occupied blocking queue.
            assert!(
                second
                    .run(move || observed.store(true, Ordering::SeqCst))
                    .now_or_never()
                    .is_none()
            );
            let mut recognition = second.run(move || written.load(Ordering::SeqCst));
            assert!(futures::poll!(recognition.as_mut()).is_pending());
            release.send(()).unwrap();
            assert!(
                recognition
                    .await
                    .unwrap_or_else(|_| panic!("recognition failed"))
            );
        });
        // Runtime drop joins any incorrectly dispatched detached blocking work;
        // absence is checked after that positive drain, without a timing guess.
        drop(runtime);
        assert!(!cancelled_work_ran.load(Ordering::SeqCst));
    }

    #[test]
    fn queued_work_keeps_its_real_lease_after_every_observer_and_handle_is_dropped() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(1)
            .build()
            .unwrap();
        runtime.block_on(async {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("owner.lock");
            let lease = NativeProviderLease::acquire(&path).unwrap();
            let io = NativeProviderIo::new(lease, Handle::current());
            let (started, ready) = tokio::sync::oneshot::channel();
            let (release, wait) = std::sync::mpsc::channel();
            let occupied = tokio::task::spawn_blocking(move || {
                let _ = started.send(());
                wait.recv().unwrap();
            });
            ready.await.unwrap();
            let (completed, finished) = tokio::sync::oneshot::channel();
            // The sole blocking thread is occupied, so this poll queues work.
            assert!(io.run(move || completed.send(())).now_or_never().is_none());
            drop(io);
            assert!(NativeProviderLease::acquire(&path).is_err());
            release.send(()).unwrap();
            occupied.await.unwrap();
            finished.await.unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    if NativeProviderLease::acquire(&path).is_ok() {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
        });
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_lost_physical_completion_is_unknown_and_does_not_leak_the_permit() {
        let directory = tempfile::tempdir().unwrap();
        let lease = NativeProviderLease::acquire(directory.path().join("owner.lock")).unwrap();
        let io = NativeProviderIo::new(lease, Handle::current());
        assert!(matches!(
            io.run(|| panic!("fixture operation failed")).await,
            Err(StoreWriteError::Unknown(_))
        ));
        assert!(
            io.run(|| true)
                .await
                .unwrap_or_else(|_| panic!("read failed"))
        );
    }
}
