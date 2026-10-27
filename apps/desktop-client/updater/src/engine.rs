use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Sender},
};

use velopack::{
    HttpOptions, UpdateCheck, UpdateInfo, UpdateManager, VelopackApp, sources::HttpSource,
};

/// Stable phase values shared with the desktop host's C boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Phase {
    Disabled,
    Checking,
    Current,
    Available,
    Downloading,
    Restarting,
    Failed,
}

/// A bounded owned snapshot; neither field contains credentials or callbacks.
#[derive(Clone)]
#[repr(C)]
pub struct Snapshot {
    pub phase: Phase,
    pub version: [u8; 96],
    pub error: [u8; 512],
}

impl Snapshot {
    fn new(phase: Phase) -> Self {
        Self {
            phase,
            version: [0; 96],
            error: [0; 512],
        }
    }
}

fn copy_text<const N: usize>(output: &mut [u8; N], value: &str) {
    output.fill(0);
    let mut length = value.len().min(N - 1);
    while !value.is_char_boundary(length) {
        length -= 1;
    }
    output[..length].copy_from_slice(&value.as_bytes()[..length]);
}

enum Command {
    Install,
    Stop,
}

trait Backend: Send + 'static {
    fn check(&mut self) -> Result<Option<String>, String>;
    fn download(&mut self) -> Result<(), String>;
    fn stage_restart(&mut self) -> Result<(), String>;
}

struct InstalledBackend {
    manager: UpdateManager,
    update: Option<UpdateInfo>,
}

impl Backend for InstalledBackend {
    fn check(&mut self) -> Result<Option<String>, String> {
        match self
            .manager
            .check_for_updates()
            .map_err(|error| error.to_string())?
        {
            UpdateCheck::UpdateAvailable(update) => {
                let version = update.TargetFullRelease.Version.to_string();
                self.update = Some(*update);
                Ok(Some(version))
            }
            _ => Ok(None),
        }
    }

    fn download(&mut self) -> Result<(), String> {
        let update = self.update.as_ref().ok_or("No update was selected")?;
        self.manager
            .download_updates(update, None)
            .map_err(|error| error.to_string())
    }

    fn stage_restart(&mut self) -> Result<(), String> {
        let update = self.update.as_ref().ok_or("No update was selected")?;
        self.manager
            .wait_exit_then_apply_updates(update, false, true, std::iter::empty::<&str>())
            .map_err(|error| error.to_string())
    }
}

/// Owns one update worker. Dropping it never grants consent to a staged update.
pub struct Updater {
    state: Arc<Mutex<Snapshot>>,
    commands: Sender<Command>,
    alive: Arc<AtomicBool>,
}

impl Updater {
    /// Process activation must run before the native window or any network work.
    /// An unpackaged development binary has no update manager and performs no check.
    pub fn installed() -> Option<Self> {
        VelopackApp::build().set_auto_apply_on_startup(false).run();
        let url = option_env!("SWARMIE_UPDATE_URL")
            .unwrap_or("https://github.com/syntropika/swarmie/releases/latest/download");
        let source = HttpSource::new_with_options(
            url,
            HttpOptions {
                TimeoutMilliseconds: 120_000,
                ..HttpOptions::default()
            },
        );
        let manager = UpdateManager::new(source, None, None).ok()?;
        Some(Self::spawn(InstalledBackend {
            manager,
            update: None,
        }))
    }

    fn spawn(mut backend: impl Backend) -> Self {
        let state = Arc::new(Mutex::new(Snapshot::new(Phase::Checking)));
        let alive = Arc::new(AtomicBool::new(true));
        let (commands, receiver) = mpsc::channel();
        let worker_state = Arc::clone(&state);
        let worker_alive = Arc::clone(&alive);
        std::thread::spawn(move || {
            let set_phase = |phase, failure: Option<String>| {
                let mut snapshot = worker_state
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                snapshot.phase = phase;
                if let Some(failure) = failure {
                    copy_text(&mut snapshot.error, &failure);
                }
            };
            match backend.check() {
                Ok(Some(version)) => {
                    let mut snapshot = worker_state
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    snapshot.phase = Phase::Available;
                    copy_text(&mut snapshot.version, &version);
                }
                Ok(None) => set_phase(Phase::Current, None),
                Err(error) => set_phase(Phase::Failed, Some(error)),
            }
            while let Ok(command) = receiver.recv() {
                if matches!(command, Command::Stop) {
                    break;
                }
                if !worker_alive.load(Ordering::Acquire) {
                    break;
                }
                if let Err(error) = backend.download() {
                    set_phase(Phase::Failed, Some(error));
                    continue;
                }
                if !worker_alive.load(Ordering::Acquire) {
                    break;
                }
                match backend.stage_restart() {
                    Ok(()) => set_phase(Phase::Restarting, None),
                    Err(error) => set_phase(Phase::Failed, Some(error)),
                }
            }
        });
        Self {
            state,
            commands,
            alive,
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    /// Accept only one explicit install request for an available release.
    pub fn install(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.phase != Phase::Available {
            return false;
        }
        state.phase = Phase::Downloading;
        self.commands.send(Command::Install).is_ok()
    }
}

impl Drop for Updater {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::Release);
        let _ = self.commands.send(Command::Stop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::atomic::AtomicUsize,
        time::{Duration, Instant},
    };

    struct Probe {
        calls: Arc<AtomicUsize>,
        fail: bool,
    }
    impl Backend for Probe {
        fn check(&mut self) -> Result<Option<String>, String> {
            Ok(Some("0.2.0".into()))
        }
        fn download(&mut self) -> Result<(), String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                Err("Download failed".into())
            } else {
                Ok(())
            }
        }
        fn stage_restart(&mut self) -> Result<(), String> {
            self.calls.fetch_add(10, Ordering::SeqCst);
            Ok(())
        }
    }

    fn await_phase(updater: &Updater, phase: Phase) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while updater.snapshot().phase != phase {
            assert!(Instant::now() < deadline, "update phase timed out");
            std::thread::yield_now();
        }
    }

    #[test]
    fn discovery_does_not_download_or_restart_without_consent() {
        let calls = Arc::new(AtomicUsize::new(0));
        let updater = Updater::spawn(Probe {
            calls: calls.clone(),
            fail: false,
        });
        await_phase(&updater, Phase::Available);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(updater.install());
        assert!(!updater.install());
        await_phase(&updater, Phase::Restarting);
        assert_eq!(calls.load(Ordering::SeqCst), 11);
    }

    #[test]
    fn download_failure_never_stages_replacement() {
        let calls = Arc::new(AtomicUsize::new(0));
        let updater = Updater::spawn(Probe {
            calls: calls.clone(),
            fail: true,
        });
        await_phase(&updater, Phase::Available);
        assert!(updater.install());
        await_phase(&updater, Phase::Failed);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
