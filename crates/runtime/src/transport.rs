use std::{cell::Cell, net::SocketAddr, rc::Rc, time::Duration};

use futures::{FutureExt, future::Either};
use tembra::core::{
    Driver,
    rpc::{ConnectionOptions, RootOrigin},
};
use tembra_native::{TokioDriver, WebSocketLink, WebSocketOptions, random_incarnation};
use tokio::net::TcpListener;
use tokio_tungstenite::accept_async_with_config;

use crate::Runtime;

type Error = Box<dyn std::error::Error>;

struct Session(Rc<Cell<usize>>);

impl Drop for Session {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

/// Serves actual Tembra RPC over one local WebSocket listener until the stop future resolves.
///
/// Counter access is public to local clients. Administrative methods independently require
/// the supervisor proof. This development embedding admits loopback listeners only; network
/// embeddings must supply their own authenticated transport admission policy.
///
/// # Errors
/// Refuses non-loopback listeners and returns bind, RPC-identity or accept errors. Individual
/// malformed connections are closed and reported without terminating other sessions.
pub async fn serve(
    runtime: &Runtime,
    driver: Rc<TokioDriver>,
    address: SocketAddr,
    stopped: impl std::future::Future<Output = ()>,
) -> Result<(), Error> {
    if !address.ip().is_loopback() {
        return Err("the development listener requires a loopback address".into());
    }
    runtime
        .host
        .core()
        .set_rpc_identity(RootOrigin::new("swarmie-runtime", random_incarnation()?)?)?;
    let listener = TcpListener::bind(address).await?;
    println!(
        "{}",
        serde_json::json!({"ready":true,"address":listener.local_addr()?.to_string()})
    );
    let sessions = Rc::new(Cell::new(0_usize));
    futures::pin_mut!(stopped);
    loop {
        match futures::future::select(listener.accept().boxed_local(), stopped.as_mut()).await {
            Either::Right(_) => return Ok(()),
            Either::Left((accepted, _)) => {
                let (socket, _) = accepted?;
                if sessions.get() >= 32 {
                    drop(socket);
                    continue;
                }
                sessions.set(sessions.get() + 1);
                let session = Session(sessions.clone());
                let host = runtime.host.clone();
                let connection_driver = driver.clone();
                driver.spawn(
                    async move {
                        let _session = session;
                        let result: Result<(), Error> = async {
                            let options = WebSocketOptions::default();
                            let socket = tokio::time::timeout(
                                Duration::from_secs(5),
                                accept_async_with_config(socket, Some(options.protocol_config()?)),
                            )
                            .await??;
                            let (link, principal) = WebSocketLink::authenticate(
                                socket,
                                connection_driver,
                                options,
                                async { Ok(serde_json::json!("local-browser")) },
                            )
                            .await?;
                            let connection = host.core().accept_client(
                                link,
                                principal,
                                ConnectionOptions::default(),
                            )?;
                            connection.closed().await;
                            Ok(())
                        }
                        .await;
                        if let Err(error) = result {
                            eprintln!("connection refused: {error}");
                        }
                    }
                    .boxed_local(),
                );
            }
        }
    }
}
