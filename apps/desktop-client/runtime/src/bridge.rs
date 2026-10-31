//! Private WebView attachment to the existing desktop worker and Tembra host.
use crate::Node;
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    ffi::c_void,
};
use swarmie_node::PrivatePort;
use zeroize::Zeroizing;

const MAX_FRAME: usize = 400_000;

pub struct NativeRuntime {
    port: PrivatePort,
    generation: Cell<u64>,
    failed: Cell<bool>,
    output: RefCell<VecDeque<Zeroizing<Vec<u8>>>>,
}

impl NativeRuntime {
    fn open(node: &Node) -> Result<Self, ()> {
        let port = node.private_port();
        let generation = port.open().map_err(|_| ())?;
        Ok(Self {
            port,
            generation: Cell::new(generation),
            failed: Cell::new(false),
            output: RefCell::new(VecDeque::new()),
        })
    }
}

impl Drop for NativeRuntime {
    fn drop(&mut self) {
        self.port.close(self.generation.get());
    }
}

/// Retire a failed native serialization without replaying any pending operation.
/// # Safety
/// `handle` is live and called on its owning UI thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn swarmie_native_disconnect(handle: *mut c_void) {
    if let Some(runtime) = unsafe { handle.cast::<NativeRuntime>().as_ref() } {
        runtime.failed.set(true);
        runtime.output.borrow_mut().clear();
        runtime.port.close(runtime.generation.get());
    }
}

/// Attach the private shell to the existing host. The native embedding authenticates its origin.
/// # Safety
/// `desktop` must be live until this attachment is closed, on the same owning UI thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn swarmie_native_start(desktop: *const Node) -> *mut c_void {
    let Some(node) = (unsafe { desktop.as_ref() }) else {
        return std::ptr::null_mut();
    };
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| NativeRuntime::open(node)))
        .ok()
        .and_then(Result::ok)
        .map_or(std::ptr::null_mut(), |runtime| {
            Box::into_raw(Box::new(runtime)).cast()
        })
}

/// Replace only the private byte attachment, retaining the application vault owner.
/// # Safety
/// `handle` is live and called on its owning UI thread, before the new peer sends frames.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn swarmie_native_connect(handle: *mut c_void) -> bool {
    let Some(runtime) = (unsafe { handle.cast::<NativeRuntime>().as_ref() }) else {
        return false;
    };
    let Ok(generation) = runtime.port.open() else {
        return false;
    };
    runtime.generation.set(generation);
    runtime.output.borrow_mut().clear();
    runtime.failed.set(false);
    true
}

/// # Safety
/// `handle` is live; `bytes` points to `length` initialized bytes for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn swarmie_native_send(
    handle: *mut c_void,
    bytes: *const u8,
    length: usize,
) -> bool {
    let Some(runtime) = (unsafe { handle.cast::<NativeRuntime>().as_ref() }) else {
        return false;
    };
    if bytes.is_null() || length > MAX_FRAME || runtime.failed.get() {
        return false;
    }
    let bytes = unsafe { std::slice::from_raw_parts(bytes, length) };
    if runtime.port.send(runtime.generation.get(), bytes).is_err() {
        runtime.failed.set(true);
        return false;
    }
    true
}

/// Copy one Tembra frame. Zero means no pending frame; -1 means closed or undersized.
/// # Safety
/// `handle` is live; `output` is writable for `capacity` bytes on the owning UI thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn swarmie_native_receive(
    handle: *mut c_void,
    output: *mut u8,
    capacity: usize,
) -> isize {
    let Some(runtime) = (unsafe { handle.cast::<NativeRuntime>().as_ref() }) else {
        return -1;
    };
    if output.is_null() || runtime.failed.get() {
        return -1;
    }
    let mut pending = runtime.output.borrow_mut();
    if pending.is_empty() {
        match runtime.port.poll(runtime.generation.get()) {
            Ok(frames) => pending.extend(frames.into_iter().map(Zeroizing::new)),
            Err(_) => {
                runtime.failed.set(true);
                return -1;
            }
        }
    }
    match pending.pop_front() {
        Some(frame) if frame.len() <= capacity => {
            unsafe {
                std::ptr::copy_nonoverlapping(frame.as_ptr(), output, frame.len());
            }
            frame.len() as isize
        }
        Some(_) => {
            pending.clear();
            runtime.failed.set(true);
            -1
        }
        None => 0,
    }
}

