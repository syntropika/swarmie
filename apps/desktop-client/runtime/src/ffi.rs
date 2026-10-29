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
    OpenShared,
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

/// Exchange transport bytes through an embedding-owned native bridge.
/// The embedding must authenticate callers and restrict browser origins.
/// A poll requires an output buffer of at least 512 KiB.
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
            Input::OpenShared => node.open_shared_channel().map(serde_json::Value::from),
            Input::Send { generation, frame } => node
                .frame(generation, frame)
                .map(|()| serde_json::Value::Null),
            Input::Poll { generation } => {
                if capacity < 512 * 1024 {
                    return Err("native poll output buffer is too small".into());
                }
                node.poll(generation)
                    .map(|frames| serde_json::json!(frames))
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_small_poll_buffer_does_not_consume_pending_frames() {
        let directory = tempfile::tempdir().unwrap();
        let node = DesktopNode::open(directory.path().into()).unwrap();
        let generation = node.open_channel().unwrap();
        node.frame(
            generation,
            serde_json::to_vec(&serde_json::json!({
                "id": 1, "op": "get", "payload": {"service": "swarmie.node", "key": ["main"]}
            }))
            .unwrap(),
        )
        .unwrap();
        let input =
            serde_json::to_vec(&serde_json::json!({"action": "poll", "generation": generation}))
                .unwrap();
        let exchange = |capacity| {
            let mut output = vec![0; capacity];
            // The node and both disjoint buffers remain live for this call.
            let length = unsafe {
                swarmie_desktop_runtime_exchange(
                    &node,
                    input.as_ptr(),
                    input.len(),
                    output.as_mut_ptr(),
                    capacity,
                )
            };
            serde_json::from_slice::<serde_json::Value>(&output[..length]).unwrap()
        };
        assert_eq!(
            exchange(1024)["error"],
            "native poll output buffer is too small"
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            let response = exchange(512 * 1024);
            assert_eq!(response["ok"], true);
            if response["value"]
                .as_array()
                .is_some_and(|frames| !frames.is_empty())
            {
                assert_eq!(response["value"].as_array().unwrap().len(), 1);
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the pending response was lost"
            );
            std::thread::yield_now();
        }
    }
}
