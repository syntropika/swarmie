//! Setup-only encrypted byte carrier. Tembra RPC never runs on this channel.
use crate::*;
use std::{
    cell::Cell,
    io,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream, ReadBuf};
use uuid::Uuid;

/// Bounded Cloud ciphertext stream. Drop cancels its physical forwarding task.
/// Only a completed purpose-separated BootstrapStream may authenticate its bytes.
pub struct CloudStream {
    inner: DuplexStream,
    worker: tokio::task::JoinHandle<()>,
    delivery: Rc<Delivery>,
    written: u64,
}
#[derive(Default)]
struct Delivery {
    bytes: Cell<u64>,
    closed: Cell<bool>,
    failed: Cell<bool>,
    wake: futures::task::AtomicWaker,
}

impl CloudStream {
    fn delivered(&self, cx: &mut Context<'_>, closing: bool) -> Poll<io::Result<()>> {
        self.delivery.wake.register(cx.waker());
        if self.delivery.failed.get() {
            return Poll::Ready(Err(io::Error::from(io::ErrorKind::BrokenPipe)));
        }
        if self.delivery.bytes.get() >= self.written && (!closing || self.delivery.closed.get()) {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }
}

impl Drop for CloudStream {
    fn drop(&mut self) {
        self.worker.abort();
    }
}
impl AsyncRead for CloudStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        output: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(cx, output)
    }
}
impl AsyncWrite for CloudStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_write(cx, bytes) {
            Poll::Ready(Ok(length)) => {
                this.written += length as u64;
                Poll::Ready(Ok(length))
            }
            other => other,
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.get_mut().delivered(cx, false)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        match Pin::new(&mut this.inner).poll_shutdown(cx) {
            Poll::Ready(Ok(())) => this.delivered(cx, true),
            other => other,
        }
    }
}

impl CloudDevice {
    /// Send or recognize an exact purpose-tagged carrier packet. It grants no link authority.
    pub async fn carrier(&self, action: CarrierAction) -> Result<CarrierReply> {
        let registration = self.registered().await?;
        let mut request = CarrierRequest {
            account: self.account().identity().account,
            issuer: self.account().endpoints().api.trim_end_matches('/').into(),
            device: registration.device,
            generation: registration.generation,
            action,
            signature: vec![],
        };
        request.signature = self.keys().sign(&request.signing_bytes()?);
        match self.account().post("/api/devices/carrier", &request).await {
            Ok(reply) => Ok(reply),
            Err(error) => {
                if matches!(
                    error,
                    swarmie_cloud_account::Error::Forbidden | swarmie_cloud_account::Error::Revoked
                ) {
                    self.invalidate_registration().await;
                }
                Err(super::cloud::account_error(error))
            }
        }
    }

    /// Publish only the random locator and native endpoint proof, never the private offer capability.
    pub async fn publish_offer(&self, offer: &LinkOffer, lifetime_seconds: u16) -> Result<u64> {
        if offer.target() != self.keys().public() {
            return Err(Error::Forbidden);
        }
        match self
            .carrier(CarrierAction::Open {
                offer: offer.rendezvous(),
                lifetime_seconds,
            })
            .await?
        {
            CarrierReply::Opened { expires_at } => Ok(expires_at),
            _ => Err(Error::Integrity),
        }
    }

    /// Mark only the target-verified claim; the bounded review lifetime is separate from discovery.
    pub(crate) async fn claim_stream(&self, offer: &LinkOffer, connection: Uuid) -> Result<()> {
        self.accepted_carrier(CarrierAction::Claim {
            offer: offer.rendezvous(),
            connection,
            lifetime_seconds: 600,
        })
        .await
    }

    /// Target-owned candidate discovery. Candidates remain unauthenticated until TLS and HMAC pass.
    pub async fn pending_streams(&self, offer: &LinkOffer) -> Result<Vec<Uuid>> {
        if offer.target() != self.keys().public() {
            return Err(Error::Forbidden);
        }
        match self
            .carrier(CarrierAction::Pending {
                offer: offer.rendezvous(),
            })
            .await?
        {
            CarrierReply::Pending { connections } if connections.len() <= 8 => Ok(connections),
            _ => Err(Error::Integrity),
        }
    }

    /// Open the candidate and perform target-pinned mutual bootstrap TLS; no setup metadata is sent.
    pub async fn connect_bootstrap(
        &self,
        offer: &LinkOffer,
        connection: Uuid,
    ) -> Result<swarmie_permissions::transport::BootstrapStream<CloudStream>> {
        if connection.is_nil() {
            return Err(Error::Invalid);
        }
        self.accepted_carrier(CarrierAction::Connect {
            offer: offer.rendezvous(),
            connection,
            target: DeviceId::of(&offer.target()),
        })
        .await?;
        swarmie_permissions::transport::BootstrapStream::connect(
            self.stream(offer.rendezvous(), connection),
            &self.keys().transport(),
            offer.target(),
            offer.rendezvous(),
        )
        .await
        .map_err(|_| Error::Forbidden)
    }

