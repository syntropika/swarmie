use crate::DesktopNode;
use serde::Deserialize;
use std::{
    ffi::{CStr, c_char},
    path::PathBuf,
};

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Input {
    Describe,
    Open,
    Send { generation: u64, frame: Vec<u8> },
    Poll { generation: u64 },
    Close { generation: u64 },
}

/// Open the host-owned runtime with its persistent state directory.
/// # Safety
/// State must be a valid NUL-terminated UTF-8 path for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn swarmie_desktop_runtime_start(state: *const c_char) -> *mut DesktopNode {
    let opened = std::panic::catch_unwind(|| {
        if state.is_null() {
            return None;
        }
        let state = unsafe { CStr::from_ptr(state) }.to_str().ok()?;
        DesktopNode::open(PathBuf::from(state)).ok()
    });
    opened
        .ok()
        .flatten()
        .map_or(std::ptr::null_mut(), |value| Box::into_raw(Box::new(value)))
}

/// Exchange transport bytes through the origin-restricted native bridge.
/// # Safety
/// Handle must be live. Input must contain length readable bytes. Output must be
/// writable for capacity bytes. Buffers must not overlap the handle.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn swarmie_desktop_runtime_exchange(
    handle: *const DesktopNode,
    input: *const u8,
    length: usize,
    output: *mut u8,
    capacity: usize,
) -> usize {
    if input.is_null() || output.is_null() || length > 512 * 1024 {
        return 0;
    }
    let Some(node) = (unsafe { handle.as_ref() }) else {
        return 0;
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let input: Input =
            serde_json::from_slice(unsafe { std::slice::from_raw_parts(input, length) })
                .map_err(|_| "invalid native transport request".to_owned())?;
        match input {
            Input::Describe => Ok(node.descriptor()),
            Input::Open => node.open_channel().map(serde_json::Value::from),
            Input::Send { generation, frame } => node
                .frame(generation, frame)
                .map(|()| serde_json::Value::Null),
            Input::Poll { generation } => node
                .poll(generation)
                .map(|frames| serde_json::json!(frames)),
            Input::Close { generation } => {
                node.close_channel(generation);
                Ok(serde_json::Value::Null)
            }
        }
    }));
    let value = match result {
        Ok(Ok(value)) => serde_json::json!({"ok": true, "value": value}),
        Ok(Err(message)) => serde_json::json!({"ok": false, "error": message}),
        Err(_) => serde_json::json!({"ok": false, "error": "native runtime failed"}),
    };
    let Ok(bytes) = serde_json::to_vec(&value) else {
        return 0;
    };
    if bytes.len() > capacity {
        return 0;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), output, bytes.len());
    }
    bytes.len()
}

/// Close the native worker and await Tembra's physical cleanup.
/// # Safety
/// Handle must be null or one live owned handle. Release it once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn swarmie_desktop_runtime_close(handle: *mut DesktopNode) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle) });
    }
}
