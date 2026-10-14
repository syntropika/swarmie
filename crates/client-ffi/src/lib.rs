//! C ABI lowering for the versioned native WIT client boundary.

use std::ffi::{CStr, CString, c_char};

/// ABI version of the ownership and JSON lowering contract.
#[unsafe(no_mangle)]
pub extern "C" fn swarmie_client_abi_version() -> u32 {
    1
}

/// Execute one UTF-8 JSON operation. The returned allocation belongs to the caller
/// and must be freed exactly once with swarmie_client_free. Null input is rejected.
///
/// # Safety
/// Non-null input must point to a readable NUL-terminated string for the entire call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn swarmie_client_execute(command: *const c_char) -> *mut c_char {
    let result = std::panic::catch_unwind(|| {
        if command.is_null() { return swarmie_client::execute(""); }
        // SAFETY: the caller supplies a readable NUL-terminated string by this ABI.
        match unsafe { CStr::from_ptr(command) }.to_str() {
            Ok(command) => swarmie_client::execute(command),
            Err(_) => swarmie_client::execute(""),
        }
    }).unwrap_or_else(|_| "{\"error\":{\"kind\":\"internal\",\"message\":\"Native client internal failure\",\"code\":null,\"data_json\":null}}".into());
    // JSON escapes embedded NUL bytes, so this conversion cannot fail.
    CString::new(result)
        .expect("JSON contains no NUL bytes")
        .into_raw()
}

/// Release one returned string; null is accepted.
///
/// # Safety
/// A non-null pointer must be a live allocation returned by swarmie_client_execute,
/// and may not be read or freed again after this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn swarmie_client_free(result: *mut c_char) {
    if !result.is_null() {
        // SAFETY: the caller transfers a live allocation from execute exactly once.
        drop(unsafe { CString::from_raw(result) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocations_round_trip_and_null_input_is_rejected() {
        let command = CString::new("{\"action\":\"decode\",\"response_json\":\"{\\\"jsonrpc\\\":\\\"2.0\\\",\\\"id\\\":\\\"a\\\",\\\"result\\\":null}\",\"request_id\":\"a\"}").unwrap();
        // SAFETY: input remains live and each returned allocation is freed once.
        unsafe {
            let reply = swarmie_client_execute(command.as_ptr());
            assert_eq!(CStr::from_ptr(reply).to_str().unwrap(), "{\"ok\":\"null\"}");
            swarmie_client_free(reply);
            let reply = swarmie_client_execute(std::ptr::null());
            assert!(
                CStr::from_ptr(reply)
                    .to_str()
                    .unwrap()
                    .contains("invalid_input")
            );
            swarmie_client_free(reply);
            swarmie_client_free(std::ptr::null_mut());
        }
    }
}