/// # Safety
/// Consume exactly one start result with no concurrent attachment operations remaining.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn swarmie_native_close(handle: *mut c_void) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle.cast::<NativeRuntime>()) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn request(runtime: &NativeRuntime, service: &str) -> Value {
        let pointer = (runtime as *const NativeRuntime).cast_mut().cast();
        let bytes = serde_json::to_vec(
            &json!({"id":1,"op":"get","payload":{"service":service,"key":["main"]}}),
        )
        .unwrap();
        assert!(unsafe { swarmie_native_send(pointer, bytes.as_ptr(), bytes.len()) });
        let mut output = Zeroizing::new(vec![0; MAX_FRAME]);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            let size =
                unsafe { swarmie_native_receive(pointer, output.as_mut_ptr(), output.len()) };
            assert!(size >= 0);
            if size > 0 {
                return serde_json::from_slice(&output[..size as usize]).unwrap();
            }
            std::thread::yield_now();
        }
        panic!("private native response timed out");
    }

    #[test]
    fn private_and_public_attachments_share_one_host_and_have_separate_admission_and_replacement() {
        let directory = tempfile::tempdir().unwrap();
        let node = Node::open(directory.path().into()).unwrap();
        let public_channels = (0..8)
            .map(|_| node.open_shared_channel().unwrap())
            .collect::<Vec<_>>();
        assert!(node.open_shared_channel().is_err());
        let runtime = NativeRuntime::open(&node).unwrap();
        assert!(
            public_channels
                .iter()
                .all(|channel| node.poll(*channel).is_ok())
        );
        let pointer = (&runtime as *const NativeRuntime).cast_mut().cast();
        let public = node.open_channel().unwrap();
        let private_generation = runtime.generation.get();
        assert!(node.poll(private_generation).is_err());
        assert!(node.frame(private_generation, vec![]).is_err());
        node.close_channel(private_generation);
        assert!(runtime.port.poll(public).is_err());
        assert!(runtime.port.send(public, b"{}").is_err());
        runtime.port.close(public);
        assert!(node.poll(public).is_ok());
        let response = request(&runtime, swarmie_node::PRIVATE_CONTROL_SERVICE);
        assert!(response.get("error").is_none(), "{response}");
        let safe = serde_json::to_string(&response).unwrap();
        assert!(safe.contains("signed-out"));
        assert!(!safe.contains("access_token"));
        assert!(!safe.contains("refresh_token"));
        let forbidden = request(&runtime, "swarmie.node");
        assert_eq!(forbidden["error"]["code"], "forbidden");
        assert!(unsafe { swarmie_native_connect(pointer) });
        assert!(
            node.poll(public).is_ok(),
            "private reconnect retains the ordinary client"
        );
        let renewed_public = node.open_channel().unwrap();
        assert!(node.poll(public).is_err());
        assert!(
            request(&runtime, swarmie_node::PRIVATE_CONTROL_SERVICE)
                .get("error")
                .is_none()
        );
        node.frame(renewed_public, serde_json::to_vec(&json!({"id":1,"op":"get","payload":{"service":swarmie_node::PRIVATE_CONTROL_SERVICE,"key":["main"]}})).unwrap()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let refused = loop {
            if let Some(bytes) = node.poll(renewed_public).unwrap().pop() {
                break serde_json::from_slice::<Value>(&bytes).unwrap();
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        };
        assert_eq!(refused["error"]["code"], "forbidden");
        let configuration =
            std::fs::read(directory.path().join("vault/vault-configuration.json")).unwrap();
        assert!(!String::from_utf8(configuration).unwrap().contains("token"));
        unsafe {
            swarmie_native_disconnect(pointer);
        }
        assert!(runtime.failed.get());
        assert!(unsafe { swarmie_native_connect(pointer) });
        assert!(
            request(&runtime, swarmie_node::PRIVATE_CONTROL_SERVICE)
                .get("error")
                .is_none()
        );
    }
}
