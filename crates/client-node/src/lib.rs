//! Node-API binding; transport, deadlines, and handle ownership live in Rust.

use napi_derive::napi;

/// Execute a nonblocking control operation such as prepare, cancel, or release.
#[napi]
pub fn control(command: String) -> String {
    swarmie_client::execute(&command)
}

/// Run blocking C-boundary dispatch away from the JavaScript event loop.
#[napi]
pub async fn execute(command: String) -> napi::Result<String> {
    napi::tokio::task::spawn_blocking(move || swarmie_client::execute(&command))
        .await
        .map_err(|_| napi::Error::from_reason("Native client worker failed"))
}
