use crate::{Snapshot, Updater};

/// Run process activation and create the installed app's update worker, if available.
#[unsafe(no_mangle)]
pub extern "C" fn swarmie_updater_start() -> *mut Updater {
    Updater::installed().map_or(std::ptr::null_mut(), |updater| {
        Box::into_raw(Box::new(updater))
    })
}

/// Copy the current bounded snapshot into the caller's storage.
///
/// # Safety
/// `handle` must be live, and `output` must be aligned writable Snapshot storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn swarmie_updater_snapshot(handle: *const Updater, output: *mut Snapshot) {
    if let (Some(handle), Some(output)) = (unsafe { handle.as_ref() }, unsafe { output.as_mut() }) {
        *output = handle.snapshot();
    }
}

/// Record explicit user consent to download and stage the discovered release.
///
/// # Safety
/// `handle` must refer to a live updater created by `swarmie_updater_start`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn swarmie_updater_install(handle: *const Updater) -> bool {
    unsafe { handle.as_ref() }.is_some_and(Updater::install)
}

/// Release ownership without applying pending downloads on another launch.
///
/// # Safety
/// `handle` must be null or a live handle; release it once and never use it afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn swarmie_updater_close(handle: *mut Updater) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle) });
    }
}
