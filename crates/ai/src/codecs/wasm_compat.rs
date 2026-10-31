use bytes::Bytes;
use std::pin::Pin;

use futures::Stream;

// Browser markers assume single-threaded execution; atomics would invalidate
// that assumption. Relaxed bounds do not make non-Send handlers thread-safe.
#[cfg(all(
    target_arch = "wasm32",
    target_os = "unknown",
    target_feature = "atomics"
))]
compile_error!(
    "The HTTP codec layer does not support threaded wasm (`+atomics`): its wasm-compat markers assume a \
     single-threaded target"
);

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not `Send`, and native decoding requires `Send`",
    label = "not `Send`",
    note = "handlers and transports run inside a driver's task: hold shared state behind an `Arc` (never an `Rc`), or use a `!Send` value only on browser wasm, where this marker is a no-op"
)]
pub trait WasmCompatSend: Send {}
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub trait WasmCompatSend {}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
impl<T> WasmCompatSend for T where T: Send {}
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
impl<T> WasmCompatSend for T {}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub trait WasmCompatSendStream:
    Stream<Item = Result<Bytes, crate::codecs::http_client::Error>> + Send
{
    type InnerItem: Send;
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub trait WasmCompatSendStream:
    Stream<Item = Result<Bytes, crate::codecs::http_client::Error>>
{
    type InnerItem;
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
impl<T> WasmCompatSendStream for T
where
    T: Stream<Item = Result<Bytes, crate::codecs::http_client::Error>> + Send,
{
    type InnerItem = Result<Bytes, crate::codecs::http_client::Error>;
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
impl<T> WasmCompatSendStream for T
where
    T: Stream<Item = Result<Bytes, crate::codecs::http_client::Error>>,
{
    type InnerItem = Result<Bytes, crate::codecs::http_client::Error>;
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not `Sync`, and native decoding requires `Sync`",
    label = "not `Sync`",
    note = "handlers and transports are shared between a driver and its in-flight tasks: use `Mutex`/atomics instead of `Cell`/`RefCell`, or use a `!Sync` value only on browser wasm, where this marker is a no-op"
)]
pub trait WasmCompatSync: Sync {}
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub trait WasmCompatSync {}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
impl<T> WasmCompatSync for T where T: Sync {}
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
impl<T> WasmCompatSync for T {}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub type WasmBoxedFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub type WasmBoxedFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub type WasmBoxedStream<'a, T> = Pin<Box<dyn Stream<Item = T> + Send + 'a>>;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub type WasmBoxedStream<'a, T> = Pin<Box<dyn Stream<Item = T> + 'a>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Elapsed;

impl std::fmt::Display for Elapsed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("future timed out")
    }
}

impl std::error::Error for Elapsed {}

pub async fn timeout<F>(duration: std::time::Duration, future: F) -> Result<F::Output, Elapsed>
where
    F: Future,
{
    use futures::future::{Either, select};

    let delay = futures_timer::Delay::new(duration);
    futures::pin_mut!(future);
    futures::pin_mut!(delay);
    match select(future, delay).await {
        Either::Left((output, _)) => Ok(output),
        Either::Right(((), _)) => Err(Elapsed),
    }
}

pub async fn sleep(duration: std::time::Duration) {
    futures_timer::Delay::new(duration).await;
}