    /// Target-owned TLS accept. The caller must verify the private capability and account lookup next.
    pub async fn accept_bootstrap(
        &self,
        offer: &LinkOffer,
        connection: Uuid,
    ) -> Result<swarmie_permissions::transport::BootstrapStream<CloudStream>> {
        if connection.is_nil() || offer.target() != self.keys().public() {
            return Err(Error::Invalid);
        }
        swarmie_permissions::transport::BootstrapStream::accept(
            self.stream(offer.rendezvous(), connection),
            &self.keys().transport(),
            offer.rendezvous(),
        )
        .await
        .map_err(|_| Error::Forbidden)
    }

    async fn accepted_carrier(&self, action: CarrierAction) -> Result<()> {
        // These retries recognize the same immutable ciphertext packet index. They cannot repeat
        // a model/tool effect, consume the offer capability or admit a control/vault grant.
        for attempt in 0..3 {
            match self.carrier(action.clone()).await {
                Ok(CarrierReply::Accepted) => return Ok(()),
                Ok(_) => return Err(Error::Integrity),
                Err(Error::Unavailable | Error::Unknown | Error::Stale) if attempt < 2 => {
                    tokio::time::sleep(Duration::from_millis(30)).await
                }
                Err(error) => return Err(error),
            }
        }
        Err(Error::Unavailable)
    }

    fn stream(&self, offer: [u8; 16], connection: Uuid) -> CloudStream {
        let (inner, carrier) = tokio::io::duplex(32 * 1024);
        let owner = self.clone();
        let delivery = Rc::new(Delivery::default());
        let progress = delivery.clone();
        let worker = tokio::task::spawn_local(async move {
            let (mut local_read, mut local_write) = tokio::io::split(carrier);
            let sending = async {
                for index in 0..128 {
                    let mut bytes = vec![0; 16 * 1024];
                    let length = local_read
                        .read(&mut bytes)
                        .await
                        .map_err(|_| Error::Unavailable)?;
                    bytes.truncate(length);
                    owner
                        .accepted_carrier(CarrierAction::Send {
                            offer,
                            connection,
                            index,
                            bytes,
                        })
                        .await?;
                    progress.bytes.set(progress.bytes.get() + length as u64);
                    if length == 0 {
                        progress.closed.set(true);
                    }
                    progress.wake.wake();
                    if length == 0 {
                        return Ok::<(), Error>(());
                    }
                }
                Err(Error::Exhausted)
            };
            let receiving = async {
                let mut idle_delay = 25;
                let mut read_failures = 0;
                for index in 0..128 {
                    loop {
                        let response = match owner
                            .carrier(CarrierAction::Read {
                                offer,
                                connection,
                                index,
                            })
                            .await
                        {
                            Ok(reply) => {
                                read_failures = 0;
                                reply
                            }
                            Err(Error::Unavailable) if read_failures < 3 => {
                                read_failures += 1;
                                tokio::time::sleep(Duration::from_millis(100 * read_failures))
                                    .await;
                                continue;
                            }
                            Err(error) => return Err(error),
                        };
                        match response {
                            CarrierReply::Packet {
                                index: actual,
                                bytes,
                            } if actual == index => match bytes {
                                Some(bytes) if bytes.is_empty() => {
                                    local_write
                                        .shutdown()
                                        .await
                                        .map_err(|_| Error::Unavailable)?;
                                    return Ok::<(), Error>(());
                                }
                                Some(bytes) if bytes.len() <= 16 * 1024 => {
                                    idle_delay = 25;
                                    local_write
                                        .write_all(&bytes)
                                        .await
                                        .map_err(|_| Error::Unavailable)?;
                                    break;
                                }
                                Some(_) => return Err(Error::Integrity),
                                None => {
                                    tokio::time::sleep(Duration::from_millis(idle_delay)).await;
                                    idle_delay = (idle_delay * 2).min(400);
                                }
                            },
                            _ => return Err(Error::Integrity),
                        }
                    }
                }
                Err(Error::Exhausted)
            };
            if tokio::try_join!(sending, receiving).is_err() {
                progress.failed.set(true);
            }
            progress.wake.wake();
        });
        CloudStream {
            inner,
            worker,
            delivery,
            written: 0,
        }
    }
}
